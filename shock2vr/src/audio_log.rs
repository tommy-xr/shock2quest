//! Ring buffer of recently played environmental sounds, so headless tooling
//! (the debug runtime's `GET /v1/audio/recent`) can verify that a sound schema
//! was actually resolved and played - there is no other way to observe audio
//! without speakers.

use std::collections::VecDeque;
use std::sync::Mutex;

use serde::Serialize;

const MAX_ENTRIES: usize = 64;

#[derive(Clone, Debug, Serialize)]
pub struct PlayedEnvSound {
    /// Monotonically increasing id, so callers can diff "new since last poll".
    pub sequence: u64,
    /// Resolved sample name (e.g. "bulmet2"), without extension.
    pub sample: String,
    /// The schema query's (tag, value) pairs (e.g. event=collision, material=metal).
    pub tags: Vec<(String, String)>,
    pub position: [f32; 3],
}

static RECENT: Mutex<(u64, VecDeque<PlayedEnvSound>)> = Mutex::new((0, VecDeque::new()));

/// Record a successfully resolved + played environmental sound.
pub fn record(sample: &str, tags: Vec<(String, String)>, position: [f32; 3]) {
    let mut guard = RECENT.lock().unwrap();
    let (next_sequence, entries) = &mut *guard;
    *next_sequence += 1;
    entries.push_back(PlayedEnvSound {
        sequence: *next_sequence,
        sample: sample.to_owned(),
        tags,
        position,
    });
    if entries.len() > MAX_ENTRIES {
        entries.pop_front();
    }
}

/// The most recent played sounds, oldest first.
pub fn recent() -> Vec<PlayedEnvSound> {
    RECENT.lock().unwrap().1.iter().cloned().collect()
}
