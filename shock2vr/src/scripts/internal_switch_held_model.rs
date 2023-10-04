use std::f32::consts::PI;

use cgmath::{vec3, Deg, Matrix4, Quaternion, Rad, Rotation3};
use dark::properties::{
    GunFlashOptions, InternalPropOriginalModelName, Link, ProjectileOptions, PropLimbModel,
    PropPlayerGun,
};
use engine::audio::AudioHandle;
use shipyard::{EntityId, Get, View, World};

use crate::{
    physics::PhysicsWorld,
    runtime_props::{RuntimePropTransform, RuntimePropVhots},
};

use super::{
    script_util::{
        get_all_links_with_template, get_first_link_with_template_and_data,
        play_environmental_sound,
    },
    Effect, MessagePayload, Script,
};

pub struct InternalSwitchHeldModelScript;
impl InternalSwitchHeldModelScript {
    pub fn new() -> InternalSwitchHeldModelScript {
        InternalSwitchHeldModelScript
    }
}

impl Script for InternalSwitchHeldModelScript {
    fn initialize(&mut self, entity_id: EntityId, world: &World) -> Effect {
        // // let v_player_gun = world.borrow::<View<PropLimbModel>>().unwrap();

        // // let maybe_player_gun = v_player_gun.get(entity_id);

        // // println!(
        // //     "!!debug wrench - initializing melee weapon: {:?} ent id: {:?}",
        // //     maybe_player_gun, entity_id,
        // // );
        // // if let Ok(player_gun) = maybe_player_gun {
        // //     // if (!player_gun.hand_model.contains("atek")
        // //     //     && !player_gun.hand_model.contains("sg")
        // //     //     && !player_gun.hand_model.contains("ar")
        // //     //     && !player_gun.hand_model.contains("emp")
        // //     //     && !player_gun.hand_model.contains("gren")
        // //     //     && !player_gun.hand_model.contains("sfg")
        // //     //     && !player_gun.hand_model.contains("amp_h"))
        // //     // {
        // //     //     panic!("Player gun: {:?}", player_gun);
        // //     // }
        // //     Effect::ChangeModel {
        // //         entity_id,
        // //         model_name: player_gun.0.clone(),
        // //     }
        // // } else {
        // //     Effect::NoEffect
        // // }

        // let v_player_gun = world.borrow::<View<PropPlayerGun>>().unwrap();

        // let maybe_player_gun = v_player_gun.get(entity_id);

        // if let Ok(player_gun) = maybe_player_gun {
        //     // if (!player_gun.hand_model.contains("atek")
        //     //     && !player_gun.hand_model.contains("sg")
        //     //     && !player_gun.hand_model.contains("ar")
        //     //     && !player_gun.hand_model.contains("emp")
        //     //     && !player_gun.hand_model.contains("gren")
        //     //     && !player_gun.hand_model.contains("sfg")
        //     //     && !player_gun.hand_model.contains("amp_h"))
        //     // {
        //     //     panic!("Player gun: {:?}", player_gun);
        //     // }
        //     Effect::ChangeModel {
        //         entity_id,
        //         model_name: player_gun.hand_model.clone(),
        //     }
        // } else {
        //     Effect::NoEffect
        // }
        Effect::NoEffect
    }

    fn handle_message(
        &mut self,
        entity_id: EntityId,
        world: &World,
        _physics: &PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        match msg {
            MessagePayload::Hold => {
                let v_player_gun = world.borrow::<View<PropPlayerGun>>().unwrap();
                let v_melee_weapon = world.borrow::<View<PropLimbModel>>().unwrap();

                if let Ok(player_gun) = v_player_gun.get(entity_id) {
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
                        model_name: player_gun.hand_model.clone(),
                    }
                } else if let Ok(limb_model) = v_melee_weapon.get(entity_id) {
                    Effect::ChangeModel {
                        entity_id,
                        model_name: limb_model.0.clone(),
                    }
                } else {
                    Effect::NoEffect
                }
            }

            MessagePayload::Drop => {
                let v_player_gun = world
                    .borrow::<View<InternalPropOriginalModelName>>()
                    .unwrap();

                let maybe_player_gun = v_player_gun.get(entity_id);
                println!(
                    "atek - maybe_player_gun: ${:?} entity_id: ${:?}",
                    maybe_player_gun, entity_id
                );

                if let Ok(player_gun) = maybe_player_gun {
                    Effect::ChangeModel {
                        entity_id,
                        model_name: player_gun.0.clone(),
                    }
                } else {
                    Effect::NoEffect
                }
            }
            _ => Effect::NoEffect,
        }
    }
}
