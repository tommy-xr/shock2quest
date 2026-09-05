//! One translation for the entire tracked rig: lowering the eye for crouch
//! must lower the hands by the same amount, preserving physical reach and IPD.
use cgmath::{Vector3, vec3};

#[derive(Clone, Copy, Debug)]
pub struct TrackingTransform {
    translation: Vector3<f32>,
    head_stage_y: f32,
    stage_offset_meters: f32,
}

impl TrackingTransform {
    /// All heights except `head_stage_y` and `stage_offset_meters` are world units.
    /// Resolve once from the head centre, then apply identically to both eyes,
    /// both hands, and gameplay's head pose. Never clamp individual tracked parts.
    pub fn new(
        center_above_floor: f32,
        eye_cap: f32,
        head_stage_y: f32,
        stage_offset_meters: f32,
    ) -> Self {
        let scale = crate::METERS_PER_WORLD_UNIT;
        let offset = stage_offset_meters / scale;
        let head_y = head_stage_y / scale - center_above_floor;
        let lowering = (head_y - eye_cap).max(0.0);
        Self {
            translation: vec3(0.0, offset - center_above_floor - lowering, 0.0),
            head_stage_y,
            stage_offset_meters,
        }
    }

    pub fn with_stance(self, center_above_floor: f32, eye_cap: f32) -> Self {
        Self::new(
            center_above_floor,
            eye_cap,
            self.head_stage_y,
            self.stage_offset_meters,
        )
    }

    /// Apply a stance transition to an already-converted input, including any
    /// pawn-local debug overrides, without changing relative tracked geometry.
    pub fn rebase_input(input: &mut crate::input_context::InputContext, center: f32, cap: f32) {
        if let Some(old) = input.tracking {
            let new = old.with_stance(center, cap);
            let delta = new.translation - old.translation;
            input.head.position += delta;
            input.left_hand.position += delta;
            input.right_hand.position += delta;
            input.tracking = Some(new);
        }
    }

    pub fn stage_to_pawn(self, stage: Vector3<f32>) -> Vector3<f32> {
        stage / crate::METERS_PER_WORLD_UNIT + self.translation
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cgmath::InnerSpace;

    #[test]
    fn crouch_preserves_head_hand_geometry_and_stereo() {
        for head_height in [1.7, 1.2, 0.8] {
            for crouched in [false, true] {
                let head = vec3(0.0, head_height, 0.0);
                let hand = head + vec3(0.2, -0.15, -0.3);
                let rig = TrackingTransform::new(
                    crate::physics::player_center_above_floor(crouched),
                    crate::physics::player_eye_cap_above_center(crouched),
                    head_height,
                    0.0,
                );
                let eye = rig.stage_to_pawn(head);
                assert!(eye.y <= crate::physics::player_eye_cap_above_center(crouched) + 1e-5);
                assert!(
                    ((rig.stage_to_pawn(hand) - eye)
                        - (hand - head) / crate::METERS_PER_WORLD_UNIT)
                        .magnitude()
                        < 1e-5
                );
                let other_eye = rig.stage_to_pawn(head + vec3(0.064, 0.0, 0.0));
                assert!(
                    ((other_eye - eye).magnitude() - 0.064 / crate::METERS_PER_WORLD_UNIT).abs()
                        < 1e-5
                );
            }
        }
    }

    #[test]
    fn eye_height_setting_translates_whole_rig() {
        let base = TrackingTransform::new(0.6, 0.48, 1.6, 0.0);
        let raised = TrackingTransform::new(0.6, 0.48, 1.6, 0.1);
        for pose in [vec3(0.0, 1.6, 0.0), vec3(0.3, 1.3, -0.4)] {
            assert!(
                ((raised.stage_to_pawn(pose) - base.stage_to_pawn(pose)).y
                    - 0.1 / crate::METERS_PER_WORLD_UNIT)
                    .abs()
                    < 1e-5
            );
        }
    }

    #[test]
    fn stance_change_rebases_input_to_the_rendered_rig_on_the_same_frame() {
        let head = vec3(0.0, 1.65, 0.0);
        let hand = head + vec3(0.2, -0.15, -0.3);
        let initial = TrackingTransform::new(
            crate::physics::player_center_above_floor(false),
            crate::physics::player_eye_cap_above_center(false),
            head.y,
            0.0,
        );
        let mut input = crate::input_context::InputContext::default();
        input.tracking = Some(initial);
        input.head.position = initial.stage_to_pawn(head);
        input.right_hand.position = initial.stage_to_pawn(hand);
        for crouched in [true, false, true] {
            let center = crate::physics::player_center_above_floor(crouched);
            let cap = crate::physics::player_eye_cap_above_center(crouched);
            TrackingTransform::rebase_input(&mut input, center, cap);
            let rendered = initial.with_stance(center, cap);
            assert!((input.head.position - rendered.stage_to_pawn(head)).magnitude() < 1e-5);
            assert!((input.right_hand.position - rendered.stage_to_pawn(hand)).magnitude() < 1e-5);
            let once = input.right_hand.position;
            TrackingTransform::rebase_input(&mut input, center, cap);
            assert_eq!(input.right_hand.position, once);
        }
    }
}
