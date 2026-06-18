//! Flatscreen first-person player controller.
//!
//! The dedicated flatscreen analog of `VirtualHand`: it wields a single weapon
//! as a first-person viewmodel and fires it on the trigger. It produces the
//! same `VirtualHandEffect`s the VR hands do (`HoldItem`, `SetPositionRotation`,
//! `OutMessage { TriggerPull/Release }`), so `mission_core` processes VR and
//! flat through one shared path, and the weapon-firing scripts fire unchanged.
//!
//! See `projects/flatscreen-and-vr-architecture.md` (Slice 5).

use cgmath::{Quaternion, Rotation, Vector3, vec3};
use shipyard::{EntityId, World};

use dark::SCALE_FACTOR;

use crate::{
    input_context::Hand,
    scripts::{Message, MessagePayload},
    virtual_hand::VirtualHandEffect,
    vr_config::{self, Handedness},
};

/// Where the wielded weapon sits relative to the camera, in look space (+x
/// right, +y up, -z forward), before the world-scale divide. Tunable framing.
const VIEWMODEL_OFFSET: Vector3<f32> = vec3(2.0, -2.5, -5.0);
/// Camera height above the player's feet position (world units before scale).
const HEAD_HEIGHT: f32 = 5.0;

pub struct FlatPlayerController {
    wielded_entity: Option<EntityId>,
    last_fire_pressed: bool,
}

impl FlatPlayerController {
    pub fn new() -> Self {
        Self {
            wielded_entity: None,
            last_fire_pressed: false,
        }
    }

    /// Wield `entity_id` as the first-person weapon: hold it (becomes
    /// non-physical and swaps to its hand model) and track it.
    pub fn wield(&mut self, entity_id: EntityId) -> Vec<VirtualHandEffect> {
        self.wielded_entity = Some(entity_id);
        self.last_fire_pressed = false;
        vec![VirtualHandEffect::HoldItem { entity_id }]
    }

    /// Per-frame: place the viewmodel in front of the camera and fire on the
    /// trigger's rising edge (release on the falling edge).
    pub fn update(
        &mut self,
        input: &Hand,
        player_pos: Vector3<f32>,
        player_rotation: Quaternion<f32>,
        head_rotation: Quaternion<f32>,
        world: &World,
    ) -> Vec<VirtualHandEffect> {
        let mut effects = Vec::new();

        let Some(entity_id) = self.wielded_entity else {
            self.last_fire_pressed = false;
            return effects;
        };

        // Camera/look transform. Position the weapon in front of the camera,
        // oriented to fire forward (the same orientation correction the VR hand
        // uses, so the firing scripts launch along the look direction).
        let look = player_rotation * head_rotation;
        let camera_pos = player_pos + vec3(0.0, HEAD_HEIGHT / SCALE_FACTOR, 0.0);
        let adjustments = vr_config::get_vr_hand_model_adjustments_from_entity(
            entity_id,
            world,
            Handedness::Right,
        );
        let position = camera_pos + look.rotate_vector(VIEWMODEL_OFFSET / SCALE_FACTOR);
        let rotation = look * adjustments.rotation;

        effects.push(VirtualHandEffect::SetPositionRotation {
            entity_id,
            position,
            rotation,
            scale: adjustments.scale,
        });

        // Fire on the trigger edge.
        let fire_pressed = input.trigger_value > 0.5;
        if fire_pressed && !self.last_fire_pressed {
            effects.push(VirtualHandEffect::OutMessage {
                message: Message {
                    to: entity_id,
                    payload: MessagePayload::TriggerPull,
                },
            });
        } else if !fire_pressed && self.last_fire_pressed {
            effects.push(VirtualHandEffect::OutMessage {
                message: Message {
                    to: entity_id,
                    payload: MessagePayload::TriggerRelease,
                },
            });
        }
        self.last_fire_pressed = fire_pressed;

        effects
    }
}

impl Default for FlatPlayerController {
    fn default() -> Self {
        Self::new()
    }
}
