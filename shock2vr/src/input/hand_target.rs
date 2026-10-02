//! World-space hand targets for remote drivers (the debug runtime's
//! `<hand>_hand.world_target`): hold a VR hand on a world point while the body
//! moves, by re-solving its pawn-space controller pose every frame.

use cgmath::{InnerSpace, Quaternion, Rotation, Vector3, vec3};

use crate::{input_context::Hand, virtual_hand::hand_world_position, vr_config::Handedness};

/// How far a hand can reach from its shoulder.
pub const ARM_REACH_METERS: f32 = 0.7;
/// Eye to shoulder height, and half the shoulder width, of an adult.
const SHOULDER_DROP_METERS: f32 = 0.25;
const SHOULDER_HALF_WIDTH_METERS: f32 = 0.19;

/// The shoulder of `side` in pawn space: below the tracked head and beside its
/// horizontal facing, so turning the head turns the shoulders with it.
fn shoulder(
    side: Handedness,
    head_position: Vector3<f32>,
    head_rotation: Quaternion<f32>,
) -> Vector3<f32> {
    let unit = 1.0 / crate::METERS_PER_WORLD_UNIT;
    let forward = crate::ui::PanelPlacement::from_head(head_position, head_rotation).forward;
    let right = forward.cross(Vector3::unit_y());
    let lateral = match side {
        Handedness::Left => -right,
        Handedness::Right => right,
    };
    head_position - Vector3::unit_y() * (SHOULDER_DROP_METERS * unit)
        + lateral * (SHOULDER_HALF_WIDTH_METERS * unit)
}

/// Where the game places `hand`'s calibrated grip point in the world - the
/// point climbing and grabbing resolve against.
pub fn hand_world_point(
    hand: &Hand,
    pawn_position: Vector3<f32>,
    pawn_rotation: Quaternion<f32>,
    forward_cm: f32,
) -> Vector3<f32> {
    let calibrated =
        hand.position + crate::glove_fit::forward_translation(hand.rotation, forward_cm);
    hand_world_position(pawn_position, pawn_rotation, calibrated)
}

/// The raw pawn-space controller position that puts `hand`'s calibrated grip
/// point on world `target` (inverse of [`hand_world_point`]), or why the arm
/// cannot reach it from its shoulder. The shoulder hangs under the VR eye: the
/// tracked head clamped to `eye_cap` above the pawn centre (the tracking
/// stance's crown), so a crouch lowers the reach.
#[allow(clippy::too_many_arguments)]
pub fn resolve_world_target(
    side: Handedness,
    target: Vector3<f32>,
    hand: &Hand,
    head_position: Vector3<f32>,
    head_rotation: Quaternion<f32>,
    eye_cap: f32,
    pawn_position: Vector3<f32>,
    pawn_rotation: Quaternion<f32>,
    forward_cm: f32,
) -> Result<Vector3<f32>, String> {
    let eye = vec3(
        head_position.x,
        head_position.y.min(eye_cap),
        head_position.z,
    );
    let calibrated = pawn_rotation.invert().rotate_vector(target - pawn_position);
    let reach = (calibrated - shoulder(side, eye, head_rotation)).magnitude()
        * crate::METERS_PER_WORLD_UNIT;
    if !(reach <= ARM_REACH_METERS) {
        return Err(format!(
            "target is {reach:.2} m from the {} shoulder; arm reach is {ARM_REACH_METERS} m",
            match side {
                Handedness::Left => "left",
                Handedness::Right => "right",
            }
        ));
    }
    Ok(calibrated - crate::glove_fit::forward_translation(hand.rotation, forward_cm))
}

#[cfg(test)]
mod tests {
    use super::*;
    use cgmath::{Deg, Rotation3, vec3};

    fn hand() -> Hand {
        let mut hand = Hand::default();
        hand.rotation = Quaternion::from_angle_y(Deg(90.0));
        hand
    }

