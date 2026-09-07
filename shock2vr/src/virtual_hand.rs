// Helper to convert the input context to a form more useful for gameplay / interacting with the world

use cgmath::{Array, InnerSpace, Quaternion, Rotation, Vector3, Zero, point3, vec3};
use dark::{
    SCALE_FACTOR,
    properties::{FrobFlag, PropFrobInfo, PropModelName},
};
use engine::assets::asset_cache::AssetCache;
use engine::scene::SceneObject;
use engine::script_log;

use rapier3d::prelude::RigidBodyHandle;
use shipyard::{EntityId, Get, UniqueView, View, World};
use tracing::{self, trace};

use crate::{
    gui::GuiPropProxyEntity,
    hand_affordance::{self, AffordanceTracker, HandAffordance},
    input_context::Hand,
    physics::{InternalCollisionGroups, PhysicsWorld, RayCastResult},
    runtime_props::RuntimePropVrGunWield,
    scripts::{Message, MessagePayload},
    util::{self, point3_to_vec3},
    vr_config::{self, Handedness},
};

const HAND_OFFSET: Vector3<f32> = vec3(0.0, 0.0, 0.0);

/// Where a tracked controller is in the world. Inputs are in pawn space, so
/// `vr_climb` resolves grips against exactly the pose the hand is drawn and
/// raycast from.
pub fn hand_world_position(
    pawn_pos: Vector3<f32>,
    pawn_rotation: Quaternion<f32>,
    hand_local: Vector3<f32>,
) -> Vector3<f32> {
    pawn_pos + HAND_OFFSET + pawn_rotation.rotate_vector(hand_local)
}

/// Maximum world-space distance from the hand/eye to a frob target's visible
/// surface. Retail `shock2.gam` authors `GAMEPARAM.Frob Dist = 50`; the
/// original `PickSetFocus` treats that as squared SS2 units, while this engine
/// divides authored world geometry by [`SCALE_FACTOR`].
pub(crate) const FROB_REACH: f32 = 7.071_068 / SCALE_FACTOR;

#[derive(Clone)]
pub struct VirtualHand {
    position: Vector3<f32>,
    rotation: Quaternion<f32>,
    trigger_value: f32,
    squeeze_value: f32,
    raytrace_hit: Option<RayCastResult>,

    // Keep track of last frobbed entity so frobbing is 'semi-auto'
    last_frobbed_entity: Option<EntityId>,

    hand_state: HandState,

    /// What this hand could do with whatever it is pointing at - the light on
    /// the glove and the pre-shape it leans into.
    affordance: AffordanceTracker,

    handedness: Handedness,
}

#[derive(Debug)]
pub enum VirtualHandEffect {
    OutMessage {
        message: Message,
    },
    // Keeping to allow for debugging
    #[allow(dead_code)]
    ApplyForce {
        entity_id: EntityId,
        force: Vector3<f32>,
        torque: Vector3<f32>,
    },
    SetPositionRotation {
        entity_id: EntityId,
        position: Vector3<f32>,
        rotation: Quaternion<f32>,
        scale: Vector3<f32>,
    },
    // Keeping to allow for debugging
    #[allow(dead_code)]
    SpawnEntity {
        template_id: i32,
        position: Vector3<f32>,
        rotation: Quaternion<f32>,
    },
    HoldItem {
        entity_id: EntityId,
    },
    /// Move an item into the player's backpack - a flat world pickup of
    /// ordinary loot, the weapon a wield swap displaced, or a VR hand opened
    /// over the cyber interface's inventory strip. VR's ordinary release is
    /// still the physical `DropItem`; only the strip claims one.
    ///
    /// Storing an item that was HELD must be paired with a preceding
    /// `MessagePayload::Drop` to it (see `FlatPlayerController::wield` and
    /// `mission_core`'s `rewrite_strip_release`): unlike `DropItem`, this
    /// variant does not dispatch one, so the world-model restore and
    /// psi-charge cancel would otherwise be skipped.
    StoreItem {
        entity_id: EntityId,
    },
    /// Like [`Self::StoreItem`], but for a VR release aimed at a specific
    /// backpack grid cell (the cyber-interface strip deposit): retail drops
    /// the item into the cell the player is pointing at rather than always
    /// the first free one. Carries the same held-item `Drop`-first contract
    /// as `StoreItem`.
    StoreItemAtCell {
        entity_id: EntityId,
        cell: (usize, usize),
    },
    /// Eject an item into the world (it regains physics + its world model).
    /// This is an explicit drop only: VR opening its hand. Losing an item to a
    /// wield swap is a `StoreItem`, not a drop (#777).
    DropItem {
        entity_id: EntityId,
    },
}

// All the details we need for the item we are grabbing..
#[derive(Copy, Clone)]
pub enum HandState {
    Empty, // Hand is not holding anything

    Grabbing {
        // Identifiers for world / physics
        entity_id: EntityId,
        // rigid_body_handle: RigidBodyHandle,
    },
}

impl VirtualHand {
    pub fn new(handedness: Handedness) -> VirtualHand {
        VirtualHand {
            position: Vector3::zero(),
            rotation: Quaternion {
                v: Vector3::zero(),
                s: 1.0,
            },
            trigger_value: 0.0,
            squeeze_value: 0.0,
            raytrace_hit: None,
            last_frobbed_entity: None,
            hand_state: HandState::Empty,
            affordance: Default::default(),
            handedness,
        }
    }
    pub fn destroy_entity(&self, entity_to_destroy_id: EntityId) -> VirtualHand {
        match self.hand_state {
            // Nothing to do here!
            HandState::Empty => self.clone(),
            HandState::Grabbing { entity_id } => {
                if entity_id == entity_to_destroy_id {
                    VirtualHand {
                        hand_state: HandState::Empty,
                        ..self.clone()
                    }
                } else {
                    self.clone()
                }
            }
        }
    }

    pub fn get_held_entity(&self) -> Option<EntityId> {
        match self.hand_state {
            HandState::Empty => None,
            HandState::Grabbing { entity_id, .. } => Some(entity_id),
        }
    }

    pub fn get_raytraced_entity(&self) -> Option<EntityId> {
        match &self.raytrace_hit {
            None => None,
            Some(hit) => hit.maybe_entity_id,
        }
    }

    pub fn is_holding(&self, entity_id: EntityId) -> bool {
        self.get_held_entity() == Some(entity_id)
    }

    pub fn get_position(&self) -> Vector3<f32> {
        self.position
    }

    pub fn get_rotation(&self) -> Quaternion<f32> {
        self.rotation
    }

