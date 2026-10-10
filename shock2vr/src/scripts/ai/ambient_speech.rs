//! Creatures request alert-state broadcasts; SchLoopPa supplies
//! their cadence. The effect applier feeds back the chosen clip's timing so
//! the script can save its countdown without owning audio or mutating the world.
use cgmath::{EuclideanSpace, MetricSpace};
use dark::{SCALE_FACTOR, properties::AIAlertLevel};
use rand::Rng;
use serde::{Deserialize, Serialize};
use shipyard::{EntityId, UniqueView, World};

use crate::{
    mission::PlayerInfo,
    scripts::{Effect, speech_util},
};

#[derive(Clone, Serialize, Deserialize)]
pub(super) struct AmbientSpeech {
    remaining: f32,
    level: Option<AIAlertLevel>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use dark::properties::{PropAIBroadcastDisabled, PropPosition, PropVoiceIndex};

    fn creature() -> (World, EntityId, AmbientSpeech) {
        let mut world = World::new();
        let entity = world.add_entity((
            PropVoiceIndex(12),
            crate::runtime_props::RuntimePropTransform(cgmath::Matrix4::from_scale(1.0)),
            PropPosition {
                position: cgmath::vec3(0.0, 0.0, 0.0),
                rotation: cgmath::Quaternion::new(1.0, 0.0, 0.0, 0.0),
                cell: 0,
            },
        ));
        (
            world,
            entity,
            AmbientSpeech {
                remaining: 0.0,
                level: None,
            },
        )
    }

    fn assert_concept(effect: Effect, expected: &str) {
        assert!(
            matches!(effect, Effect::PlaySpeech { voice_index: 12, concept, .. }
            if concept == expected)
        );
    }

    #[test]
    fn repeats_use_playback_feedback_and_switch_to_the_current_alert_level() {
        let (world, entity, mut speech) = creature();
        assert_concept(
            speech.update(&world, entity, AIAlertLevel::Lowest, 0.1),
            "atlevelzero",
        );
        speech.played("atlevelzero", 12.0);
        assert!(matches!(
            speech.update(&world, entity, AIAlertLevel::Lowest, 11.0),
            Effect::NoEffect
        ));
        assert_concept(
            speech.update(&world, entity, AIAlertLevel::Lowest, 1.0),
            "atlevelzero",
        );
        speech.played("atlevelzero", 12.0);
        assert_concept(
            speech.update(&world, entity, AIAlertLevel::Low, 0.1),
            "atlevelone",
        );
        assert_concept(
            speech.update(&world, entity, AIAlertLevel::Moderate, 0.1),
            "atleveltwo",
        );
        assert_concept(
            speech.update(&world, entity, AIAlertLevel::High, 0.1),
            "atlevelthree",
        );
        speech.played("comattack", 4.0);
        assert!(matches!(
            speech.update(&world, entity, AIAlertLevel::High, 3.0),
            Effect::NoEffect
        ));
        assert_concept(
            speech.update(&world, entity, AIAlertLevel::High, 1.0),
            "atlevelthree",
        );
    }

    #[test]
    fn missing_samples_are_retried_at_most_once_per_two_seconds() {
        let (world, entity, mut speech) = creature();
        assert_concept(
            speech.update(&world, entity, AIAlertLevel::Lowest, 0.1),
            "atlevelzero",
        );
        assert!(matches!(
            speech.update(&world, entity, AIAlertLevel::Lowest, 1.0),
            Effect::NoEffect
        ));
        assert_concept(
            speech.update(&world, entity, AIAlertLevel::Lowest, 1.0),
            "atlevelzero",
        );
    }

    #[test]
    fn authored_mute_and_missing_voice_are_silent() {
        let (mut world, entity, mut speech) = creature();
        world.add_component(entity, (PropAIBroadcastDisabled(true),));
        assert!(matches!(
            speech.update(&world, entity, AIAlertLevel::Lowest, 3.0),
            Effect::NoEffect
        ));
        world.add_component(entity, (PropAIBroadcastDisabled(false), PropVoiceIndex(-1)));
        assert!(matches!(
            speech.update(&world, entity, AIAlertLevel::Lowest, 3.0),
            Effect::NoEffect
        ));
    }

    #[test]
    fn countdown_round_trips_without_replaying_immediately() {
        let (world, entity, mut speech) = creature();
        speech.update(&world, entity, AIAlertLevel::Lowest, 0.1);
        speech.played("atlevelzero", 15.0);
        let json = serde_json::to_string(&speech).unwrap();
        let mut restored: AmbientSpeech = serde_json::from_str(&json).unwrap();
        assert!(matches!(
            restored.update(&world, entity, AIAlertLevel::Lowest, 14.0),
            Effect::NoEffect
        ));
        assert_concept(
            restored.update(&world, entity, AIAlertLevel::Lowest, 1.0),
            "atlevelzero",
        );
    }
}

impl Default for AmbientSpeech {
    fn default() -> Self {
        Self {
            // Stagger a room's first calls within retail's two-second poll.
            remaining: rand::thread_rng().gen_range(0.0..2.0),
            level: None,
        }
    }
}

impl AmbientSpeech {
    pub fn played(&mut self, concept: &str, seconds: f32) {
        if seconds.is_finite()
            && seconds >= 0.0
            && (!concept.starts_with("atlevel")
                || self
                    .level
                    .is_some_and(|level| concept == Self::concept(level)))
        {
            self.remaining = seconds;
        }
    }

    fn concept(level: AIAlertLevel) -> &'static str {
        match level {
            AIAlertLevel::Lowest => "atlevelzero",
            AIAlertLevel::Low => "atlevelone",
            AIAlertLevel::Moderate => "atleveltwo",
            AIAlertLevel::High => "atlevelthree",
        }
    }

    pub fn update(
        &mut self,
        world: &World,
        entity: EntityId,
        level: AIAlertLevel,
        dt: f32,
    ) -> Effect {
        if self.level.is_some_and(|previous| previous != level) {
            self.remaining = 0.0;
        }
        self.level = Some(level);
        self.remaining -= dt;
        if self.remaining > 0.0 {
            return Effect::NoEffect;
        }
        // Also bounds retries for absent concepts/voices or priority rejection.
        self.remaining = 2.0;
        if !speech_util::broadcasts_enabled(world, entity) {
            return Effect::NoEffect;
        }
        let position = super::ai_util::get_position_and_forward(world, entity)
            .0
            .to_vec();
        // Hearing range is 100 Dark feet. Distant creatures do not
        // allocate inaudible voices or resume with a backlog on approach.
        if world
            .borrow::<UniqueView<PlayerInfo>>()
            .is_ok_and(|player| player.pos.distance2(position) >= (100.0 / SCALE_FACTOR).powi(2))
        {
            return Effect::NoEffect;
        }
        match speech_util::resolve_entity_voice_index(world, entity) {
            Some(voice_index) => Effect::PlaySpeech {
                entity_id: entity,
                voice_index,
                concept: Self::concept(level).into(),
                tags: vec![],
            },
            None => Effect::NoEffect,
        }
    }
}
