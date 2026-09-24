//! Non-spatial sounds restart instead of stacking: a new play of a schema at
//! the listener replaces the previous one still playing (a replicator's "thank
//! you" on a second vend, two ecologies both announcing a stand-down).
//! Positional sounds keep overlapping - rapid fire and pain must not clip.

use std::collections::HashMap;

use engine::audio::AudioHandle;

#[derive(Default)]
pub struct ListenerSounds {
    latest: HashMap<String, AudioHandle>,
}

impl ListenerSounds {
    /// Record `handle` as the current play of `name`, returning the play it
    /// replaces so the caller can stop it.
    pub fn replace(&mut self, name: &str, handle: AudioHandle) -> Option<AudioHandle> {
        self.latest.insert(name.to_ascii_lowercase(), handle)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_replay_returns_the_play_it_replaces() {
        let mut sounds = ListenerSounds::default();
        let first = AudioHandle::new();
        let second = AudioHandle::new();

        assert!(sounds.replace("replicator2E", first.clone()).is_none());
        assert_eq!(
            sounds.replace("REPLICATOR2e", second).map(|h| h.id()),
            Some(first.id())
        );
        assert!(sounds.replace("xer03", AudioHandle::new()).is_none());
    }
}