    pub fn grab_entity(
        &self,
        _world: &World,
        entity_id: EntityId,
        // entity_rigid_body: RigidBodyHandle,
    ) -> VirtualHand {
        if self.get_held_entity().is_some() {
            // Already holding something!
            return self.clone();
        }

        VirtualHand {
            hand_state: HandState::Grabbing { entity_id },
            ..self.clone()
        }
    }

    pub fn replace_entity(
        &self,
        old_entity_id: EntityId,
        new_entity_id: EntityId,
        _new_rigid_body: RigidBodyHandle,
    ) -> VirtualHand {
        match self.hand_state {
            HandState::Empty => self.clone(),
            HandState::Grabbing { entity_id } => {
                if entity_id == old_entity_id {
                    VirtualHand {
                        hand_state: HandState::Grabbing {
                            entity_id: new_entity_id,
                        },
                        ..self.clone()
                    }
                } else {
                    self.clone()
                }
            }
        }
    }

    pub fn update(
        prev: &VirtualHand,
        physics: &PhysicsWorld,
        world: &World,
        pawn_pos: Vector3<f32>,
        pawn_rot: Quaternion<f32>,
        input_hand: &Hand,
        held_by_other_hand: Option<EntityId>,
        anchor_claim: Option<crate::body_frame::HandClaim>,
        two_hand: crate::two_hand_grip::TwoHandFrame,
    ) -> (VirtualHand, Vec<VirtualHandEffect>) {
        let handedness = prev.handedness;
        let hand_position = hand_world_position(pawn_pos, pawn_rot, input_hand.position);
        let hand_rotation = pawn_rot * input_hand.rotation;

        // Also do a raycast to provide the 'Hover' effect
        let ray_start = point3(hand_position.x, hand_position.y, hand_position.z);
        let forward = hand_rotation.rotate_vector(vec3(0.0, 0.0, -1.0));
        let result =
            interaction_ray_cast(physics, world, ray_start, forward, prev.get_held_entity());

        let (hand, mut effs) = match prev.hand_state {
            HandState::Grabbing { entity_id } => {
                // See what we're hitting
                let mut msgs = Vec::new();

                // A full hand has nothing to reach for. Clear rather than decay:
                // closing on an item is a hard transition, so the light must not
                // keep advertising the grab for the hysteresis window. A body
                // anchor is the exception - a hand carried to the shoulder must
                // show whether opening it there will stow, before it opens.
                let mut affordance = AffordanceTracker::default();
                if let Some(crate::body_frame::HandClaim::Anchor(observed)) = anchor_claim {
                    affordance.update(observed, 0.0, false);
                }

                // If we're holding onto something, but not grabbing, we can drop it
                if input_hand.squeeze_value < 0.5 {
                    let mut msgs = vec![VirtualHandEffect::DropItem { entity_id }];

                    let result_copy = result.clone();
                    if let Some(ray_cast_result) = result_copy {
                        if let Some(hit_entity_id) = ray_cast_result.maybe_entity_id {
                            msgs.push(VirtualHandEffect::OutMessage {
                                message: Message {
                                    to: hit_entity_id,
                                    payload: MessagePayload::ProvideForConsumption {
                                        entity: entity_id,
                                    },
                                },
                            });
                        }
                    };

                    let updated_hand = VirtualHand {
                        position: hand_position,
                        rotation: hand_rotation,
                        trigger_value: input_hand.trigger_value,
                        squeeze_value: input_hand.squeeze_value,
                        raytrace_hit: None,
                        last_frobbed_entity: None,
                        hand_state: HandState::Empty,
                        affordance,
                        handedness,
                    };
                    (updated_hand, msgs)
                } else {
                    let next_hand_state = prev.hand_state;
                    // let position = &physics.get_position(rigid_body_handle).unwrap();
                    // let velocity = physics.get_velocity(rigid_body_handle).unwrap();
                    // let hold_position = hand_position + offset_position;
                    // let dir = hold_position - position;

                    let vr_offsets = get_held_position_orientation(entity_id, world, handedness);

                    let v_model_name = world.borrow::<View<PropModelName>>().unwrap();
                    let _maybe_model_name = v_model_name.get(entity_id);

                    if prev.trigger_value < 0.5 && input_hand.trigger_value > 0.5 {
                        msgs.push(VirtualHandEffect::OutMessage {
                            message: Message {
                                to: entity_id,
                                payload: held_trigger_press_payload(world, entity_id),
                            },
                        });
                    }

                    if prev.trigger_value > 0.5 && input_hand.trigger_value < 0.5 {
                        script_log!(DEBUG, "Hand releasing trigger");
                        msgs.push(VirtualHandEffect::OutMessage {
                            message: Message {
                                to: entity_id,
                                payload: MessagePayload::TriggerRelease,
                            },
                        });
                    }

                    // A second hand on the item re-aims it down the line
                    // between the hands; the tracked wrist still owns where it
                    // is. One transform either way, so the muzzle, the magazine
                    // anchors and the melee drive's kinematic target all follow.
                    let placement = two_hand.aim_rotation.unwrap_or(hand_rotation);
                    msgs.push(VirtualHandEffect::SetPositionRotation {
                        entity_id,
                        // The offset is hand-local (e.g. seating a weapon's
                        // grip in the palm), so it must rotate with the hand
                        position: hand_position + placement.rotate_vector(vr_offsets.offset),
                        rotation: placement * vr_offsets.rotation,
                        // A pickup's authored held scale (`vr_grips`): the
                        // world models run well over life size and the glove
                        // holding them is life size. Guns report 1.0 - their
                        // shrink is already baked into the wielded geometry.
                        scale: Vector3::from_value(vr_config::held_render_scale(world, entity_id)),
                    });

                    let updated_hand = VirtualHand {
                        position: hand_position,
                        rotation: hand_rotation,
                        trigger_value: input_hand.trigger_value,
                        squeeze_value: input_hand.squeeze_value,
                        raytrace_hit: None,
                        last_frobbed_entity: None,
                        hand_state: next_hand_state,
                        affordance,
                        handedness,
                    };
                    (updated_hand, msgs)
                }
            }
            HandState::Empty => handle_empty_hand_state(
                handedness,
                hand_position,
                hand_rotation,
                prev.last_frobbed_entity,
                prev.affordance,
                prev.squeeze_value,
                world,
                physics,
                input_hand,
                held_by_other_hand,
                anchor_claim,
                two_hand,
            ),
        };

        match result {
            Some(RayCastResult {
                hit_point,
                hit_normal: _,
                maybe_entity_id: Some(to_entity_id),
                maybe_rigid_body_handle: _,
                is_sensor: _,
            }) => effs.push(VirtualHandEffect::OutMessage {
                message: Message {
                    to: to_entity_id,
                    payload: MessagePayload::Hover {
                        held_entity_id: hand.get_held_entity(),
                        world_position: point3_to_vec3(hit_point),
                        is_triggered: input_hand.trigger_value > 0.5,
                        is_grabbing: input_hand.squeeze_value > 0.5,
                        hand: hand.handedness,
                    },
                },
            }),
            _ => (),
        };

        (hand, effs)
    }

