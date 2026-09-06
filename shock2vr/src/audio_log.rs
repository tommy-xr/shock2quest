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
use std::rc::Rc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use cgmath::Vector3;
use engine::audio::{
    AudioChannel, AudioClip, AudioContext, AudioHandle, AudioPlaybackSettings,
    play_audio_with_settings, play_spatial_audio_with_gain,
};
use serde::Serialize;
use shipyard::EntityId;

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

/// Complete arguments for inserting one entry into the ring buffer.
struct SoundRecord<'a> {
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

/// Metadata mirrored into the audio log by [`play_and_record`]. Playback-owned
/// fields (gain, position, pan application, clip duration and handle id) are
/// filled from the same arguments used to start the sink, so they cannot drift
/// from the real play.
pub struct PlayRecord<'a> {
    pub sample: &'a str,
    pub volume_millibels: Option<i32>,
    pub pan_millibels: Option<i32>,
    pub tags: Vec<(String, String)>,
    pub source_entity: Option<SourceEntity>,
}

/// The two playback paths supported by [`play_and_record`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PlayOptions {
    ListenerRelative(AudioPlaybackSettings),
    Spatial {
        position: Vector3<f32>,
        source: Option<EntityId>,
        gain: f32,
    },
}

impl PlayOptions {
    fn recorded_position(self) -> [f32; 3] {
        match self {
            Self::ListenerRelative(_) => [0.0, 0.0, 0.0],
            Self::Spatial { position, .. } => [position.x, position.y, position.z],
        }
    }

