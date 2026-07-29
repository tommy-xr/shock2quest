use dark::properties::{AIAlertLevel, PropAIAlertness};
use shipyard::{EntityId, Get, UniqueView, View, World};

use crate::{mission::PlayerInfo, physics::PhysicsWorld, time::Time};

use super::{Effect, Message, MessagePayload, Script, script_util::send_to_all_switch_links};

/// Retail `CameraAlert`: raise the linked security ecology once when the
/// camera reaches level-three alertness, then stay latched until `Reset`.
pub struct CameraAlert {
    alarm_raised: bool,
}

impl CameraAlert {
    pub fn new() -> Self {
        Self {
            alarm_raised: false,
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
    fn initialize(&mut self, entity_id: EntityId, world: &World) -> Effect {
        // A save restored while the camera is red must not synthesize a second
        // Alarm on its first frame; the linked ecology owns and restores the
        // active recovery window.
        self.alarm_raised = Self::is_high_alert(world, entity_id);
        Effect::NoEffect
    }

    fn update(
        &mut self,
        entity_id: EntityId,
        world: &World,
        _physics: &PhysicsWorld,
        _time: &Time,
    ) -> Effect {
        if self.alarm_raised || !Self::is_high_alert(world, entity_id) {
            return Effect::NoEffect;
        }
        let Ok(player) = world.borrow::<UniqueView<PlayerInfo>>() else {
            return Effect::NoEffect;
        };
        let victim = player.entity_id;
        drop(player);
        self.alarm_raised = true;
        send_to_all_switch_links(
            world,
            entity_id,
            MessagePayload::Alarm {
                from: entity_id,
                victim,
            },
        )
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

        self.alarm_raised = false;
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
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use cgmath::{Quaternion, vec3};
    use dark::properties::{Link, Links, ToLink, WrappedEntityId};

    use super::*;

    fn add_player(world: &mut World) -> EntityId {
        let player = world.add_entity(());
        let inventory = world.add_entity(());
        world.add_unique(PlayerInfo {
            pos: vec3(0.0, 0.0, 0.0),
            rotation: Quaternion::new(1.0, 0.0, 0.0, 0.0),
            entity_id: player,
            left_hand_entity_id: None,
            right_hand_entity_id: None,
            inventory_entity_id: inventory,
        });
        player
    }

    #[test]
    fn high_alert_raises_one_authored_alarm_until_reset() {
        let mut world = World::new();
        let player = add_player(&mut world);
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
        let mut script = CameraAlert::new();
        let time = Time {
            elapsed: Duration::from_secs(1),
            total: Duration::from_secs(1),
        };

        let first = script.update(camera, &world, &PhysicsWorld::new(), &time);
        assert!(Effect::flatten(vec![first]).into_iter().any(|effect| {
            matches!(
                effect,
                Effect::Send { msg }
                    if msg.to == ecology
                        && matches!(
                            msg.payload,
                            MessagePayload::Alarm { from, victim }
                                if from == camera && victim == player
                        )
            )
        }));
        assert!(matches!(
            script.update(camera, &world, &PhysicsWorld::new(), &time),
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
    fn restored_high_camera_does_not_duplicate_an_active_alarm() {
        let mut world = World::new();
        add_player(&mut world);
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
        let mut script = CameraAlert::new();

        script.initialize(camera, &world);

        assert!(matches!(
            script.update(
                camera,
                &world,
                &PhysicsWorld::new(),
                &Time {
                    elapsed: Duration::from_secs(1),
                    total: Duration::from_secs(1),
                },
            ),
            Effect::NoEffect
        ));
    }
}