    /// What this hand could do with whatever it is pointing at, after
    /// hysteresis and any failure pulse. Drives the glove's light and its
    /// pre-shape, and is reported over HTTP for tests.
    pub fn affordance(&self) -> HandAffordance {
        self.affordance.state()
    }

    /// Which hand this is.
    pub fn handedness(&self) -> Handedness {
        self.handedness
    }

    /// Render the glove for this hand, and report the grip it fitted to
    /// whatever the hand holds. The hand renderer is owned by the caller
    /// (`VrInteraction`) so its cached state - the fit cache included - is
    /// shared between both hands.
    pub fn render(
        &self,
        world: &World,
        asset_cache: &mut AssetCache,
        renderer: &mut crate::hand_glove::GloveRenderer,
        support: Option<crate::two_hand_grip::SupportLatch>,
    ) -> (Vec<SceneObject>, Option<crate::game_scene::DebugHandGrip>) {
        // A wielded gun draws the glove in place of the baked hand the wield
        // stripped off it; everything else draws it at the tracked hand, or not
        // at all when the held model draws a hand of its own. Either way the
        // glove is the tracked pose at life size - the gun is the thing scaled
        // to meet it (`vr_config::gun_wield_scale`).
        // A hand supporting the other's weapon is an empty hand as far as its
        // own visual goes: it always draws.
        if support.is_none()
            && !holds_gun_glove(world, self.get_held_entity())
            && !shows_hand_visual(world, self.get_held_entity())
        {
            return (Vec::new(), None);
        }
        let hand = crate::hand_glove::hand_to_world(self.position, self.rotation, self.handedness);

        // The fit is solved against the held item's own mesh and cached by the
        // renderer, so a hand that keeps holding the same thing pays for it
        // once; a held model with no mesh to close against reports `None` and
        // falls back to the generic wrap.
        // A support hand closes on the item at the point it latched, not at the
        // item's own grip, so it fits against the same mesh placed there.
        let fit = match support {
            Some(latch) => vr_config::held_model_and_scale(world, latch.entity_id).and_then(
                |(model_name, scale)| {
                    renderer.support_grip(
                        &model_name,
                        self.handedness,
                        scale,
                        latch.point,
                        asset_cache,
                    )
                },
            ),
            None => self.get_held_entity().and_then(|entity_id| {
                let (model_name, scale) = vr_config::held_model_and_scale(world, entity_id)?;
                renderer.fitted_grip(&model_name, self.handedness, scale, asset_cache)
            }),
        };
        let hold = match (support, self.get_held_entity()) {
            (Some(_), _) | (_, Some(_)) => {
                crate::hand_glove::Hold::Item(fit.map(|fit| fit.amounts))
            }
            _ => crate::hand_glove::Hold::Empty,
        };

        let objects = renderer.render_hand(
            hand,
            self.trigger_value,
            self.squeeze_value,
            hold,
            // Eligibility lights the glove; the action shapes it.
            self.affordance.state().light(),
            self.affordance.preshape(),
        );

        let grip = fit.map(|fit| crate::game_scene::DebugHandGrip {
            family: fit.family.as_str(),
            thumb: fit.amounts.thumb,
            index: fit.amounts.index,
            middle: fit.amounts.middle,
            ring: fit.amounts.ring,
            pinky: fit.amounts.pinky,
        });

        (objects, grip)
    }
}

/// Whether the held entity is a wield that draws the glove over the weapon -
/// the arm-stripped gun `_h` set, marked by the wield itself (see
/// [`crate::runtime_props::RuntimePropVrGunWield`]).
fn holds_gun_glove(world: &World, held_entity: Option<EntityId>) -> bool {
    let Some(entity_id) = held_entity else {
        return false;
    };
    world
        .borrow::<View<RuntimePropVrGunWield>>()
        .map(|marked| marked.get(entity_id).is_ok())
        .unwrap_or(false)
}