    fn recorded_gain(self) -> f32 {
        match self {
            Self::ListenerRelative(settings) => settings.gain,
            Self::Spatial { gain, .. } => gain,
        }
    }
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

/// Start one listener-relative or spatial sound and mirror that exact play into
/// the debug audio log. The duration and handle id are captured before their
/// owners move into the engine, and preempted plays are retired before the new
/// record is inserted so reusing a handle cannot stop its replacement.
pub fn play_and_record(
    audio_context: &mut AudioContext<EntityId, String>,
    handle: AudioHandle,
    channel: Option<AudioChannel>,
    clip: Rc<AudioClip>,
    options: PlayOptions,
    record: PlayRecord<'_>,
) -> Vec<u64> {
    play_and_record_with(
        handle,
        channel,
        clip,
        options,
        record,
        |handle, channel, clip, options| match options {
            PlayOptions::ListenerRelative(settings) => {
                play_audio_with_settings(audio_context, handle, channel, clip, settings)
            }
            PlayOptions::Spatial {
                position,
                source,
                gain,
            } => play_spatial_audio_with_gain(
                audio_context,
                position,
                source,
                handle,
                channel,
                clip,
                gain,
            ),
        },
    )
}

fn play_and_record_with<F>(
    handle: AudioHandle,
    channel: Option<AudioChannel>,
    clip: Rc<AudioClip>,
    options: PlayOptions,
    record_args: PlayRecord<'_>,
    play: F,
) -> Vec<u64>
where
    F: FnOnce(AudioHandle, Option<AudioChannel>, Rc<AudioClip>, PlayOptions) -> Vec<u64>,
{
    let duration = clip.total_duration();
    let handle_id = handle.id();
    let position = options.recorded_position();
    let gain = options.recorded_gain();
    let pan_applied =
        record_args.pan_millibels.is_some() && matches!(options, PlayOptions::ListenerRelative(_));
    let preempted = play(handle, channel, clip, options);
    record_stops(&preempted);
    record(SoundRecord {
        sample: record_args.sample,
        volume_millibels: record_args.volume_millibels,
        gain,
        pan_millibels: record_args.pan_millibels,
        pan_applied,
        tags: record_args.tags,
        position,
        duration,
        source_entity: record_args.source_entity,
        handle: Some(handle_id),
    });
    preempted
}

/// `sim_time` expressed in fixed 60 Hz frames. Shared with
/// [`crate::message_trace`], which stamps its entries the same way.
pub(crate) fn frame_of(sim_time: f64) -> u64 {
    (sim_time * FRAMES_PER_SECOND).round().max(0.0) as u64
}

/// Record a successfully resolved + played sound.
fn record(record: SoundRecord) {
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
fn record_stops(handles: &[u64]) {
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

    #[test]
    fn shared_play_retires_a_reused_handle_before_recording_its_replacement() {
        let handle = engine::audio::AudioHandle::new();
        let handle_id = handle.id();
        let clip = std::rc::Rc::new(engine::audio::AudioClip::from_raw(1, 1, vec![0; 5]));

        set_sim_time(20.0);
        play_and_record_with(
            handle.clone(),
            None,
            clip.clone(),
            PlayOptions::ListenerRelative(engine::audio::AudioPlaybackSettings::default()),
            PlayRecord {
                sample: "shared-helper-reused-handle",
                volume_millibels: None,
                pan_millibels: None,
                tags: vec![("kind".to_owned(), "first".to_owned())],
                source_entity: None,
            },
            |_, _, _, _| vec![],
        );

        set_sim_time(21.0);
        let preempted = play_and_record_with(
            handle,
            None,
            clip,
            PlayOptions::ListenerRelative(engine::audio::AudioPlaybackSettings::default()),
            PlayRecord {
                sample: "shared-helper-reused-handle",
                volume_millibels: None,
                pan_millibels: None,
                tags: vec![("kind".to_owned(), "second".to_owned())],
                source_entity: None,
            },
            |_, _, _, _| vec![handle_id],
        );

        let mine: Vec<_> = recent()
            .into_iter()
            .filter(|entry| entry.sample == "shared-helper-reused-handle")
            .collect();
        assert_eq!(preempted, vec![handle_id]);
        assert_eq!(mine.len(), 2);
        assert_eq!(mine[0].duration_secs, Some(5.0));
        assert_eq!(mine[0].stopped_at_sim_time, Some(21.0));
        assert!(!mine[0].still_playing);
        assert_eq!(mine[1].stopped_at_sim_time, None);
        assert!(mine[1].still_playing);
    }

    #[test]
    fn shared_play_preserves_listener_relative_and_spatial_options() {
        let clip = std::rc::Rc::new(engine::audio::AudioClip::from_raw(1, 2, vec![0; 2]));
        let listener_settings = engine::audio::AudioPlaybackSettings {
            gain: 0.25,
            channel_gains: [0.5, 0.75],
            looping: true,
        };

        play_and_record_with(
            engine::audio::AudioHandle::new(),
            None,
            clip.clone(),
            PlayOptions::ListenerRelative(listener_settings),
            PlayRecord {
                sample: "shared-helper-listener-relative",
                volume_millibels: Some(-100),
                pan_millibels: Some(500),
                tags: vec![("kind".to_owned(), "listener".to_owned())],
                source_entity: None,
            },
            |_, channel, _, options| {
                assert!(channel.is_none());
                assert_eq!(options, PlayOptions::ListenerRelative(listener_settings));
                vec![]
            },
        );

        let position = cgmath::vec3(1.0, 2.0, 3.0);
        let source = shipyard::EntityId::new_from_index_and_gen(7, 0);
        play_and_record_with(
            engine::audio::AudioHandle::new(),
            None,
            clip,
            PlayOptions::Spatial {
                position,
                source: Some(source),
                gain: 0.75,
            },
            PlayRecord {
                sample: "shared-helper-spatial",
                volume_millibels: Some(-200),
                pan_millibels: Some(-500),
                tags: vec![("kind".to_owned(), "spatial".to_owned())],
                source_entity: None,
            },
            |_, channel, _, options| {
                assert!(channel.is_none());
                assert_eq!(
                    options,
                    PlayOptions::Spatial {
                        position,
                        source: Some(source),
                        gain: 0.75,
                    }
                );
                vec![]
            },
        );

        let listener = recent()
            .into_iter()
            .find(|entry| entry.sample == "shared-helper-listener-relative")
            .unwrap();
        assert_eq!(listener.position, [0.0, 0.0, 0.0]);
        assert_eq!(listener.gain, 0.25);
        assert!(listener.pan_applied);

        let spatial = recent()
            .into_iter()
            .find(|entry| entry.sample == "shared-helper-spatial")
            .unwrap();
        assert_eq!(spatial.position, [1.0, 2.0, 3.0]);
        assert_eq!(spatial.gain, 0.75);
        assert!(!spatial.pan_applied);
    }
}
