use dark::properties::{Gravity, PropRoomGravity};
use shipyard::{EntityId, Get, View, World};

use crate::physics::PhysicsWorld;

use super::{
    Effect, MessagePayload, Script, script_util::send_to_all_switch_links,
    trap_new_tripwire::TrapNewTripwire,
};

pub struct RoomTrigger {
    trap_tripwire: TrapNewTripwire,
    gravity: Option<f32>,
}

impl RoomTrigger {
    pub fn new() -> RoomTrigger {
        RoomTrigger {
            trap_tripwire: TrapNewTripwire::new(),
            gravity: None,
        }
    }
}

impl Script for RoomTrigger {
    fn initialize(&mut self, entity_id: EntityId, world: &World) -> Effect {
        self.trap_tripwire.initialize(entity_id, world);
        let gravity = world.borrow::<View<PropRoomGravity>>().unwrap();
        self.gravity = super::script_util::get_all_switch_links(world, entity_id)
            .into_iter()
            .find_map(|room| gravity.get(room).ok())
            .map(|gravity| match gravity.0 {
                Gravity::Reset => 1.0,
                Gravity::Set(percent) => percent,
            });
        Effect::NoEffect
    }

    fn handle_message(
        &mut self,
        entity_id: EntityId,
        world: &World,
        _physics: &PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        let tripwire_effect = self
            .trap_tripwire
            .handle_message(entity_id, world, _physics, msg);

        let trigger_effect = {
            match msg {
                // Forward begin / end intersect messages
                MessagePayload::SensorBeginIntersect { with: _ } => {
                    send_to_all_switch_links(world, entity_id, msg.clone())
                }
                MessagePayload::SensorEndIntersect { with: _ } => {
                    send_to_all_switch_links(world, entity_id, msg.clone())
                }
                _ => Effect::NoEffect,
            }
        };

        // Preserve the unique sensor as the owner. CoreRoom receives only
        // anonymous forwarded intersections and cannot distinguish adjacent
        // volumes which share one room archetype.
        let gravity_effect = match (self.gravity, msg) {
            (Some(gravity), MessagePayload::SensorBeginIntersect { with }) => {
                Effect::SetRoomGravity {
                    entity_id: *with,
                    sensor_id: entity_id,
                    gravity_percent: Some(gravity),
                }
            }
            (Some(_), MessagePayload::SensorEndIntersect { with }) => Effect::SetRoomGravity {
                entity_id: *with,
                sensor_id: entity_id,
                gravity_percent: None,
            },
            _ => Effect::NoEffect,
        };
        Effect::Combined {
            effects: vec![tripwire_effect, trigger_effect, gravity_effect],
        }
    }
}

#[cfg(test)]
mod tests {
    use dark::properties::{Link, Links, PropTripFlags, ToLink, TripFlags, WrappedEntityId};

    use super::*;

    #[test]
    fn room_gravity_effects_keep_unique_sensor_ownership_after_reinitialization() {
        let mut world = World::new();
        let room = world.add_entity((PropRoomGravity(Gravity::Set(0.01)),));
        let actor = world.add_entity(());
        let mut sensors = Vec::new();
        for _ in 0..2 {
            sensors.push(world.add_entity((
                PropTripFlags {
                    trip_flags: TripFlags::empty(),
                },
                Links {
                    to_links: vec![ToLink {
                        to_template_id: 251,
                        to_entity_id: Some(WrappedEntityId(room)),
                        link: Link::SwitchLink,
                    }],
                },
            )));
        }
        let physics = PhysicsWorld::new();
        // Runtime room sensors are recreated on load; no saved script latch
        // may be needed before they emit their first gravity contribution.
        for sensor in sensors {
            for _ in 0..2 {
                let mut trigger = RoomTrigger::new();
                trigger.initialize(sensor, &world);
                for (msg, expected) in [
                    (
                        MessagePayload::SensorBeginIntersect { with: actor },
                        Some(0.01),
                    ),
                    (MessagePayload::SensorEndIntersect { with: actor }, None),
                ] {
                    let effects = Effect::flatten(vec![
                        trigger.handle_message(sensor, &world, &physics, &msg),
                    ]);
                    let contributions = effects
                        .iter()
                        .filter_map(|effect| match effect {
                            Effect::SetRoomGravity {
                                entity_id,
                                sensor_id,
                                gravity_percent,
                            } => Some((*entity_id, *sensor_id, *gravity_percent)),
                            _ => None,
                        })
                        .collect::<Vec<_>>();
                    assert_eq!(contributions, vec![(actor, sensor, expected)]);
                }
            }
        }
    }
}