fn handle_empty_hand_state(
    handedness: Handedness,
    hand_position: Vector3<f32>,
    hand_rotation: Quaternion<f32>,
    frobbed_entity: Option<EntityId>,
    mut affordance: AffordanceTracker,
    prev_squeeze: f32,
    world: &World,
    physics: &PhysicsWorld,
    input_hand: &Hand,
    held_by_other_hand: Option<EntityId>,
    anchor_claim: Option<crate::body_frame::HandClaim>,
    two_hand: crate::two_hand_grip::TwoHandFrame,
) -> (VirtualHand, Vec<VirtualHandEffect>) {
    let ray_start = point3(hand_position.x, hand_position.y, hand_position.z);
    let forward = hand_rotation.rotate_vector(vec3(0.0, 0.0, -1.0));
    let result = interaction_ray_cast(physics, world, ray_start, forward, held_by_other_hand);
    trace!("ray cast result: {:?}", &result);

    // One resolved target per hand per frame: the glove's light and pre-shape
    // read the same hit the trigger and squeeze below act on, so the hand can
    // never advertise an action the input would refuse.
    //
    // A hand at one of the player's own body anchors is the exception: it is
    // reaching for the belt or a shoulder, not for whatever its ray happens to
    // cross, so the anchor owns the light as well as the input this frame
    // (`mission_core` runs the gesture the grip commits to). A hand *carrying*
    // the belt card is claimed too, but keeps its own light: what it needs to
    // show is the reader it is being held against.
    let (observed, preshape_weight) = match anchor_claim {
        Some(crate::body_frame::HandClaim::Anchor(observed)) => {
            (observed, hand_affordance::preshape_weight(0.0))
        }
        Some(crate::body_frame::HandClaim::CarryingCard) | None => {
            match result.as_ref().and_then(|hit| {
                hit.maybe_entity_id
                    .map(|entity_id| (entity_id, (hit.hit_point - ray_start).magnitude()))
            }) {
                Some((entity_id, distance)) => (
                    hand_affordance::classify(world, entity_id),
                    hand_affordance::preshape_weight(distance),
                ),
                None => (HandAffordance::None, 0.0),
            }
        }
    };
    // A hand on the other hand's item advertises the support grip it could
    // take, whatever its ray happens to cross past the item.
    let (observed, preshape_weight) = if two_hand.offered {
        (
            HandAffordance::Grabbable,
            hand_affordance::preshape_weight(0.0),
        )
    } else {
        (observed, preshape_weight)
    };
    let mut attempt_failed = false;
    // A refusal is one press, not one per frame the player keeps holding.
    let squeeze_pressed = prev_squeeze <= 0.5 && input_hand.squeeze_value > 0.5;

    let mut msgs = Vec::new();
    let mut last_frobbed_entity = frobbed_entity;
    let mut next_hand_state = HandState::Empty;
    // A claimed hand reaches for the body, not through it: its grip belongs to
    // the anchor gesture, so it must not also grab or frob what the ray found.
    // A supporting hand holds the other hand's weapon: like a hand at a body
    // anchor, its grip is spoken for and must not also grab or frob whatever
    // its ray found past the weapon.
    let claimed_by_anchor = anchor_claim.is_some() || two_hand.supporting;
    if !claimed_by_anchor && (input_hand.trigger_value > 0.5 || input_hand.a_value > 0.5) {
        if let Some(RayCastResult {
            hit_point: _,
            hit_normal: _,
            maybe_entity_id: Some(entity),
            maybe_rigid_body_handle: _,
            is_sensor: _,
        }) = result
        {
            if last_frobbed_entity != Some(entity) {
                msgs.push(VirtualHandEffect::OutMessage {
                    message: Message {
                        to: entity,
                        payload: {
                            if input_hand.trigger_value > 0.5 {
                                MessagePayload::Frob
                            } else {
                                MessagePayload::Slay
                                //MessagePayload::Damage { amount: 1.0 }
                            }
                        },
                    },
                });
                last_frobbed_entity = Some(entity);
                // The frob still goes out - the script owns the refusal - but a
                // locked target is one the hand knew it could not open.
                attempt_failed = observed == HandAffordance::Blocked;

                // Also, frob any items that may be nearby...
            }
        }
    } else if input_hand.squeeze_value <= 0.5 {
        // Only clear the latch once neither trigger nor squeeze is engaged -
        // otherwise a squeeze held across frames (below) would see the latch
        // cleared every frame the trigger is idle and re-frob the same
        // entity every tick it survives.
        last_frobbed_entity = None
    }

    if !claimed_by_anchor && input_hand.squeeze_value > 0.5 {
        if let Some(RayCastResult {
            hit_point: _,
            hit_normal: _,
            maybe_entity_id: Some(entity_id),
            maybe_rigid_body_handle: Some(rigid_body_handle),
            is_sensor: _,
        }) = result
        {
            let needs_scripted_frob = uses_scripted_world_frob(world, entity_id);
            if Some(entity_id) != held_by_other_hand
                && can_grab_item(world, entity_id)
                && !needs_scripted_frob
            {
                let position = &physics.get_position(rigid_body_handle).unwrap();
                let _dir = hand_position - position;
                msgs.push(VirtualHandEffect::HoldItem { entity_id });

                next_hand_state = HandState::Grabbing { entity_id };
            } else if Some(entity_id) != held_by_other_hand
                && needs_scripted_frob
                && last_frobbed_entity != Some(entity_id)
            {
                // Items whose taking must go through a script (nanites
                // collected straight into the player stat, keycards, ...) are
                // always Frob'd, never squeeze-grabbed into the hand - see
                // `uses_scripted_world_frob`.
                msgs.push(VirtualHandEffect::OutMessage {
                    message: Message {
                        to: entity_id,
                        payload: MessagePayload::Frob,
                    },
                });
                last_frobbed_entity = Some(entity_id);
            } else if squeeze_pressed && !needs_scripted_frob {
                // Squeezing scenery with no `MOVE` and no script to take it.
                // A scripted target already Frobbed by this same press is not a
                // refusal - it is the press still being held.
                attempt_failed = true;
            }
        }
    }

    affordance.update(observed, preshape_weight, attempt_failed);

    let updated_hand = VirtualHand {
        position: hand_position,
        rotation: hand_rotation,
        trigger_value: input_hand.trigger_value,
        squeeze_value: input_hand.squeeze_value,
        raytrace_hit: result,
        last_frobbed_entity,
        hand_state: next_hand_state,
        affordance,
        handedness,
    };
    (updated_hand, msgs)
}

///
/// get_held_position_orientation
///
/// Helper function to figure out the right way to 'hold' an item.
/// We use world models for our item, and the default orientation/position doesn't always make sense for how to hold it
/// (otherwise, we might end up holding the wrench sideways, or the shotgun backwards!)
fn get_held_position_orientation(
    entity_id: EntityId,
    world: &World,
    handedness: Handedness,
) -> vr_config::VRHandModelPerHandAdjustments {
    vr_config::get_vr_hand_model_adjustments_from_entity(entity_id, world, handedness)
}

/// Translate a production VR trigger press into the held object's authored
/// interaction. Retail consumables expose `inventory_action = SCRIPT`; using
/// one from a hand must therefore send the same `Frob` their inventory UI
/// would send. Weapons (including the Psi Amp) keep their dedicated trigger
/// protocol even though the Weapon archetype also inherits that inventory
/// flag. The decision stays tied to Dark's production frob metadata.
fn held_trigger_press_payload(world: &World, entity_id: EntityId) -> MessagePayload {
    // An always-collected item can only be in a hand at all if an older save put
    // it there, and it must still collect rather than act: PropKeySrc injects
    // `internal_keycard` at runtime even though retail ID cards author no
    // inventory SCRIPT flag, so the metadata check below would miss a card.
    if crate::scripts::script_util::is_always_collected(world, entity_id) {
        return MessagePayload::Frob;
    }

    let has_scripted_inventory_use = world
        .borrow::<View<PropFrobInfo>>()
        .map(|frob_info| {
            frob_info
                .get(entity_id)
                .is_ok_and(|frob_info| frob_info.inventory_action.contains(FrobFlag::SCRIPT))
        })
        .unwrap_or(false);

    if has_scripted_inventory_use && !is_wieldable_weapon(world, entity_id) {
        MessagePayload::Frob
    } else {
        MessagePayload::TriggerPull
    }
}

pub(crate) fn can_grab_item(world: &World, entity_id: EntityId) -> bool {
    let v_prop_frobinfo = world.borrow::<View<PropFrobInfo>>().unwrap();

    if let Ok(frob_info) = v_prop_frobinfo.get(entity_id) {
        if frob_info.world_action.contains(FrobFlag::MOVE)
            || frob_info.world_action.contains(FrobFlag::USE_AMMO)
        {
            return true;
        }
    }

    false
}

/// Whether this object is a key source. Every PropKeySrc receives the derived
/// `internal_keycard` script, whose Frob records the credential and consumes
/// the pickup; bypassing it creates a physical card that cannot unlock doors.
pub(crate) fn is_key_source(world: &World, entity_id: EntityId) -> bool {
    world
        .borrow::<View<dark::properties::PropKeySrc>>()
        .map(|keycards| keycards.get(entity_id).is_ok())
        .unwrap_or(false)
}

