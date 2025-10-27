use dark::properties::{PropDestLevel, PropDestLoc};

use shipyard::{EntityId, Get, View, World};

use crate::physics::PhysicsWorld;

use super::{Effect, MessagePayload, Script};

pub struct TrapTripLevel {}
impl TrapTripLevel {
    pub fn new() -> TrapTripLevel {
        TrapTripLevel {}
    }
}

impl Script for TrapTripLevel {
    fn handle_message(
        &mut self,
        entity_id: EntityId,
        world: &World,
        _physics: &PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        match msg {
            MessagePayload::SensorBeginIntersect { with: _ } => {
                let v_dest_level = world.borrow::<View<PropDestLevel>>().unwrap();
                let level_file = v_dest_level.get(entity_id).unwrap();

                let v_dest_loc = world.borrow::<View<PropDestLoc>>().unwrap();
                let maybe_dest_loc = v_dest_loc.get(entity_id).ok().map(|dest_loc| dest_loc.0);
                Effect::GlobalEffect(super::GlobalEffect::TransitionLevel {
                    level_file: format!("{}.mis", level_file.0),
                    loc: maybe_dest_loc,
                    entities_to_trigger: vec![],
                })
            }
            _ => Effect::NoEffect,
        }
    }
}
