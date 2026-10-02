//! Off-hand slide contact. Unlike a support grip, it never steers the receiver.
use super::*;
use cgmath::Transform;
use shipyard::{Get, View};

use crate::runtime_props::{
    RuntimePropGloveWeapon, RuntimePropJointTransforms, RuntimePropObjectArticulation,
};
use crate::vr_weapon_action::PISTOL_SLIDE_MOTION as MOTION;

pub(super) struct SlideAttachment {
    entity: EntityId,
    primary: usize,
    // Preserve where on the contact the player grabbed; no jump on acquisition.
    offset: Vector3<f32>,
    controller_model: GripPose,
}

pub(super) struct SlideContact {
    entity: EntityId,
    primary: usize,
    anchor: Vector3<f32>,
    travel: Vector3<f32>,
    fraction: f32,
    model: GripPose,
    hand: GripPose,
}

fn profile() -> SupportProfile {
    SupportProfile {
        // Rear/top of Nightdive @s00_sli: bounds X -0.666..-0.091,
        // Y 0.103..0.189. Coordinates precede the fitted weapon scale.
        palm_anchor: [-0.16, 0.19, 0.016],
        region: None,
        motion: None,
        rotation_degrees: [0.0, 0.0, -90.0],
        curls: [0.4, 0.45, 0.5, 0.55, 0.6],
        trigger_curls: None,
        grab_radius: 0.045,
        release_distance: 0.1,
        max_swing_degrees: 75.0,
    }
}

impl VrInteraction {
    fn slide_contact(&self, world: &World, poses: [GripPose; 2]) -> Option<SlideContact> {
        let rigs = self.grip_kinematics.as_ref()?;
        for primary in 0..2 {
            let Some(held) = &self.fitted_grips[primary] else {
                continue;
            };
            let hands = [&self.left_hand, &self.right_hand];
            if held.model != "atek_h" || hands[primary].get_held_entity() != Some(held.entity) {
                continue;
            }
            world
                .borrow::<View<RuntimePropGloveWeapon>>()
                .ok()?
                .get(held.entity)
                .ok()?;
            let objects = world.borrow::<View<RuntimePropObjectArticulation>>().ok()?;
            let object = &objects.get(held.entity).ok()?.0;
            let travel = MOTION.travel(object)?;
            let joint = object
                .joints
                .iter()
                .find(|j| j.parameter == MOTION.parameter && j.motion_type == 2)?
                .index as usize;
            let transforms = world.borrow::<View<RuntimePropJointTransforms>>().ok()?;
            let closed = object.pose(&[(MOTION.parameter, MOTION.closed)])[joint]
                .w
                .truncate();
            let current = transforms.get(held.entity).ok()?.0[joint].w.truncate();
            let fraction = ((current - closed).dot(travel) / travel.magnitude2()).clamp(0.0, 1.0);
            let grip = held.resolved.as_ref()?;
            let mirror = held.model_mirror?;
            let handedness = if primary == 0 {
                Handedness::Left
            } else {
                Handedness::Right
            };
            let travel = if primary == 0 {
                mirror.transform_vector(travel)
            } else {
                travel
            } * grip.item_scale;
            let profile = profile();
            let anchor = profile.anchor_in_frame(handedness, mirror) * grip.item_scale;
            let pose = poses[primary];
            let rotation = pose.rotation.normalize() * grip.rotation;
            let model = held.physical_model_pose(world).unwrap_or(GripPose {
                position: pose.point(rigs[primary].palm)
                    - rotation.rotate_vector(held.primary_anchor(rigs[primary].palm)?),
                rotation,
            });
            let hand = profile.glove_pose(
                handedness,
                model,
                grip,
                &rigs[1 - primary],
                anchor + travel * fraction,
            );
            return Some(SlideContact {
                entity: held.entity,
                primary,
                anchor,
                travel,
                fraction,
                model,
                hand,
            });
        }
        None
    }

