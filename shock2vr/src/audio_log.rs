//! Ring buffer of recently played sounds, so headless tooling (the debug
//! runtime's `GET /v1/audio/recent`) can verify that a sound schema was
//! actually resolved and played - there is no other way to observe audio
//! without speakers.
//!
//! Beyond "what played", entries carry enough context to debug *overlapping*
//! audio: the simulation time (and 60 Hz frame) of the play, the clip's
//! duration, the audio handle (so a later `StopSound` can be correlated), and
//! the source entity's stable identity (name + template id - runtime entity
//! ids are not stable across runs).
//!
//! `still_playing` is derived at *query* time from simulation time
//! (`sim_time + duration_secs > now`, and not explicitly stopped) rather than
//! from live rodio sink state (`engine::audio::AudioContext`'s
//! `handle_to_sink`, which it retains while `!sink.empty()`): rodio plays on
//! its own thread against the wall clock, so that state is nondeterministic
//! when the debug runtime steps frames faster (or slower) than real time.
//! Entries whose duration is unknown report `still_playing: false`, and a
//! looping sound's duration is one iteration, so it can read as finished while
//! still audible. The buffer is process-global and never cleared, so it can
//! span a level transition; `sim_time` stays monotonic across one.

use std::collections::VecDeque;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use serde::Serialize;

const MAX_ENTRIES: usize = 64;

/// The debug runtime steps at a fixed 60 Hz, so a frame number is a faithful
/// index into a stepped session.
pub(crate) const FRAMES_PER_SECOND: f64 = 60.0;

/// Stable identity of the entity that caused a sound. Runtime entity ids
/// differ every launch, so callers get the symbolic name and template id.
#[derive(Clone, Debug, Serialize)]
pub struct SourceEntity {
    pub name: String,
    pub template_id: Option<i32>,
}

#[derive(Clone, Debug, Serialize)]
pub struct PlayedSound {
    /// Monotonically increasing id, so callers can diff "new since last poll".
    pub sequence: u64,
    /// Simulation time (seconds) at which the sound started.
    pub sim_time: f64,
    /// `sim_time` expressed in fixed 60 Hz frames.
    pub frame: u64,
    /// Resolved sample name (e.g. "bulmet2"), without extension.
    pub sample: String,
    /// Authored Dark-schema volume, in millibels; absent for direct samples.
    pub volume_millibels: Option<i32>,
    /// Linear gain actually assigned to the rodio sink.
    pub gain: f32,
    /// Resolved authored pan, in millibels; absent for direct samples.
    pub pan_millibels: Option<i32>,
    /// False when positional 3D panning superseded the schema pan.
    pub pan_applied: bool,
    /// The schema query's (tag, value) pairs (e.g. event=collision, material=metal).
    pub tags: Vec<(String, String)>,
    pub position: [f32; 3],
    /// Clip length in seconds, when the decoder reports one.
    pub duration_secs: Option<f64>,
    /// The entity that caused the sound, when known.
    pub source_entity: Option<SourceEntity>,
    /// Audio handle id, when the play used one (correlates with `StopSound`).
    pub handle: Option<u64>,
    /// Simulation time at which a `StopSound` cut the play short.
    pub stopped_at_sim_time: Option<f64>,
    /// Derived at query time - see the module docs.
    pub still_playing: bool,
}

/// Arguments for [`record`], grouped so call sites stay readable.
pub struct SoundRecord<'a> {
    pub sample: &'a str,
    pub volume_millibels: Option<i32>,
    pub gain: f32,
    pub pan_millibels: Option<i32>,
    pub pan_applied: bool,
    pub tags: Vec<(String, String)>,
    pub position: [f32; 3],
    pub duration: Option<Duration>,
    pub source_entity: Option<SourceEntity>,
    pub handle: Option<u64>,
}

static RECENT: Mutex<(u64, VecDeque<PlayedSound>)> = Mutex::new((0, VecDeque::new()));

/// Current simulation time, stored as `f64` bits so it can live in an atomic.
static SIM_TIME_BITS: AtomicU64 = AtomicU64::new(0);

/// Publish the simulation clock, once per `Game::update`, so records made
/// anywhere below it can stamp themselves without threading `Time` through.
pub fn set_sim_time(total_secs: f64) {
    SIM_TIME_BITS.store(total_secs.to_bits(), Ordering::Relaxed);
}

/// The most recently published simulation time, in seconds.
fn sim_time() -> f64 {
    f64::from_bits(SIM_TIME_BITS.load(Ordering::Relaxed))
}

/// `sim_time` expressed in fixed 60 Hz frames. Shared with
/// [`crate::message_trace`], which stamps its entries the same way.
pub(crate) fn frame_of(sim_time: f64) -> u64 {
    (sim_time * FRAMES_PER_SECOND).round().max(0.0) as u64
}

