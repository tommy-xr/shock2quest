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
                let is_vr = world
                    .borrow::<UniqueView<GlobalPresentationMode>>()
                    .map(|mode| mode.0 == PresentationMode::Vr)
                    .unwrap_or(false);
                if is_vr {
                    // On a 25AE install the remastered first-person gun models
                    // resolve (mods/sshock2ee.kpf outranks the classic
                    // archives) and are closed meshes, so VR wields them
                    // directly - ChangeModel also adopts their muzzle vhots.
                    if crate::is_25th_anniversary_install() {
                        if let Some(view_model) = get_raw_view_model(world, entity_id)
                            .filter(|name| vr_config::is_vr_view_model(name))
                        {
                            return Effect::ChangeModel {
                                entity_id,
                                model_name: view_model,
                            };
                        }
                    }

                    // Otherwise keep the world model: the classic _h meshes
                    // have their never-visible faces stripped for the fixed
                    // flat camera and look broken from VR's free viewpoints
                    // (#352).
                    let mut effects = Vec::new();

                    // Self-heal cross-mode saves: a save made in flat while
                    // wielding persists the _h viewmodel name; restore the
                    // original world model (a no-op in the normal VR flow).
                    if let (Some(original), Some(current)) = (
                        get_previous_model(world, entity_id),
                        get_current_model(world, entity_id),
                    ) {
                        if original != current {
                            effects.push(Effect::ChangeModel {
                                entity_id,
                                model_name: original,
                            });
                        }
                    }

                    // Keep the world model's own attachment IDs and barrel
                    // geometry. A hand-model donor can have different axes.

                    return Effect::Multiple(effects);
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
                    return Effect::ChangeModel {
                        entity_id,
                        model_name: view_model,
                    };
                }
                // No original model to restore. For the PsiSword the VR wield
                // *materialized* one (it authors only `PropLimbModel`), so the
                // drop has to take it away again - otherwise the first-person
                // arm mesh stays in the world where the sword was released.
                match get_current_model(world, entity_id) {
                    Some(current) if vr_config::is_vr_view_model(&current) => {
                        Effect::ClearModel { entity_id }
                    }
                    _ => Effect::NoEffect,
                }
            }

            _ => Effect::NoEffect,
        }
    }
}

/// The entity's authored first-person model name, unfiltered:
/// `PropPlayerGun.hand_model` for guns, `PropLimbModel` for melee.
fn get_raw_view_model(world: &World, entity_id: EntityId) -> Option<String> {
    let v_player_gun = world.borrow::<View<PropPlayerGun>>().unwrap();
    let v_melee_weapon = world.borrow::<View<PropLimbModel>>().unwrap();

    if let Ok(player_gun) = v_player_gun.get(entity_id) {
        Some(player_gun.hand_model.clone())
    } else if let Ok(limb_model) = v_melee_weapon.get(entity_id) {
        Some(limb_model.0.clone())
    } else {
        None
    }
}

fn get_view_model(world: &World, entity_id: EntityId) -> Option<String> {
    get_raw_view_model(world, entity_id).filter(|str| vr_config::is_allowed_hand_model(str))
}

fn get_current_model(world: &World, entity_id: EntityId) -> Option<String> {
    let v_model_name = world
        .borrow::<View<dark::properties::PropModelName>>()
        .unwrap();
    v_model_name
        .get(entity_id)
        .ok()
        .map(|model| model.0.clone())
}

fn get_previous_model(world: &World, entity_id: EntityId) -> Option<String> {
    let v_player_gun = world
        .borrow::<View<InternalPropOriginalModelName>>()
        .unwrap();

    let maybe_player_gun = v_player_gun.get(entity_id);
    maybe_player_gun.ok().map(|player_gun| player_gun.0.clone())
}