/// Whether taking this world item must go through a script before the physical
/// transfer. An authored `SCRIPT` world action owns its side effects and item
/// fate (for example `FrobQB` awards a quest bit and moves the item exactly
/// once). The always-collected categories - keycards, nanite piles, cyber
/// modules and logs - must route through Frob rather than a physical grab in
/// every presentation and on every path, even where their authored world action
/// is only `MOVE` (see `scripts::script_util::is_always_collected`, the shared
/// predicate every acquisition site consults).
pub(crate) fn uses_scripted_world_frob(world: &World, entity_id: EntityId) -> bool {
    let has_authored_world_script = world
        .borrow::<View<PropFrobInfo>>()
        .map(|frob_info| {
            frob_info
                .get(entity_id)
                .is_ok_and(|frob_info| frob_info.world_action.contains(FrobFlag::SCRIPT))
        })
        .unwrap_or(false);
    has_authored_world_script || crate::scripts::script_util::is_always_collected(world, entity_id)
}

/// Whether an inventory item is a wieldable weapon - a gun (`PropPlayerGun`) or
/// a melee arm (`PropLimbModel`). Clicking one in the backpack/strip wields it
/// (`Effect::GrabEntity`) instead of using it; shared by `ContainerGui` and the
/// flat cursor-drag wield so the two can't drift.
pub(crate) fn is_wieldable_weapon(world: &World, entity_id: EntityId) -> bool {
    world
        .borrow::<View<dark::properties::PropPlayerGun>>()
        .map(|v| v.get(entity_id).is_ok())
        .unwrap_or(false)
        || world
            .borrow::<View<dark::properties::PropLimbModel>>()
            .map(|v| v.get(entity_id).is_ok())
            .unwrap_or(false)
}

/// Whether the hand visual is drawn for a hand holding `held_entity`, at the
/// tracked hand's own pose.
///
/// A wielded weapon's model is drawn at the hand's transform, so a weapon
/// whose model draws a hand of its own *replaces* the glove - drawing both
/// puts a hand inside the gun. That is the melee `_h` rigs and the psi amp,
/// whose arm is part of the amp. A wielded **gun** keeps its glove instead,
/// seated on the weapon rather than at the tracked pose, and is answered by
/// [`held_glove_seat`] before this is consulted. Anything else - an empty
/// hand, or a held object that is not a wieldable weapon - keeps the hand.
pub(crate) fn shows_hand_visual(world: &World, held_entity: Option<EntityId>) -> bool {
    // Calibration override: draw the glove *as well as* the weapon model, so
    // the `_h` rig's baked fist can be compared against where the controller
    // actually is. See `dev_params::MELEE_GLOVE_OVERLAY`.
    if crate::dev_params::get(crate::dev_params::MELEE_GLOVE_OVERLAY) > 0.5 {
        return true;
    }
    match held_entity {
        None => true,
        Some(entity_id) => !is_wieldable_weapon(world, entity_id),
    }
}

/// Find the player's carried weapon matching an original gamesys weapon
/// archetype. Mission objects and carried items can have more-specific or
/// mission-local template ids, so selection follows the preserved canonical
/// class identity through the global inheritance hierarchy.
pub(crate) fn carried_weapon_by_class(world: &World, class_template_id: i32) -> Option<EntityId> {
    let hierarchy = world
        .borrow::<UniqueView<crate::mission::mission_core::GlobalTemplateHierarchy>>()
        .ok()?;

    crate::scripts::script_util::player_carried_items(world)
        .into_iter()
        .find(|entity| {
            is_wieldable_weapon(world, *entity)
                && crate::scripts::script_util::entity_class_template_id(world, *entity)
                    .is_some_and(|template_id| {
                        hierarchy.is_or_descends_from(template_id, class_template_id)
                    })
        })
}

///
/// In the case where we hit an entity that is 'proxied' (like, a hitbox that points to a parent),
/// resolve to the parent entity.
///
fn resolve_hit_proxy_entity(world: &World, ray_cast_result: RayCastResult) -> RayCastResult {
    let maybe_new_entity_id = ray_cast_result
        .maybe_entity_id
        .map(|entity_id| util::resolve_proxy_entity(world, entity_id));

    RayCastResult {
        maybe_entity_id: maybe_new_entity_id,
        ..ray_cast_result
    }
}