    fn head() -> (Vector3<f32>, Quaternion<f32>) {
        (
            vec3(0.0, crate::input_context::DEFAULT_HEAD_HEIGHT, 0.0),
            Quaternion::from_angle_y(Deg(0.0)),
        )
    }

    #[test]
    fn resolved_hand_lands_on_the_target_for_any_pawn_pose() {
        let (head_position, head_rotation) = head();
        let target = vec3(10.0, 5.4, -3.0);
        for (pawn_position, yaw) in [(vec3(10.0, 4.3, -2.6), 0.0), (vec3(9.8, 4.2, -2.8), 135.0)] {
            let pawn_rotation = Quaternion::from_angle_y(Deg(yaw));
            let mut hand = hand();
            hand.position = resolve_world_target(
                Handedness::Right,
                target,
                &hand,
                head_position,
                head_rotation,
                crate::physics::player_eye_cap_above_center(false),
                pawn_position,
                pawn_rotation,
                -15.0,
            )
            .unwrap();
            let world = hand_world_point(&hand, pawn_position, pawn_rotation, -15.0);
            assert!((world - target).magnitude() < 1e-4, "yaw {yaw}: {world:?}");
        }
    }

    #[test]
    fn a_target_beyond_arm_reach_is_rejected() {
        let (head_position, head_rotation) = head();
        let pawn_rotation = Quaternion::from_angle_y(Deg(0.0));
        let unit = 1.0 / crate::METERS_PER_WORLD_UNIT;
        let right_shoulder = shoulder(Handedness::Right, head_position, head_rotation);
        let reach = |meters: f32| {
            resolve_world_target(
                Handedness::Right,
                right_shoulder + vec3(0.0, 0.0, -meters * unit),
                &hand(),
                head_position,
                head_rotation,
                crate::physics::player_eye_cap_above_center(false),
                vec3(0.0, 0.0, 0.0),
                pawn_rotation,
                -15.0,
            )
        };
        assert!(reach(0.65).is_ok());
        let err = reach(0.75).unwrap_err();
        assert!(err.contains("reach"), "{err}");
    }

    #[test]
    fn a_crouched_eye_cap_lowers_the_shoulder() {
        let (head_position, head_rotation) = head();
        let cap = crate::physics::player_eye_cap_above_center(true);
        assert!(cap < head_position.y, "the crouched cap clamps this head");
        let unit = 1.0 / crate::METERS_PER_WORLD_UNIT;
        let rendered = vec3(head_position.x, cap, head_position.z);
        let reach = |from: Vector3<f32>| {
            resolve_world_target(
                Handedness::Right,
                shoulder(Handedness::Right, from, head_rotation) + vec3(0.0, 0.0, -0.65 * unit),
                &hand(),
                head_position,
                head_rotation,
                cap,
                vec3(0.0, 0.0, 0.0),
                Quaternion::from_angle_y(Deg(0.0)),
                -15.0,
            )
        };
        assert!(reach(rendered).is_ok(), "in reach of the lowered shoulder");
        assert!(
            reach(head_position).is_err(),
            "out of reach of the lowered shoulder"
        );
    }

    #[test]
    fn shoulders_sit_below_the_head_on_either_side_of_its_facing() {
        let (head_position, head_rotation) = head();
        let left = shoulder(Handedness::Left, head_position, head_rotation);
        let right = shoulder(Handedness::Right, head_position, head_rotation);
        assert!(left.y < head_position.y && (left.y - right.y).abs() < 1e-6);
        let forward = crate::ui::PanelPlacement::from_head(head_position, head_rotation).forward;
        assert!(forward.cross(Vector3::unit_y()).dot(right - left) > 0.0);
        let width = (right - left).magnitude() * crate::METERS_PER_WORLD_UNIT;
        assert!((width - 2.0 * SHOULDER_HALF_WIDTH_METERS).abs() < 1e-4);
    }
}
