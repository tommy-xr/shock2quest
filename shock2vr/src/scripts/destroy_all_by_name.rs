use dark::properties::{PropConsumeType, PropSymName};
use shipyard::{EntityId, Get, IntoIter, IntoWithId, View, World};


use crate::physics::PhysicsWorld;

use super::{
    Effect, MessagePayload, Script,
};

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
                    let mut items_to_consume = Vec::new();

                    let match_string = symbols_to_consume.0.to_ascii_lowercase();

                    world.run(|v_prop_symyname: View<PropSymName>| {
                        for (id, symname) in v_prop_symyname.iter().with_id() {
                            let name_lowercase = symname.0.to_ascii_lowercase();

                            if name_lowercase.contains(&match_string) {
                                items_to_consume.push(id);
                            }
                        }
                    });

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
