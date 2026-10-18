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
    started: Duration,
    until: Duration,
    /// Authored mouth positions for the clip (`crate::lip_sync`), if any.
    mouth: Option<Vec<u8>>,
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
        mouth: Option<Vec<u8>>,
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
                    started: now,
                    until: now + duration,
                    mouth,
                },
            )
            .map(|previous| previous.handle)
    }

    /// The speaker's mouth shapes while its clip plays. A replacing clip
    /// restarts from its own sequence; a finished one leaves the face neutral.
    pub fn mouth_weights(
        &self,
        entity: EntityId,
        now: Duration,
    ) -> Option<Vec<(&'static str, f32)>> {
        let active = self.active.get(&entity).filter(|a| now < a.until)?;
        let elapsed = now.saturating_sub(active.started).as_secs_f32() * 1000.0;
        crate::lip_sync::mouth_weights(active.mouth.as_deref()?, elapsed)
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
            None,
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
                None,
            )
            .unwrap();
        assert_eq!(stopped.id(), handle.id());
    }

    #[test]
    fn mouth_follows_the_playing_clip_and_resets_when_it_ends() {
        let mut speech = SpeechPlayback::default();
        let mut world = shipyard::World::new();
        let creature = world.add_entity(());
        let other = world.add_entity(());
        let start = Duration::from_secs(10);
        speech.started(
            creature,
            1,
            start,
            Duration::from_secs(1),
            AudioHandle::new(),
            Some(vec![2; 16]),
        );
        speech.started(
            other,
            1,
            start,
            Duration::from_secs(1),
            AudioHandle::new(),
            None,
        );
        assert_eq!(
            speech.mouth_weights(creature, start),
            Some(vec![("MouthShapeB", 1.0)])
        );
        assert_eq!(
            speech.mouth_weights(other, start),
            None,
            "no Face, no sequence"
        );
        // An interruption restarts on the new clip's sequence.
        let later = start + Duration::from_millis(500);
        speech.started(
            creature,
            2,
            later,
            Duration::from_secs(1),
            AudioHandle::new(),
            Some(vec![5; 16]),
        );
        assert_eq!(
            speech.mouth_weights(creature, later),
            Some(vec![("MouthShapeD", 1.0)])
        );
        // Repeat playback restarts the same sequence from its beginning.
        let again = later + Duration::from_millis(250);
        speech.started(
            creature,
            3,
            again,
            Duration::from_secs(1),
            AudioHandle::new(),
            Some(vec![5; 16]),
        );
        assert_eq!(
            speech.mouth_weights(creature, again),
            Some(vec![("MouthShapeD", 1.0)])
        );
        assert_eq!(
            speech.mouth_weights(creature, again + Duration::from_secs(1)),
            None
        );
    }
}
