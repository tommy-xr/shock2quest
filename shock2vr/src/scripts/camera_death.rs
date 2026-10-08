use dark::properties::{AIMode, ObjectState, PropHitPoints, PropPosition};
use engine::audio::AudioHandle;
use serde::{Deserialize, Serialize};
use shipyard::{EntityId, Get, View, World};

use super::{
    Effect, MessagePayload, Script, ScriptRestoreContext, ScriptState, ScriptStateError,
    effect::AIPropertyUpdate, script_util,
};
use crate::physics::PhysicsWorld;

const STATE_KEY: &str = "shock2vr.camera_death";

/// CameraDeath retains the housing, switches to the authored damaged model,
/// and creates the retail harmless explosion once. Also authored on Rick Turret.
#[derive(Default, Serialize, Deserialize)]
pub struct CameraDeath {
    slain: bool,
}

impl CameraDeath {
    pub fn new() -> Self {
        Self::default()
    }
}

impl Script for CameraDeath {
    fn handle_message(
        &mut self,
        entity_id: EntityId,
        world: &World,
        _: &PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        if !matches!(msg, MessagePayload::Slay) || self.slain {
            return Effect::NoEffect;
        }
        self.slain = true;
        let hp = world
            .borrow::<View<PropHitPoints>>()
            .unwrap()
            .get(entity_id)
            .map_or(0, |hp| hp.hit_points.max(0));
        let mut effects = vec![
            script_util::change_to_last_model(world, entity_id),
            Effect::AdjustHitPoints {
                entity_id,
                delta: -hp,
            },
            Effect::SetObjectState {
                entity_id,
                state: ObjectState::Destroyed,
            },
            Effect::SetAIProperty {
                entity_id,
                update: AIPropertyUpdate::Mode { mode: AIMode::Dead },
            },
            script_util::play_environmental_sound(
                world,
                entity_id,
                "death",
                vec![],
                AudioHandle::new(),
            ),
        ];
        if let Ok(pose) = world.borrow::<View<PropPosition>>().unwrap().get(entity_id) {
            effects.push(Effect::CreateEntityByTemplateName {
                source_entity_id: entity_id,
                template_name: "HE_harmless".into(),
                position: cgmath::point3(pose.position.x, pose.position.y, pose.position.z),
                orientation: pose.rotation,
                initial_velocity: cgmath::vec3(0.0, 0.0, 0.0),
                options: Default::default(),
            });
        }
        Effect::combine(effects)
    }

    fn script_state_key(&self) -> Option<&'static str> {
        Some(STATE_KEY)
    }
    fn save_state(&self) -> Result<ScriptState, ScriptStateError> {
        ScriptState::encode(1, self, STATE_KEY)
    }
    fn restore_state(
        &mut self,
        state: &ScriptState,
        _: &ScriptRestoreContext<'_>,
    ) -> Result<(), ScriptStateError> {
        *self = state.decode(1, STATE_KEY)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dark::properties::{PropScripts, PropTweqModelConfig, TweqAnimationConfig, TweqHalt};

    #[test]
    fn death_retains_authored_shell_and_does_not_repeat_after_save() {
        let mut world = World::new();
        let entity = world.add_entity((
            PropHitPoints { hit_points: 5 },
            PropScripts {
                scripts: vec!["CameraDeath".into()],
                inherits: false,
            },
            PropTweqModelConfig {
                animation_config: TweqAnimationConfig::SIM,
                halt: TweqHalt::StopTweq,
                model_names: vec!["camdam".into()],
            },
        ));
        assert!(script_util::retain_on_slay(&world, entity));
        let ordinary = world.add_entity((PropHitPoints { hit_points: 5 },));
        assert!(!script_util::retain_on_slay(&world, ordinary));
        let physics = PhysicsWorld::new();
        let mut script = CameraDeath::new();
        let effects = Effect::flatten(vec![script.handle_message(
            entity,
            &world,
            &physics,
            &MessagePayload::Slay,
        )]);
        assert!(effects.iter().any(
            |e| matches!(e, Effect::ChangeModel { model_name, .. } if model_name == "camdam")
        ));
        assert!(effects.iter().any(|e| matches!(
            e,
            Effect::SetAIProperty {
                update: AIPropertyUpdate::Mode { mode: AIMode::Dead },
                ..
            }
        )));
        assert!(
            effects
                .iter()
                .any(|e| matches!(e, Effect::AdjustHitPoints { delta: -5, .. }))
        );
        assert!(
            !effects
                .iter()
                .any(|e| matches!(e, Effect::DestroyEntity { .. } | Effect::SlayEntity { .. }))
        );
        let saved = script.save_state().unwrap();
        let mut loaded = CameraDeath::new();
        loaded
            .restore_state(&saved, &ScriptRestoreContext::new(&Default::default()))
            .unwrap();
        assert!(matches!(
            loaded.handle_message(entity, &world, &physics, &MessagePayload::Slay),
            Effect::NoEffect
        ));
    }
}
