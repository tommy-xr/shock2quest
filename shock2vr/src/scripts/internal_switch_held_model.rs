use dark::properties::{InternalPropOriginalModelName, PropLimbModel, PropPlayerGun};
use shipyard::{EntityId, Get, UniqueView, View, World};

use crate::{PresentationMode, mission::GlobalPresentationMode, physics::PhysicsWorld, vr_config};

use super::{Effect, MessagePayload, Script};

pub struct InternalSwitchHeldModelScript;
impl InternalSwitchHeldModelScript {
    pub fn new() -> InternalSwitchHeldModelScript {
        InternalSwitchHeldModelScript
    }
}

impl Script for InternalSwitchHeldModelScript {
    fn handle_message(
        &mut self,
        entity_id: EntityId,
        world: &World,
        _physics: &PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        match msg {
            MessagePayload::Hold => {
                // The swap to the first-person hand model (_h mesh) is
                // flat-only: those meshes have their never-visible faces
                // stripped for the fixed flat camera and look broken from
                // VR's free viewpoints, so VR keeps the world model (#352).
                let is_vr = world
                    .borrow::<UniqueView<GlobalPresentationMode>>()
                    .map(|mode| mode.0 == PresentationMode::Vr)
                    .unwrap_or(false);
                if is_vr {
                    return Effect::NoEffect;
                }

                if let Some(view_model) = get_view_model(world, entity_id) {
                    Effect::ChangeModel {
                        entity_id,
                        model_name: view_model,
                    }
                } else {
                    Effect::NoEffect
                }
            }

            MessagePayload::Drop => {
                if let Some(view_model) = get_previous_model(world, entity_id) {
                    Effect::ChangeModel {
                        entity_id,
                        model_name: view_model,
                    }
                } else {
                    Effect::NoEffect
                }
            }

            _ => Effect::NoEffect,
        }
    }
}

fn get_view_model(world: &World, entity_id: EntityId) -> Option<String> {
    let v_player_gun = world.borrow::<View<PropPlayerGun>>().unwrap();
    let v_melee_weapon = world.borrow::<View<PropLimbModel>>().unwrap();

    let ret = {
        if let Ok(player_gun) = v_player_gun.get(entity_id) {
            Some(player_gun.hand_model.clone())
        } else if let Ok(limb_model) = v_melee_weapon.get(entity_id) {
            Some(limb_model.0.clone())
        } else {
            None
        }
    };

    ret.filter(|str| vr_config::is_allowed_hand_model(str))
}

fn get_previous_model(world: &World, entity_id: EntityId) -> Option<String> {
    let v_player_gun = world
        .borrow::<View<InternalPropOriginalModelName>>()
        .unwrap();

    let maybe_player_gun = v_player_gun.get(entity_id);
    maybe_player_gun.ok().map(|player_gun| player_gun.0.clone())
}
