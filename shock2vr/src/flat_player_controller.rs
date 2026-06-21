//! Flatscreen first-person player controller.
//!
//! The dedicated flatscreen analog of `VirtualHand`: it wields a single weapon
//! as a first-person viewmodel, fires it on the trigger, and uses/frobs/picks
//! up the object under the crosshair. It produces the same `VirtualHandEffect`s
//! the VR hands do (`HoldItem`, `DropItem`, `SetPositionRotation`,
//! `OutMessage { TriggerPull/Release/Frob }`), so `mission_core` processes VR
//! and flat through one shared path, and the weapon/frob scripts run unchanged.
//!
//! See `projects/flatscreen-and-vr-architecture.md` (Slices 5-6).

use cgmath::{Deg, Quaternion, Rotation, Rotation3, Vector3, point3, vec3};
use shipyard::{EntityId, Get, View, World};

use dark::{SCALE_FACTOR, properties::PropFrobInfo};

use crate::{
    input_context::Hand,
    physics::{InternalCollisionGroups, PhysicsWorld},
    scripts::{Message, MessagePayload},
    util::resolve_proxy_entity,
    virtual_hand::{VirtualHandEffect, can_grab_item},
};

/// Shared viewmodel framing offset (look space: +x right, +y up, -z forward),
/// before the world-scale divide. Used for all wielded items for now; per-weapon
/// `PropPlayerGun.model_offset` placement is a TODO.
const VIEWMODEL_OFFSET: Vector3<f32> = vec3(2.0, -2.5, -5.0);
/// Camera height above the player's feet position (world units before scale).
const HEAD_HEIGHT: f32 = 5.0;

/// Base yaw applied to every first-person gun model before its per-weapon
/// `PropPlayerGun.heading`. The pistol (`atek_h`, heading 0) renders correctly
/// at -90deg; `heading` then corrects models authored at other angles (e.g. the
/// shotgun's 90deg). Tuned against the FP models, NOT the VR hand-model table.
const VIEWMODEL_BASE_YAW_DEG: f32 = -90.0;

pub struct FlatPlayerController {
    wielded_entity: Option<EntityId>,
    last_fire_pressed: bool,
    last_use_pressed: bool,
}

impl FlatPlayerController {
    pub fn new() -> Self {
        Self {
            wielded_entity: None,
            last_fire_pressed: false,
            last_use_pressed: false,
        }
    }

    pub fn is_wielding(&self) -> bool {
        self.wielded_entity.is_some()
    }

    pub fn wielded_entity(&self) -> Option<EntityId> {
        self.wielded_entity
    }

    /// Stop wielding `entity_id` if it was the held weapon (e.g. it was
    /// destroyed).
    pub fn on_entity_destroyed(&mut self, entity_id: EntityId) {
        if self.wielded_entity == Some(entity_id) {
            self.wielded_entity = None;
        }
    }

    /// If the wielded weapon was recreated as a new entity, track the new id.
    pub fn replace_wielded(&mut self, old: EntityId, new: EntityId) {
        if self.wielded_entity == Some(old) {
            self.wielded_entity = Some(new);
        }
    }

    /// Wield `entity_id` as the first-person weapon. Any previously-wielded
    /// weapon is dropped back into the world (regains physics + world model).
    pub fn wield(&mut self, entity_id: EntityId) -> Vec<VirtualHandEffect> {
        let mut effects = Vec::new();
        if let Some(prev) = self.wielded_entity {
            if prev != entity_id {
                effects.push(VirtualHandEffect::DropItem { entity_id: prev });
            }
        }
        self.wielded_entity = Some(entity_id);
        self.last_fire_pressed = false;
        effects.push(VirtualHandEffect::HoldItem { entity_id });
        effects
    }