/// Record a successfully resolved + played sound.
pub fn record(record: SoundRecord) {
    let sim_time = sim_time();
    let mut guard = RECENT.lock().unwrap();
    let (next_sequence, entries) = &mut *guard;
    *next_sequence += 1;
    entries.push_back(PlayedSound {
        sequence: *next_sequence,
        sim_time,
        frame: frame_of(sim_time),
        sample: record.sample.to_owned(),
        volume_millibels: record.volume_millibels,
        gain: record.gain,
        pan_millibels: record.pan_millibels,
        pan_applied: record.pan_applied,
        tags: record.tags,
        position: record.position,
        duration_secs: record.duration.map(|d| d.as_secs_f64()),
        source_entity: record.source_entity,
        handle: record.handle,
        stopped_at_sim_time: None,
        still_playing: false,
    });
    if entries.len() > MAX_ENTRIES {
        entries.pop_front();
    }
}

/// Mark several handles stopped at once - used for the plays a new play
/// preempts (a single-slot channel, or a reused handle), which never reach
/// `Effect::StopSound`.
pub fn record_stops(handles: &[u64]) {
    for handle in handles {
        record_stop(*handle);
    }
}

/// Mark the most recent (not yet stopped) play of `handle` as stopped, so
/// `still_playing` stops reporting it even though its duration has not elapsed.
pub fn record_stop(handle: u64) {
    let sim_time = sim_time();
    let mut guard = RECENT.lock().unwrap();
    if let Some(entry) = guard
        .1
        .iter_mut()
        .rev()
        .find(|entry| entry.handle == Some(handle) && entry.stopped_at_sim_time.is_none())
    {
        entry.stopped_at_sim_time = Some(sim_time);
    }
}

/// Whether an entry is still audible at simulation time `now`: it has a known
/// duration that has not elapsed, and nothing stopped it early.
fn resolve_still_playing(entry: &PlayedSound, now: f64) -> bool {
    entry.stopped_at_sim_time.is_none()
        && entry
            .duration_secs
            .is_some_and(|duration| entry.sim_time + duration > now)
}

/// The most recent played sounds, oldest first, with `still_playing` resolved
/// against the current simulation time.
pub fn recent() -> Vec<PlayedSound> {
    let now = sim_time();
    RECENT
        .lock()
        .unwrap()
        .1
        .iter()
        .cloned()
        .map(|mut entry| {
            entry.still_playing = resolve_still_playing(&entry, now);
            entry
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(sim_time: f64, duration_secs: Option<f64>) -> PlayedSound {
        PlayedSound {
            sequence: 1,
            sim_time,
            frame: frame_of(sim_time),
            sample: "test".to_owned(),
            volume_millibels: None,
            gain: 1.0,
            pan_millibels: None,
            pan_applied: false,
            tags: vec![],
            position: [0.0, 0.0, 0.0],
            duration_secs,
            source_entity: None,
            handle: Some(1),
            stopped_at_sim_time: None,
            still_playing: false,
        }
    }

    // Deliberately pure: the ring buffer and the simulation clock are
    // process-global, so the derivation is tested without touching either.
    #[test]
    fn a_clip_plays_until_its_duration_elapses() {
        let sound = entry(10.0, Some(5.0));
        assert_eq!(sound.frame, 600);
        assert!(resolve_still_playing(&sound, 14.9));
        assert!(!resolve_still_playing(&sound, 15.0));
    }

    #[test]
    fn an_unknown_duration_never_reports_playing() {
        assert!(!resolve_still_playing(&entry(10.0, None), 10.1));
    }

    #[test]
    fn a_stop_retires_a_clip_mid_duration() {
        let mut sound = entry(10.0, Some(5.0));
        sound.stopped_at_sim_time = Some(11.0);
        assert!(!resolve_still_playing(&sound, 11.5));
    }

    #[test]
    fn record_stop_marks_the_latest_unstopped_play_of_a_handle() {
        // A handle unique to this test - the buffer is process-global.
        let handle = 9_000_017;
        set_sim_time(3.0);
        for _ in 0..2 {
            record(SoundRecord {
                sample: "test",
                volume_millibels: None,
                gain: 1.0,
                pan_millibels: None,
                pan_applied: false,
                tags: vec![],
                position: [0.0, 0.0, 0.0],
                duration: Some(Duration::from_secs(5)),
                source_entity: None,
                handle: Some(handle),
            });
        }
        record_stop(handle);

        let mine: Vec<_> = recent()
            .into_iter()
            .filter(|entry| entry.handle == Some(handle))
            .collect();
        assert_eq!(mine.len(), 2);
        assert_eq!(mine[0].stopped_at_sim_time, None);
        assert!(mine[1].stopped_at_sim_time.is_some());
    }
}
