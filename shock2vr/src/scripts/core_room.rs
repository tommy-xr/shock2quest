use dark::properties::{Gravity, PropRoomGravity};
use shipyard::{EntityId, Get, View, World};

use crate::physics::PhysicsWorld;

use super::{Effect, MessagePayload, Script};

pub struct CoreRoom {
    gravity_adjustment: Option<PropRoomGravity>,
}
impl CoreRoom {
    pub fn new() -> CoreRoom {
        CoreRoom {
            gravity_adjustment: None,
        }
    }
}

impl Script for CoreRoom {
    fn initialize(&mut self, entity_id: EntityId, world: &World) -> Effect {
        let v_prop_grav = world.borrow::<View<PropRoomGravity>>().unwrap();
        self.gravity_adjustment = v_prop_grav.get(entity_id).ok().cloned();

        Effect::NoEffect
    }
    fn handle_message(
        &mut self,
        _entity_id: EntityId,
        _world: &World,
        _physics: &PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        if self.gravity_adjustment.is_some() {
            match msg {
                MessagePayload::SensorBeginIntersect { with } => {
                    let grav = self.gravity_adjustment.clone().unwrap();

                    match grav.0 {
                        Gravity::Reset => Effect::ResetGravity { entity_id: *with },
                        Gravity::Set(pct) => Effect::SetGravity {
                            entity_id: *with,
                            gravity_percent: pct,
                        },
                    }
                }
                MessagePayload::SensorEndIntersect { with } => {
                    Effect::ResetGravity { entity_id: *with }
                }
                _ => Effect::NoEffect,
            }
        } else {
            Effect::NoEffect
        }
    }
}