    pub(super) fn update_slide(&mut self, ctx: &InteractionContext) -> Vec<VirtualHandEffect> {
        let inputs = [&ctx.input.left_hand, &ctx.input.right_hand];
        let poses = inputs.map(|input| GripPose {
            position: hand_world_position(ctx.player_pos, ctx.player_rotation, input.position),
            rotation: ctx.player_rotation * input.rotation,
        });
        if self
            .slide_reserved
            .is_some_and(|i| inputs[i].squeeze_value < 0.5 && inputs[i].trigger_value < 0.5)
        {
            self.slide_reserved = None;
        }
        let candidate = self.slide_contact(ctx.world, poses);
        let Some(c) = candidate else {
            self.slide = None;
            return Vec::new();
        };
        let other = 1 - c.primary;
        let available = ctx.support_enabled
            && ctx
                .input
                .pose_tracking
                .is_none_or(|p| p.head && p.hands.iter().all(|v| *v))
            && poses.iter().all(|p| p.is_tracked())
            && !ctx.reserved_releases.contains(&c.entity)
            && !self.body_tool_hands[other]
            && [&self.left_hand, &self.right_hand][other]
                .get_held_entity()
                .is_none()
            && !self
                .hand_climb
                .grips()
                .any(|(h, _)| crate::vr_config::hand_slot(h) == other)
            && inputs[c.primary].squeeze_value >= 0.5
            && inputs[other].squeeze_value >= 0.5;
        let palm = poses[other].point(self.grip_kinematics.as_ref().unwrap()[other].palm);
        let local = c
            .model
            .rotation
            .conjugate()
            .rotate_vector(palm - c.model.position);
        let profile = profile();
        if self
            .slide
            .as_ref()
            .is_some_and(|s| s.entity != c.entity || s.primary != c.primary)
            || !available
        {
            self.slide = None;
        }
        if self.slide.is_none()
            && available
            && self.support.as_ref().is_none_or(|s| !s.active)
            && !self.support_pressed[other]
            && !self.support_blocked[other]
            && (local - c.anchor - c.travel * c.fraction).magnitude() <= profile.grab_radius
        {
            self.slide = Some(SlideAttachment {
                entity: c.entity,
                primary: c.primary,
                offset: local - c.anchor - c.travel * c.fraction,
                controller_model: GripPose {
                    position: poses[c.primary]
                        .rotation
                        .conjugate()
                        .rotate_vector(c.model.position - poses[c.primary].position),
                    rotation: poses[c.primary].rotation.conjugate() * c.model.rotation,
                },
            });
            self.slide_reserved = Some(other);
            self.support = None;
            self.support_blocked[other] = true;
        }
        let Some(attachment) = &self.slide else {
            return Vec::new();
        };
        // Freeze the grab-time collision offset in controller space: moving both
        // hands together or a weapon recovering from a wall is not a slide pull.
        let control = GripPose {
            position: poses[c.primary].point(attachment.controller_model.position),
            rotation: poses[c.primary].rotation * attachment.controller_model.rotation,
        };
        let local = control
            .rotation
            .conjugate()
            .rotate_vector(palm - control.position);
        let delta = local - c.anchor - attachment.offset;
        let fraction = (delta.dot(c.travel) / c.travel.magnitude2()).clamp(0.0, 1.0);
        if (delta - c.travel * fraction).magnitude() > profile.release_distance {
            self.slide = None;
            return Vec::new();
        }
        // Paused redraws consume edges and releases but cannot move a part.
        if !ctx.step_dt.is_finite() || ctx.step_dt <= 0.0 {
            return Vec::new();
        }
        vec![VirtualHandEffect::MoveSlide {
            entity_id: c.entity,
            fraction,
        }]
    }

    pub(super) fn synchronize_slide(&mut self, world: &World) {
        self.slide_preview = self.slide_contact(world, self.hand_poses());
        if let (Some(s), Some(c)) = (&self.slide, &self.slide_preview) {
            if s.entity == c.entity && s.primary == c.primary {
                self.visual_hands[1 - c.primary] = Some(c.hand);
            } else {
                self.slide = None;
            }
        } else {
            self.slide = None;
        }
    }

    pub(super) fn slide_grip(&self) -> Option<(usize, crate::vr_grip::ResolvedGrip)> {
        let s = self.slide.as_ref()?;
        let mut grip = self.fitted_grips[s.primary].as_ref()?.resolved.clone()?;
        grip.curls = profile().curls;
        grip.trigger_curls = None;
        Some((1 - s.primary, grip))
    }

    pub(super) fn slide_diagnostics(&self, entity: EntityId) -> Option<serde_json::Value> {
        let c = self.slide_preview.as_ref().filter(|c| c.entity == entity)?;
        Some(serde_json::json!({
            "attached": self.slide.as_ref().is_some_and(|s| s.entity == c.entity),
            "fraction": c.fraction,
            "glove_pose": self.visual_hands[1-c.primary],
            "controller_position": c.hand.position - crate::glove_fit::forward_translation(
                c.hand.rotation, crate::dev_params::get(crate::dev_params::GLOVE_FORWARD_CM)),
            "controller_rotation": c.hand.rotation,
            "world_travel": c.model.rotation.rotate_vector(c.travel),
        }))
    }
}
