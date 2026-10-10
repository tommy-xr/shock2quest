//! One spoken clip per creature, arbitrated by the retail concept priorities.
//! Playback handles are transient; the owning AI saves its repeat countdown.
use engine::audio::AudioHandle;
use shipyard::EntityId;
use std::{collections::HashMap, time::Duration};

#[derive(Default)]
pub(crate) struct SpeechPlayback {
    active: HashMap<EntityId, ActiveSpeech>,
}

struct ActiveSpeech {
    handle: AudioHandle,
    priority: u32,
    until: Duration,
}

impl SpeechPlayback {
    pub fn allows(&self, entity: EntityId, priority: u32, now: Duration) -> bool {
        self.active
            .get(&entity)
            .is_none_or(|active| now >= active.until || priority > active.priority)
    }

    pub fn started(
        &mut self,
        entity: EntityId,
        priority: u32,
        now: Duration,
        duration: Duration,
        handle: AudioHandle,
    ) -> Option<AudioHandle> {
        // Retire finished speakers (including actors removed from the world),
        // retaining this entity's handle so replacement can stop its old sink.
        self.active
            .retain(|id, active| *id == entity || now < active.until);
        self.active
            .insert(
                entity,
                ActiveSpeech {
                    handle,
                    priority,
                    until: now + duration,
                },
            )
            .map(|previous| previous.handle)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn speech_priority_prevents_overlap_and_allows_interruptions() {
        let mut speech = SpeechPlayback::default();
        let mut world = shipyard::World::new();
        let creature = world.add_entity(());
        let other = world.add_entity(());
        let handle = AudioHandle::new();
        speech.started(
            creature,
            2,
            Duration::ZERO,
            Duration::from_secs(3),
            handle.clone(),
        );
        assert!(!speech.allows(creature, 1, Duration::from_secs(1)));
        assert!(!speech.allows(creature, 2, Duration::from_secs(1)));
        assert!(speech.allows(creature, 3, Duration::from_secs(1)));
        assert!(speech.allows(other, 1, Duration::from_secs(1)));
        assert!(speech.allows(creature, 1, Duration::from_secs(3)));
        let stopped = speech
            .started(
                creature,
                3,
                Duration::from_secs(1),
                Duration::from_secs(2),
                AudioHandle::new(),
            )
            .unwrap();
        assert_eq!(stopped.id(), handle.id());
    }
}
