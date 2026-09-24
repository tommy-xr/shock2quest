//! One translation for the entire tracked rig: lowering the eye for crouch
//! must lower the hands by the same amount, preserving physical reach and IPD.
use cgmath::{InnerSpace, Vector3, Zero, vec3};
use shipyard::{Unique, UniqueView, World};

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

/// Opt-in horizontal roomscale spike. Tracking history is transient and belongs
/// to the mission, so a new mission/save establishes its own physical origin.
#[derive(Unique)]
pub struct RoomscaleState {
    previous_head: Option<Vector3<f32>>,
    offset: Vector3<f32>,
}

impl Default for RoomscaleState {
    fn default() -> Self {
        Self {
            previous_head: None,
            offset: Vector3::zero(),
        }
    }
}

impl RoomscaleState {
    /// Return a pawn-local horizontal movement request. Subtract the entire
    /// tracked delta from the rig, then let the ordinary character controller
    /// apply whatever travel is legal. The rejected part is never accumulated.
    pub fn sample(
        &mut self,
        input: &crate::input_context::InputContext,
        enabled: bool,
        advancing: bool,
        locomotion_allowed: bool,
    ) -> Vector3<f32> {
        if !enabled {
            *self = Self::default();
            return Vector3::zero();
        }
        // Debug idle updates are not simulation frames: do not consume a pose
        // before /v1/step has had a chance to move the character.
        if !advancing {
            return Vector3::zero();
        }
        let head = vec3(input.head.position.x, 0.0, input.head.position.z);
        if input.pose_tracking.is_some_and(|tracking| !tracking.head)
            || !head.x.is_finite()
            || !head.z.is_finite()
        {
            self.previous_head = None;
            return Vector3::zero();
        }
        if input.tracking_origin_changed {
            self.previous_head = None;
        }
        let previous = self.previous_head.replace(head);
        let Some(previous) = previous else {
            // Spawn/recovery uses the wearer's current position as the origin.
            self.offset = -head;
            return Vector3::zero();
        };
        let delta = head - previous;
        // Reject discontinuities, not ordinary walking speed. This is a spike
        // fallback; OpenXR reference-space changes also explicitly reset above.
        if delta.magnitude() > 0.5 / crate::METERS_PER_WORLD_UNIT {
            self.offset = -head;
            return Vector3::zero();
        }
        if !locomotion_allowed {
            // Keep live tracking while another system owns motion, without
            // replaying that physical travel when ordinary locomotion resumes.
            return Vector3::zero();
        }
        self.offset -= delta;
        delta
    }

    pub fn offset(world: &World) -> Vector3<f32> {
        world
            .borrow::<UniqueView<Self>>()
            .map_or(Vector3::zero(), |state| state.offset)
    }

    /// The same translation is consumed by input and the shared camera resolver.
    pub fn apply(input: &mut crate::input_context::InputContext, offset: Vector3<f32>) {
        input.head.position += offset;
        input.left_hand.position += offset;
        input.right_hand.position += offset;
    }

    /// A Game-level pause skips mission updates entirely. Reset only the delta
    /// history so movement during that interval cannot become a resumed step.
    pub fn invalidate(world: &World) {
        if let Ok(mut state) = world.borrow::<shipyard::UniqueViewMut<Self>>() {
            state.previous_head = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cgmath::InnerSpace;

    #[test]
    fn roomscale_counts_free_travel_once_and_preserves_rig_geometry() {
        let mut state = RoomscaleState::default();
        let mut input = crate::input_context::InputContext::default();
        input.head.position.x = 2.0;
        input.left_hand.position = input.head.position + vec3(-0.3, -0.2, 0.1);
        input.right_hand.position = input.head.position + vec3(0.3, -0.2, 0.1);
        assert_eq!(state.sample(&input, true, true, true), Vector3::zero());
        let mut body = Vector3::zero();
        for _ in 0..10 {
            input.head.position.x += 0.05;
            input.left_hand.position.x += 0.05;
            input.right_hand.position.x += 0.05;
            body += state.sample(&input, true, true, true);
            let mut resolved = input.clone();
            RoomscaleState::apply(&mut resolved, state.offset);
            assert!(resolved.head.position.x.abs() < 1e-5);
            for (raw, corrected) in [
                (input.left_hand.position, resolved.left_hand.position),
                (input.right_hand.position, resolved.right_hand.position),
            ] {
                assert!(
                    ((corrected - resolved.head.position) - (raw - input.head.position))
                        .magnitude()
                        < 1e-5
                );
            }
            assert_eq!(resolved.head.position.y, input.head.position.y);
        }
        assert!((body.x - 0.5).abs() < 1e-5);
        let eye = input.head.position + vec3(0.032, 0.0, 0.0);
        assert!(
            (((eye + state.offset) - (input.head.position + state.offset)).x - 0.032).abs() < 1e-5
        );
    }

    #[test]
    fn roomscale_does_not_replay_blocked_motion_or_consume_debug_idle_samples() {
        let mut state = RoomscaleState::default();
        let mut input = crate::input_context::InputContext::default();
        state.sample(&input, true, true, true);
        input.head.position.x = 0.1;
        assert_eq!(state.sample(&input, true, false, true), Vector3::zero());
        assert_eq!(state.sample(&input, true, true, true).x, 0.1);
        // The controller may reject that entire request. It must never recur.
        assert_eq!(state.sample(&input, true, true, true), Vector3::zero());
        input.head.position.x = 0.05;
        assert_eq!(state.sample(&input, true, true, true).x, -0.05);
    }

    #[test]
    fn roomscale_resets_discontinuities_and_never_converts_crouch_into_travel() {
        let mut state = RoomscaleState::default();
        let mut input = crate::input_context::InputContext::default();
        state.sample(&input, true, true, true);
        input.head.position.y -= 0.5;
        assert_eq!(state.sample(&input, true, true, true), Vector3::zero());
        input.head.position.x = 10.0;
        assert_eq!(state.sample(&input, true, true, true), Vector3::zero());
        input.head.position.x = 10.1;
        input.tracking_origin_changed = true;
        assert_eq!(state.sample(&input, true, true, true), Vector3::zero());
        input.tracking_origin_changed = false;
        input.pose_tracking = Some(crate::input_context::PoseTracking {
            head: false,
            hands: [true; 2],
        });
        state.sample(&input, true, true, true);
        input.pose_tracking = None;
        input.head.position.x = 10.2;
        assert_eq!(state.sample(&input, true, true, true), Vector3::zero());
        input.head.position.x = 10.3;
        assert_eq!(state.sample(&input, true, true, false), Vector3::zero());
        assert_eq!(state.sample(&input, true, true, true), Vector3::zero());
        assert_eq!(state.sample(&input, false, true, true), Vector3::zero());
        assert_eq!(state.offset, Vector3::zero());
    }

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