    /// Per-frame update. Returns the effects to apply plus the entity currently
    /// under the crosshair (for highlight rendering), if any.
    pub fn update(
        &mut self,
        input: &Hand,
        player_pos: Vector3<f32>,
        player_rotation: Quaternion<f32>,
        head_rotation: Quaternion<f32>,
        world: &World,
        physics: &PhysicsWorld,
    ) -> (Vec<VirtualHandEffect>, Option<EntityId>) {
        let mut effects = Vec::new();

        let look = player_rotation * head_rotation;
        let camera_pos = player_pos + vec3(0.0, HEAD_HEIGHT / SCALE_FACTOR, 0.0);

        // Crosshair raycast: the frobbable entity under the reticle (resolving
        // hitbox proxies to their parent, and ignoring the weapon we hold).
        let forward = look.rotate_vector(vec3(0.0, 0.0, -1.0));
        let highlighted = physics
            .ray_cast(
                point3(camera_pos.x, camera_pos.y, camera_pos.z),
                forward,
                InternalCollisionGroups::ENTITY
                    | InternalCollisionGroups::SELECTABLE
                    | InternalCollisionGroups::WORLD
                    | InternalCollisionGroups::UI
                    | InternalCollisionGroups::RAYCAST,
            )
            .and_then(|r| r.maybe_entity_id)
            .map(|e| resolve_proxy_entity(world, e))
            .filter(|e| Some(*e) != self.wielded_entity && is_frobbable(world, *e));

        // Place the viewmodel + fire on the trigger edge.
        if let Some(entity_id) = self.wielded_entity {
            // First-person framing from the weapon's native PropPlayerGun
            // (model_offset + heading), falling back to a fixed offset for
            // non-gun pickups. This intentionally ignores the VR hand-model
            // table, which is keyed inconsistently and drops per-weapon heading.
            // The first-person `hand_model` meshes all share one orientation,
            // corrected by a single base yaw. NB: PropPlayerGun.heading is NOT
            // the FP model's rotation - applying it over-rotates exactly by its
            // value (the shotgun/assault 90deg, the psi-amp 180deg), so it is
            // deliberately not used here. Position uses a shared framing offset
            // for now (per-weapon model_offset placement is a TODO).
            let rotation = look * Quaternion::from_angle_y(Deg(VIEWMODEL_BASE_YAW_DEG));
            effects.push(VirtualHandEffect::SetPositionRotation {
                entity_id,
                position: camera_pos + look.rotate_vector(VIEWMODEL_OFFSET / SCALE_FACTOR),
                rotation,
                scale: vec3(1.0, 1.0, 1.0),
            });

            let fire_pressed = input.trigger_value > 0.5;
            if fire_pressed && !self.last_fire_pressed {
                effects.push(out_message(entity_id, MessagePayload::TriggerPull));
            } else if !fire_pressed && self.last_fire_pressed {
                effects.push(out_message(entity_id, MessagePayload::TriggerRelease));
            }
            self.last_fire_pressed = fire_pressed;
        } else {
            self.last_fire_pressed = false;
        }

        // Use / frob / pickup on the use-button (squeeze) rising edge.
        let use_pressed = input.squeeze_value > 0.5;
        if use_pressed && !self.last_use_pressed {
            if let Some(target) = highlighted {
                if can_grab_item(world, target) {
                    // A pickup-able object (e.g. a weapon): wield it.
                    effects.extend(self.wield(target));
                } else {
                    // Otherwise interact with it.
                    effects.push(out_message(target, MessagePayload::Frob));
                }
            }
        }
        self.last_use_pressed = use_pressed;

        (effects, highlighted)
    }
}

impl Default for FlatPlayerController {
    fn default() -> Self {
        Self::new()
    }
}

fn out_message(to: EntityId, payload: MessagePayload) -> VirtualHandEffect {
    VirtualHandEffect::OutMessage {
        message: Message { to, payload },
    }
}

/// Whether an entity is worth highlighting / interacting with (it has frob
/// info), which excludes plain world geometry the ray also hits.
fn is_frobbable(world: &World, entity_id: EntityId) -> bool {
    world
        .borrow::<View<PropFrobInfo>>()
        .map(|v| v.get(entity_id).is_ok())
        .unwrap_or(false)
}
