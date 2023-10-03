use dark::properties::{PropLimbModel, PropPlayerGun};
use shipyard::{EntityId, Get, View, World};

use crate::physics::PhysicsWorld;

use super::{Effect, Message, MessagePayload, Script};

// Script to handle collision type
pub struct MeleeWeapon {}

impl MeleeWeapon {
    pub fn new() -> MeleeWeapon {
        MeleeWeapon {}
    }
}

impl Script for MeleeWeapon {
    fn initialize(&mut self, entity_id: EntityId, world: &World) -> Effect {
        // TEMPLATES: 990, 1707
        let v_player_gun = world.borrow::<View<PropLimbModel>>().unwrap();

        let maybe_player_gun = v_player_gun.get(entity_id);

        println!(
            "!!debug wrench - initializing melee weapon: {:?} ent id: {:?}",
            maybe_player_gun, entity_id,
        );
        if let Ok(player_gun) = maybe_player_gun {
            // if (!player_gun.hand_model.contains("atek")
            //     && !player_gun.hand_model.contains("sg")
            //     && !player_gun.hand_model.contains("ar")
            //     && !player_gun.hand_model.contains("emp")
            //     && !player_gun.hand_model.contains("gren")
            //     && !player_gun.hand_model.contains("sfg")
            //     && !player_gun.hand_model.contains("amp_h"))
            // {
            //     panic!("Player gun: {:?}", player_gun);
            // }
            Effect::ChangeModel {
                entity_id,
                model_name: player_gun.0.clone(),
            }
        } else {
            Effect::NoEffect
        }
    }

    fn handle_message(
        &mut self,
        _entity_id: EntityId,
        _world: &World,
        _physics: &PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        match msg {
            MessagePayload::Collided { with } => Effect::Send {
                msg: Message {
                    to: *with,
                    payload: MessagePayload::Damage { amount: 1.0 },
                },
            },
            _ => Effect::NoEffect,
        }
    }
}
