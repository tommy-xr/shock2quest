use dark::properties::{AIAlertLevel, PropAIAlertness};
use serde::{Deserialize, Serialize};
use shipyard::{EntityId, Get, View, World};

use crate::{physics::PhysicsWorld, time::Time};

use super::{
    Effect, Message, MessagePayload, Script, ScriptRestoreContext, ScriptState, ScriptStateError,
    script_util::send_to_all_switch_links,
};

const SCRIPT_STATE_KEY: &str = "shock2vr.camera_alert";

/// Alarm lifecycle for one security camera.
///
/// `Resetting` exists because a `Reset` only *queues* the camera's alertness
/// reset: on the frame the message arrives the camera still reads as
/// level-three, and re-arming immediately would raise a fresh alarm before
/// the reset lands. The latch re-arms only once the camera is observed below
/// level three.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
enum AlarmPhase {
    Armed,
    Latched,
    Resetting,
}

#[derive(Serialize, Deserialize)]
struct CameraAlertState {
    phase: AlarmPhase,
}

/// Retail `CameraAlert`: raise the linked security ecology once when the
/// camera reaches level-three alertness, then stay latched until `Reset`.
pub struct CameraAlert {
    phase: AlarmPhase,
}

impl CameraAlert {
    pub fn new() -> Self {
        Self {
            phase: AlarmPhase::Armed,
        }
    }

    fn is_high_alert(world: &World, entity_id: EntityId) -> bool {
        world
            .borrow::<View<PropAIAlertness>>()
            .ok()
            .and_then(|alertness| {
                alertness
                    .get(entity_id)
                    .ok()
                    .map(|alertness| alertness.level == AIAlertLevel::High)
            })
            .unwrap_or(false)
    }
}

impl Script for CameraAlert {
    fn update(
        &mut self,
        entity_id: EntityId,
        world: &World,
        _physics: &PhysicsWorld,
        _time: &Time,
    ) -> Effect {
        let high_alert = Self::is_high_alert(world, entity_id);
        match self.phase {
            AlarmPhase::Armed if high_alert => {
                self.phase = AlarmPhase::Latched;
                send_to_all_switch_links(
                    world,
                    entity_id,
                    MessagePayload::Alarm { from: entity_id },
                )
            }
            AlarmPhase::Resetting if !high_alert => {
                self.phase = AlarmPhase::Armed;
                Effect::NoEffect
            }
            _ => Effect::NoEffect,
        }
    }

    fn handle_message(
        &mut self,
        entity_id: EntityId,
        _world: &World,
        _physics: &PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        if !matches!(msg, MessagePayload::Reset { .. }) {
            return Effect::NoEffect;
        }

        self.phase = AlarmPhase::Resetting;
        Effect::Send {
            msg: Message {
                to: entity_id,
                payload: MessagePayload::SetAlertness {
                    level: AIAlertLevel::Lowest,
                    pin: false,
                },
            },
        }
    }