pub(crate) fn interaction_ray_cast(
    physics: &PhysicsWorld,
    world: &World,
    ray_start: cgmath::Point3<f32>,
    forward: Vector3<f32>,
    entity_to_ignore: Option<EntityId>,
) -> Option<RayCastResult> {
    let ordinary_groups = InternalCollisionGroups::ENTITIES
        | InternalCollisionGroups::SELECTABLE
        | InternalCollisionGroups::WORLD
        | InternalCollisionGroups::RAYCAST;
    let ui_hit = physics.ray_cast2(
        ray_start,
        forward,
        FROB_REACH,
        InternalCollisionGroups::UI,
        entity_to_ignore,
        true,
    );

    let gui_host = ui_hit.as_ref().and_then(|hit| {
        let proxy = hit.maybe_entity_id?;
        world
            .borrow::<View<GuiPropProxyEntity>>()
            .ok()?
            .get(proxy)
            .ok()
            .map(GuiPropProxyEntity::host_entity)
    });

    if let (Some(ui_hit), Some(gui_host)) = (ui_hit, gui_host) {
        let ui_distance = (ui_hit.hit_point - ray_start).magnitude();
        let is_not_panel_host =
            |entity_id| util::resolve_proxy_entity(world, entity_id) != gui_host;
        let blocker = physics.ray_cast2_with_entity_filter(
            ray_start,
            forward,
            ui_distance,
            ordinary_groups,
            entity_to_ignore,
            true,
            &is_not_panel_host,
        );

        blocker
            .map(|result| resolve_hit_proxy_entity(world, result))
            .or(Some(ui_hit))
    } else {
        physics
            .ray_cast2(
                ray_start,
                forward,
                FROB_REACH,
                ordinary_groups | InternalCollisionGroups::UI,
                entity_to_ignore,
                true,
            )
            .map(|result| resolve_hit_proxy_entity(world, result))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::physics::CollisionGroup;
    use dark::properties::{KeyCard, PropFrobInfo, PropKeySrc, PropPlayerGun};

    fn scripted_inventory_use() -> PropFrobInfo {
        PropFrobInfo {
            world_action: FrobFlag::MOVE,
            inventory_action: FrobFlag::SCRIPT,
            tool_action: FrobFlag::empty(),
        }
    }

    fn player_gun() -> PropPlayerGun {
        PropPlayerGun {
            flags: 0,
            hand_model: "held_h".to_owned(),
            icon_file: String::new(),
            model_offset: vec3(0.0, 0.0, 0.0),
            fire_offset: vec3(0.0, 0.0, 0.0),
            heading: 0,
            reload_pitch: 0,
            reload_rate: 0,
            gun_type: 0,
        }
    }

    fn held_trigger_payload(world: &World, entity_id: EntityId) -> MessagePayload {
        let hand = VirtualHand::new(Handedness::Right).grab_entity(world, entity_id);
        let mut input = Hand::default();
        input.squeeze_value = 1.0;
        input.trigger_value = 1.0;

        let (_, effects) = VirtualHand::update(
            &hand,
            &PhysicsWorld::new(),
            world,
            Vector3::zero(),
            Quaternion::new(1.0, 0.0, 0.0, 0.0),
            &input,
            None,
            None,
            Default::default(),
        );

        effects
            .into_iter()
            .find_map(|effect| match effect {
                VirtualHandEffect::OutMessage { message } if message.to == entity_id => {
                    Some(message.payload)
                }
                _ => None,
            })
            .expect("a rising trigger edge should message the actual held entity")
    }

    fn interaction_fixture() -> (World, PhysicsWorld, EntityId, EntityId) {
        let mut world = World::new();
        let host = world.add_entity(());
        let proxy = world.add_entity(GuiPropProxyEntity::new(host));
        let mut physics = PhysicsWorld::new();
        let identity = Quaternion::new(1.0, 0.0, 0.0, 0.0);

        physics.add_kinematic(
            host,
            vec3(0.0, 0.0, -1.0),
            identity,
            Vector3::zero(),
            vec3(1.0, 1.0, 0.4),
            CollisionGroup::selectable(),
            false,
        );
        physics.add_kinematic(
            proxy,
            vec3(0.0, 0.0, -1.4),
            identity,
            Vector3::zero(),
            vec3(1.0, 1.0, 0.02),
            CollisionGroup::ui(),
            false,
        );

        let player = world.add_entity(());
        let mut player_handle = physics.create_player(vec3(100.0, 100.0, 100.0), player);
        physics.update(Vector3::zero(), &mut player_handle);
        (world, physics, host, proxy)
    }

    fn update_queries(world: &mut World, physics: &mut PhysicsWorld) {
        let player = world.add_entity(());
        let mut player_handle = physics.create_player(vec3(100.0, 100.0, 100.0), player);
        physics.update(Vector3::zero(), &mut player_handle);
    }

    /// Register `entity` as a selectable kinematic body and refresh the
    /// query pipeline so it is immediately raycastable. Shared by every
    /// fixture below that plants an entity for the raycast to hit.
    fn register_kinematic_body(
        world: &mut World,
        physics: &mut PhysicsWorld,
        entity: EntityId,
        position: Vector3<f32>,
        size: Vector3<f32>,
        collision_group: CollisionGroup,
    ) {
        physics.add_kinematic(
            entity,
            position,
            Quaternion::new(1.0, 0.0, 0.0, 0.0),
            Vector3::zero(),
            size,
            collision_group,
            false,
        );
        update_queries(world, physics);
    }

    fn add_occluder(
        world: &mut World,
        physics: &mut PhysicsWorld,
        z: f32,
        collision_group: CollisionGroup,
    ) -> EntityId {
        let entity = world.add_entity(());
        register_kinematic_body(
            world,
            physics,
            entity,
            vec3(0.0, 0.0, z),
            vec3(1.0, 1.0, 0.1),
            collision_group,
        );
        entity
    }

    fn cast_at_fixture(
        world: &World,
        physics: &PhysicsWorld,
        ignored: Option<EntityId>,
    ) -> Option<EntityId> {
        interaction_ray_cast(
            physics,
            world,
            point3(0.0, 0.0, 0.0),
            vec3(0.0, 0.0, -1.0),
            ignored,
        )
        .and_then(|hit| hit.maybe_entity_id)
    }

    /// Negative-first regression for #1114: the transmitter's selectable
    /// bounds sit in front of its active keypad plane. Its own host must not
    /// make that world-panel UI permanently unreachable.
    #[test]
    fn world_panel_ui_bypasses_its_own_host_collider() {
        let (world, physics, _host, proxy) = interaction_fixture();

        assert_eq!(cast_at_fixture(&world, &physics, None), Some(proxy));
    }

    #[test]
    fn world_geometry_still_occludes_world_panel_ui() {
        let (mut world, mut physics, _host, _proxy) = interaction_fixture();
        let wall = add_occluder(
            &mut world,
            &mut physics,
            -0.5,
            CollisionGroup::world_for_test(),
        );

        assert_eq!(cast_at_fixture(&world, &physics, None), Some(wall));
    }

    #[test]
    fn unrelated_selectable_still_occludes_world_panel_ui() {
        let (mut world, mut physics, _host, _proxy) = interaction_fixture();
        let selectable = add_occluder(&mut world, &mut physics, -0.5, CollisionGroup::selectable());

        assert_eq!(cast_at_fixture(&world, &physics, None), Some(selectable));
    }

    #[test]
    fn held_entity_remains_excluded_from_world_panel_ray() {
        let (mut world, mut physics, _host, proxy) = interaction_fixture();
        let held = add_occluder(&mut world, &mut physics, -0.5, CollisionGroup::selectable());

        assert_eq!(cast_at_fixture(&world, &physics, Some(held)), Some(proxy));
    }

    /// Negative-first regression for #958: a held retail consumable owns an
    /// authored inventory SCRIPT action. The production VR trigger gesture
    /// must request that same Frob action instead of weapon fire.
    #[test]
    fn held_scripted_inventory_item_uses_its_authored_frob_action() {
        let mut world = World::new();
        let booster = world.add_entity(scripted_inventory_use());

        assert!(matches!(
            held_trigger_payload(&world, booster),
            MessagePayload::Frob
        ));
    }

    /// Guns and the Psi Amp both carry PropPlayerGun, so their trigger must
    /// continue reaching WeaponScript/PsiAmpScript as TriggerPull even though
    /// their inherited inventory action is also SCRIPT.
    #[test]
    fn held_player_gun_preserves_trigger_pull() {
        let mut world = World::new();
        let weapon_or_psi_amp = world.add_entity((scripted_inventory_use(), player_gun()));

        assert!(matches!(
            held_trigger_payload(&world, weapon_or_psi_amp),
            MessagePayload::TriggerPull
        ));
    }

    /// A keycard physically held by an older save or a pre-fix backpack grab
    /// must still be recoverable through the production VR trigger gesture.
    /// Its inventory metadata has no SCRIPT bit, so PropKeySrc is the semantic
    /// source of truth.
    #[test]
    fn held_keycard_trigger_collects_it_through_frob() {
        let mut world = World::new();
        let keycard = world.add_entity((
            PropFrobInfo {
                world_action: FrobFlag::MOVE,
                inventory_action: FrobFlag::empty(),
                tool_action: FrobFlag::empty(),
            },
            PropKeySrc(KeyCard {
                is_master: false,
                region_id: 128,
                lock_id: 0,
            }),
        ));

        assert!(matches!(
            held_trigger_payload(&world, keycard),
            MessagePayload::Frob
        ));
    }

    /// A wielded weapon draws its own first-person model at the hand, so the
    /// hand visual has to step aside - otherwise there is a hand inside the gun.
    #[test]
    fn wielded_weapon_replaces_the_hand_visual() {
        let mut world = World::new();
        let weapon = world.add_entity(player_gun());

        assert!(!shows_hand_visual(&world, Some(weapon)));
    }

    /// An empty hand, or one holding something with no first-person weapon
    /// model (a crate, a consumable), still shows the hand.
    #[test]
    fn empty_and_plain_held_hands_keep_the_hand_visual() {
        let mut world = World::new();
        let consumable = world.add_entity(scripted_inventory_use());

        assert!(shows_hand_visual(&world, None));
        assert!(shows_hand_visual(&world, Some(consumable)));
    }

    /// A scripted world pickup (here, a keycard - nanite piles are the
    /// motivating case) that also carries a `MOVE` frob action, so a test can
    /// tell the deliberate "route through Frob" branch apart from simply
    /// never being grabbable (`can_grab_item` alone would already be false
    /// without it).
    fn scripted_world_pickup_at(
        world: &mut World,
        physics: &mut PhysicsWorld,
        position: Vector3<f32>,
    ) -> EntityId {
        use dark::properties::{KeyCard, PropKeySrc};

        let entity = world.add_entity((
            PropKeySrc(KeyCard {
                is_master: false,
                region_id: 0,
                lock_id: 0,
            }),
            PropFrobInfo {
                world_action: FrobFlag::MOVE,
                inventory_action: FrobFlag::empty(),
                tool_action: FrobFlag::empty(),
            },
        ));
        register_kinematic_body(
            world,
            physics,
            entity,
            position,
            vec3(1.0, 1.0, 1.0),
            CollisionGroup::selectable(),
        );
        entity
    }

    fn scripted_world_pickup(world: &mut World, physics: &mut PhysicsWorld) -> EntityId {
        scripted_world_pickup_at(world, physics, vec3(0.0, 0.0, -0.5))
    }

    fn frob_message_count(effects: &[VirtualHandEffect]) -> usize {
        effects
            .iter()
            .filter(|effect| {
                matches!(
                    effect,
                    VirtualHandEffect::OutMessage {
                        message: Message {
                            payload: MessagePayload::Frob,
                            ..
                        }
                    }
                )
            })
            .count()
    }

    /// Negative-first regression: a scripted world pickup (here, a keycard -
    /// nanite piles are the motivating case) is not physically grabbable, so
    /// squeeze routes it through Frob (see `uses_scripted_world_frob`). The
    /// trigger-idle branch used to clear the frob latch unconditionally every
    /// frame, so a squeeze held across frames re-sent Frob every tick the
    /// entity survived instead of just once on the rising edge.
    #[test]
    fn held_squeeze_frobs_a_scripted_pickup_only_once() {
        let mut world = World::new();
        let mut physics = PhysicsWorld::new();
        scripted_world_pickup(&mut world, &mut physics);

        let mut input = Hand::default();
        input.squeeze_value = 1.0;

        let (hand, effects) = handle_empty_hand_state(
            Handedness::Right,
            Vector3::zero(),
            Quaternion::new(1.0, 0.0, 0.0, 0.0),
            None,
            AffordanceTracker::default(),
            0.0,
            &world,
            &physics,
            &input,
            None,
            None,
            Default::default(),
        );
        assert_eq!(frob_message_count(&effects), 1, "first frame should Frob");

        let (_, effects) = handle_empty_hand_state(
            Handedness::Right,
            Vector3::zero(),
            Quaternion::new(1.0, 0.0, 0.0, 0.0),
            hand.last_frobbed_entity,
            AffordanceTracker::default(),
            0.0,
            &world,
            &physics,
            &input,
            None,
            None,
            Default::default(),
        );
        assert_eq!(
            frob_message_count(&effects),
            0,
            "a held squeeze must not re-frob every frame"
        );
    }

    /// Negative-first regression: the once-per-hold latch above must be keyed
    /// by entity, not merely "something was frobbed". A latch keyed only on
    /// `is_none()` would block Frobbing a *second* pickup the hand sweeps
    /// onto while squeeze is still held from the first.
    #[test]
    fn held_squeeze_sweeping_onto_a_new_scripted_pickup_frobs_it_too() {
        let mut world = World::new();
        let mut physics = PhysicsWorld::new();
        let first = scripted_world_pickup_at(&mut world, &mut physics, vec3(0.0, 0.0, -0.5));
        let second = scripted_world_pickup_at(&mut world, &mut physics, vec3(1.0, 0.0, -0.5));

        let mut input = Hand::default();
        input.squeeze_value = 1.0;

        let (hand, effects) = handle_empty_hand_state(
            Handedness::Right,
            Vector3::zero(),
            Quaternion::new(1.0, 0.0, 0.0, 0.0),
            None,
            AffordanceTracker::default(),
            0.0,
            &world,
            &physics,
            &input,
            None,
            None,
            Default::default(),
        );
        assert_eq!(
            frob_message_count(&effects),
            1,
            "the first pickup should be Frobbed"
        );
        assert_eq!(hand.last_frobbed_entity, Some(first));

        // Squeeze never releases, but the hand sweeps sideways onto a second
        // pickup.
        let (_, effects) = handle_empty_hand_state(
            Handedness::Right,
            vec3(1.0, 0.0, 0.0),
            Quaternion::new(1.0, 0.0, 0.0, 0.0),
            hand.last_frobbed_entity,
            AffordanceTracker::default(),
            0.0,
            &world,
            &physics,
            &input,
            None,
            None,
            Default::default(),
        );
        let frobbed_second = effects.iter().any(|effect| {
            matches!(
                effect,
                VirtualHandEffect::OutMessage {
                    message: Message {
                        to,
                        payload: MessagePayload::Frob,
                    }
                } if *to == second
            )
        });
        assert!(
            frobbed_second,
            "a new target under the hand must be Frobbed even though squeeze never released, got {effects:?}"
        );
    }

    fn frob_fixture(distance: f32) -> (World, PhysicsWorld, EntityId) {
        frob_fixture_with(distance, FrobFlag::SCRIPT)
    }

    fn frob_fixture_with(distance: f32, world_action: FrobFlag) -> (World, PhysicsWorld, EntityId) {
        let mut world = World::new();
        let target = world.add_entity(PropFrobInfo {
            world_action,
            inventory_action: FrobFlag::empty(),
            tool_action: FrobFlag::empty(),
        });
        let mut physics = PhysicsWorld::new();
        physics.add_kinematic(
            target,
            vec3(0.0, 0.0, -distance),
            Quaternion::new(1.0, 0.0, 0.0, 0.0),
            vec3(0.0, 0.0, 0.0),
            vec3(0.2, 0.2, 0.2),
            CollisionGroup::selectable(),
            false,
        );
        let mut player = physics.create_player(
            vec3(100.0, 100.0, 100.0),
            EntityId::from_inner(1000).unwrap(),
        );
        physics.update(vec3(0.0, 0.0, 0.0), &mut player);
        (world, physics, target)
    }

    /// Retail `GAMEPARAM` authors `Frob Dist = 50`, which the original picker
    /// treats as squared SS2 units. After this engine's 2.5 world-scale divide,
    /// a surface farther than `sqrt(50) / 2.5` must not highlight or frob.
    #[test]
    fn vr_hand_only_frobs_within_retail_reach() {
        let input = Hand {
            trigger_value: 1.0,
            ..Hand::default()
        };
        let identity = Quaternion::new(1.0, 0.0, 0.0, 0.0);

        let (far_world, far_physics, _) = frob_fixture(4.0);
        let (far_hand, far_effects) = VirtualHand::update(
            &VirtualHand::new(Handedness::Right),
            &far_physics,
            &far_world,
            vec3(0.0, 0.0, 0.0),
            identity,
            &input,
            None,
            None,
            Default::default(),
        );
        assert_eq!(far_hand.get_raytraced_entity(), None);
        assert!(
            far_effects.is_empty(),
            "an out-of-reach VR trigger must not frob, got {far_effects:?}"
        );

        let (near_world, near_physics, near_target) = frob_fixture(2.5);
        let (near_hand, near_effects) = VirtualHand::update(
            &VirtualHand::new(Handedness::Right),
            &near_physics,
            &near_world,
            vec3(0.0, 0.0, 0.0),
            identity,
            &input,
            None,
            None,
            Default::default(),
        );
        assert_eq!(near_hand.get_raytraced_entity(), Some(near_target));
        assert!(
            near_effects.iter().any(|effect| matches!(
                effect,
                VirtualHandEffect::OutMessage {
                    message: Message {
                        to,
                        payload: MessagePayload::Frob,
                    },
                } if *to == near_target
            )),
            "an in-reach VR trigger should preserve frob behavior, got {near_effects:?}"
        );
    }

    /// The light and the pre-shape must resolve off the same target the input
    /// acts on: an authored pickup in reach reads as a grab, a scripted-frob
    /// target as a press, and an empty ray as nothing.
    #[test]
    fn hovering_resolves_one_affordance_per_hand() {
        let idle = Hand::default();
        let identity = Quaternion::new(1.0, 0.0, 0.0, 0.0);
        let hover = |distance: f32, world_action: FrobFlag| {
            let (world, physics, _) = frob_fixture_with(distance, world_action);
            VirtualHand::update(
                &VirtualHand::new(Handedness::Right),
                &physics,
                &world,
                vec3(0.0, 0.0, 0.0),
                identity,
                &idle,
                None,
                None,
                Default::default(),
            )
            .0
            .affordance()
        };

        assert_eq!(hover(2.5, FrobFlag::MOVE), HandAffordance::Grabbable);
        assert_eq!(hover(2.5, FrobFlag::SCRIPT), HandAffordance::Frobbable);
        assert_eq!(hover(4.0, FrobFlag::MOVE), HandAffordance::None);
    }

    /// A locked target is recognised but refused: amber, not green, and a frob
    /// against it reads as a failed attempt.
    #[test]
    fn a_locked_target_reads_as_blocked_then_failed() {
        let (mut world, physics, target) = frob_fixture_with(2.5, FrobFlag::SCRIPT);
        world.add_component(target, dark::properties::PropLocked(true));
        let identity = Quaternion::new(1.0, 0.0, 0.0, 0.0);

        let (hovering, _) = VirtualHand::update(
            &VirtualHand::new(Handedness::Right),
            &physics,
            &world,
            vec3(0.0, 0.0, 0.0),
            identity,
            &Hand::default(),
            None,
            None,
            Default::default(),
        );
        assert_eq!(hovering.affordance(), HandAffordance::Blocked);

        let (frobbing, _) = VirtualHand::update(
            &hovering,
            &physics,
            &world,
            vec3(0.0, 0.0, 0.0),
            identity,
            &Hand {
                trigger_value: 1.0,
                ..Hand::default()
            },
            None,
            None,
            Default::default(),
        );
        assert_eq!(frobbing.affordance(), HandAffordance::Failed);
    }

    /// A squeeze the world *accepted* must never read as a refusal, however
    /// long it is held: a scripted pickup is Frobbed once and the latch then
    /// suppresses further frobs, which is the same press, not a rejection.
    #[test]
    fn holding_a_squeeze_on_a_scripted_pickup_never_reads_as_failed() {
        let (world, physics, _) = frob_fixture_with(2.5, FrobFlag::SCRIPT);
        let squeeze = Hand {
            squeeze_value: 1.0,
            ..Hand::default()
        };
        let identity = Quaternion::new(1.0, 0.0, 0.0, 0.0);

        let mut hand = VirtualHand::new(Handedness::Right);
        for frame in 0..5 {
            hand = VirtualHand::update(
                &hand,
                &physics,
                &world,
                vec3(0.0, 0.0, 0.0),
                identity,
                &squeeze,
                None,
                None,
                Default::default(),
            )
            .0;
            assert_eq!(
                hand.affordance(),
                HandAffordance::Frobbable,
                "frame {frame} of an accepted squeeze must not flash a refusal"
            );
        }
    }

    /// A squeeze on something the hand cannot take is a refused attempt, and
    /// reads red rather than staying green on a grab that never happens.
    #[test]
    fn a_refused_grab_reads_as_a_failed_attempt() {
        let (world, physics, _) = frob_fixture_with(2.5, FrobFlag::empty());
        let squeeze = Hand {
            squeeze_value: 1.0,
            ..Hand::default()
        };
        let (hand, _) = VirtualHand::update(
            &VirtualHand::new(Handedness::Right),
            &physics,
            &world,
            vec3(0.0, 0.0, 0.0),
            Quaternion::new(1.0, 0.0, 0.0, 0.0),
            &squeeze,
            None,
            None,
            Default::default(),
        );
        assert_eq!(hand.affordance(), HandAffordance::Failed);
    }
}
