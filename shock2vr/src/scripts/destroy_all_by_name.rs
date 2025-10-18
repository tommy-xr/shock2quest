use dark::properties::PropConsumeType;
use shipyard::{EntityId, Get, View, World};

use crate::physics::PhysicsWorld;

use super::{script_util, Effect, MessagePayload, Script};

pub struct DestroyAllByName {}
impl DestroyAllByName {
    pub fn new() -> DestroyAllByName {
        DestroyAllByName {}
    }
}
impl Script for DestroyAllByName {
    fn handle_message(
        &mut self,
        entity_id: EntityId,
        world: &World,
        _physics: &PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        match msg {
            MessagePayload::TurnOn { from: _ } => {
                let v_prop_consume = world.borrow::<View<PropConsumeType>>().unwrap();

                if let Ok(symbols_to_consume) = v_prop_consume.get(entity_id) {
                    let match_string = symbols_to_consume.0.to_ascii_lowercase();

                    let items_to_consume = script_util::get_entities_by_name(world, &match_string);

                    let effs: Vec<Effect> = items_to_consume
                        .iter()
                        .map(|e| Effect::DestroyEntity { entity_id: *e })
                        .collect();

                    Effect::Combined { effects: effs }
                } else {
                    Effect::NoEffect
                }
            }
            _ => Effect::NoEffect,
        }
    }
}