    fn script_state_key(&self) -> Option<&'static str> {
        Some(SCRIPT_STATE_KEY)
    }

    fn save_state(&self) -> Result<ScriptState, ScriptStateError> {
        ScriptState::encode(1, &CameraAlertState { phase: self.phase }, SCRIPT_STATE_KEY)
    }

    fn restore_state(
        &mut self,
        state: &ScriptState,
        _context: &ScriptRestoreContext<'_>,
    ) -> Result<(), ScriptStateError> {
        let restored: CameraAlertState = state.decode(1, SCRIPT_STATE_KEY)?;
        self.phase = restored.phase;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, time::Duration};

    use dark::properties::{Link, Links, ToLink, WrappedEntityId};
    use shipyard::ViewMut;

    use super::*;

    fn camera_world() -> (World, EntityId, EntityId) {
        let mut world = World::new();
        let ecology = world.add_entity(());
        let camera = world.add_entity((
            PropAIAlertness {
                level: AIAlertLevel::High,
                peak: AIAlertLevel::High,
            },
            Links {
                to_links: vec![ToLink {
                    to_template_id: 71,
                    to_entity_id: Some(WrappedEntityId(ecology)),
                    link: Link::SwitchLink,
                }],
            },
        ));
        (world, camera, ecology)
    }

    fn set_alertness(world: &World, camera: EntityId, level: AIAlertLevel) {
        let mut alertness = world.borrow::<ViewMut<PropAIAlertness>>().unwrap();
        (&mut alertness).get(camera).unwrap().level = level;
    }

    fn time() -> Time {
        Time {
            elapsed: Duration::from_secs(1),
            total: Duration::from_secs(1),
        }
    }

    fn raises_alarm(effect: Effect, camera: EntityId, ecology: EntityId) -> bool {
        Effect::flatten(vec![effect]).into_iter().any(|effect| {
            matches!(
                effect,
                Effect::Send { msg }
                    if msg.to == ecology
                        && matches!(msg.payload, MessagePayload::Alarm { from } if from == camera)
            )
        })
    }

    #[test]
    fn high_alert_raises_one_authored_alarm_until_reset() {
        let (world, camera, ecology) = camera_world();
        let mut script = CameraAlert::new();

        let first = script.update(camera, &world, &PhysicsWorld::new(), &time());
        assert!(raises_alarm(first, camera, ecology));
        assert!(matches!(
            script.update(camera, &world, &PhysicsWorld::new(), &time()),
            Effect::NoEffect
        ));

        let reset = script.handle_message(
            camera,
            &world,
            &PhysicsWorld::new(),
            &MessagePayload::Reset { from: ecology },
        );
        assert!(matches!(
            reset,
            Effect::Send { msg }
                if msg.to == camera
                    && matches!(
                        msg.payload,
                        MessagePayload::SetAlertness {
                            level: AIAlertLevel::Lowest,
                            pin: false
                        }
                    )
        ));
    }

    #[test]
    fn reset_does_not_rearm_until_the_camera_leaves_high_alert() {
        let (world, camera, ecology) = camera_world();
        let mut script = CameraAlert::new();
        script.update(camera, &world, &PhysicsWorld::new(), &time());

        script.handle_message(
            camera,
            &world,
            &PhysicsWorld::new(),
            &MessagePayload::Reset { from: ecology },
        );

        // The queued SetAlertness has not landed yet: the camera still reads
        // level three, and the latch must not raise a fresh alarm off it.
        assert!(matches!(
            script.update(camera, &world, &PhysicsWorld::new(), &time()),
            Effect::NoEffect
        ));

        set_alertness(&world, camera, AIAlertLevel::Lowest);
        assert!(matches!(
            script.update(camera, &world, &PhysicsWorld::new(), &time()),
            Effect::NoEffect
        ));

        // Re-armed: a fresh escalation raises a fresh alarm.
        set_alertness(&world, camera, AIAlertLevel::High);
        let effect = script.update(camera, &world, &PhysicsWorld::new(), &time());
        assert!(raises_alarm(effect, camera, ecology));
    }

    #[test]
    fn saved_phase_round_trips_exactly() {
        let (world, camera, ecology) = camera_world();
        let mut before_save = CameraAlert::new();
        before_save.update(camera, &world, &PhysicsWorld::new(), &time());
        assert_eq!(before_save.phase, AlarmPhase::Latched);

        let state = before_save.save_state().unwrap();
        let mut after_load = CameraAlert::new();
        after_load
            .restore_state(&state, &ScriptRestoreContext::new(&HashMap::new()))
            .unwrap();

        // A restored latched camera must not synthesize a second alarm.
        assert!(matches!(
            after_load.update(camera, &world, &PhysicsWorld::new(), &time()),
            Effect::NoEffect
        ));
        let _ = ecology;
    }
}
