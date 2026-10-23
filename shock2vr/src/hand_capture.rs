//! Tracked-hand capture: OpenXR `XR_EXT_hand_tracking` joint positions to the
//! glove's finger bones, for authoring grips with real hands.
//!
//! The 26 OpenXR joints (palm, wrist, thumb metacarpal..tip, index..little
//! metacarpal..tip) share indices 1..=25 with the SteamVR skeleton the glove
//! poses use (index 0 is the palm there, the root here). Only joint positions
//! are used: each bone is swung from the open bind pose onto the captured
//! direction to its child joint, which needs no knowledge of either rig's
//! joint frames but drops bone roll (the thumb's opposition twist).
use cgmath::{InnerSpace, Matrix3, Quaternion, Rotation, Vector3};

use crate::{
    hand_pose::{FingerBones, Pose, open_right_hand},
    vr_config::Handedness,
};

pub const HAND_JOINT_COUNT: usize = 26;

/// OpenXR joint positions, indexed like `XrHandJointEXT`.
pub type HandJoints = [Vector3<f32>; HAND_JOINT_COUNT];

/// Metacarpal bases (index..little): rigid relative to the wrist, unlike the
/// knuckles, which move as the palm cups.
const METACARPALS: [usize; 4] = [6, 11, 16, 21];
/// Fingertip joints, thumb to little.
pub(crate) const TIPS: [usize; 5] = [5, 10, 15, 20, 25];

fn parent(bone: usize) -> usize {
    match bone {
        1 => 0,
        2 | 6 | 11 | 16 | 21 => 1,
        bone => bone - 1,
    }
}

/// `v` normalized, or `None` for a collapsed or non-finite vector, as
/// untracked joints report.
fn direction(v: Vector3<f32>) -> Option<Vector3<f32>> {
    let length = v.magnitude();
    (length.is_finite() && length > 1e-4).then(|| v / length)
}

/// A hand's anatomical frame from rigid, long segments only: origin at the
/// metacarpal-base centroid, +X from the little toward the index base, +Z
/// the palm normal (X × the middle metacarpal). Finger curl, palm cupping and
/// where a runtime puts its wrist joint do not move it.
pub fn hand_frame(joints: &HandJoints) -> Option<(Vector3<f32>, Quaternion<f32>)> {
    let origin = METACARPALS.iter().map(|&j| joints[j]).sum::<Vector3<f32>>() / 4.0;
    let x = direction(joints[6] - joints[21])?;
    let z = direction(x.cross(joints[12] - joints[11]))?;
    let y = z.cross(x);
    Some((origin, Quaternion::from(Matrix3::from_cols(x, y, z))))
}

/// The pose data is a right hand in left-handed (Unity) numbers. A real right
/// hand's right-handed numbers mirror into that; a left hand's already are a
/// mirrored right hand. Its own inverse.
fn to_pose_numbers(p: Vector3<f32>, hand: Handedness) -> Vector3<f32> {
    match hand {
        Handedness::Right => Vector3::new(-p.x, p.y, p.z),
        Handedness::Left => p,
    }
}

/// World positions and rotations of a pose's bones 0..=25 (SteamVR numbers).
fn forward_kinematics(pose: &Pose) -> (HandJoints, [Quaternion<f32>; HAND_JOINT_COUNT]) {
    let mut positions = [pose.bone_positions[0]; HAND_JOINT_COUNT];
    let mut rotations = [pose.bone_rotations[0]; HAND_JOINT_COUNT];
    for bone in 1..HAND_JOINT_COUNT {
        let p = parent(bone);
        positions[bone] = positions[p] + rotations[p].rotate_vector(pose.bone_positions[bone]);
        rotations[bone] = rotations[p] * pose.bone_rotations[bone];
    }
    (positions, rotations)
}

/// Finger bones that reproduce a tracked hand's joint directions, or `None`
/// when the joints are degenerate. Either hand produces right-hand bones; the
/// glove mirrors them for the left.
pub fn capture_finger_bones(joints: &HandJoints, hand: Handedness) -> Option<FingerBones> {
    let joints = joints.map(|p| to_pose_numbers(p, hand));
    let bind = open_right_hand();
    let (bind_positions, bind_rotations) = forward_kinematics(&bind);
    // Captured directions, rotated into the bind skeleton's hand frame.
    let align = hand_frame(&bind_positions)?.1 * hand_frame(&joints)?.1.invert();
    let mut world = bind_rotations;
    let mut pose = bind.clone();
    for bone in 2..HAND_JOINT_COUNT {
        let p = parent(bone);
        world[bone] = world[p] * bind.bone_rotations[bone];
        if TIPS.contains(&bone) {
            continue;
        }
        let child = bone + 1;
        let current = direction(world[bone].rotate_vector(bind.bone_positions[child]))?;
        let target = direction(align.rotate_vector(joints[child] - joints[bone]))?;
        world[bone] = (Quaternion::between_vectors(current, target) * world[bone]).normalize();
        pose.bone_rotations[bone] = (world[p].invert() * world[bone]).normalize();
    }
    Some(pose.finger_bones())
}

/// The held item's pose in controller-grip space (a grip's offset and
/// rotation) that puts the glove where the tracked hand held it, or `None`
/// for degenerate joints.
///
/// `tracked` and `glove` must be in the same units (world units), and `glove`
/// is the glove's joints in controller-grip space as drawn for that hand, so
/// mirrored for the left. The item keeps the tracked hand's distance from the
/// palm, so a hand much larger than the glove holds it slightly off the skin.
pub fn grip_transform(
    tracked: &HandJoints,
    glove: &HandJoints,
    item_position: Vector3<f32>,
    item_rotation: Quaternion<f32>,
) -> Option<(Vector3<f32>, Quaternion<f32>)> {
    let (hand_position, hand_rotation) = hand_pose(tracked, glove)?;
    let to_hand = hand_rotation.invert();
    Some((
        to_hand.rotate_vector(item_position - hand_position),
        (to_hand * item_rotation).normalize(),
    ))
}

/// The controller-grip pose that draws the glove (`glove`, its joints in
/// controller-grip space) over a tracked hand, or `None` for degenerate joints.
pub fn hand_pose(
    tracked: &HandJoints,
    glove: &HandJoints,
) -> Option<(Vector3<f32>, Quaternion<f32>)> {
    let (tracked_origin, tracked_rotation) = hand_frame(tracked)?;
    let (glove_origin, glove_rotation) = hand_frame(glove)?;
    let rotation = (tracked_rotation * glove_rotation.invert()).normalize();
    Some((
        tracked_origin - rotation.rotate_vector(glove_origin),
        rotation,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hand_pose::{FingerAmounts, fist_right_hand};
    use cgmath::{Deg, Rotation3, vec3};

    fn placement() -> (Vector3<f32>, Quaternion<f32>) {
        (
            vec3(0.4, 1.2, -0.3),
            Quaternion::from_axis_angle(vec3(0.3, 1.0, -0.2).normalize(), Deg(70.0)),
        )
    }

    /// A pose's joints as tracking would report them for `hand`, placed
    /// somewhere arbitrary in the tracking space.
    fn tracked(pose: &Pose, hand: Handedness) -> HandJoints {
        let (position, rotation) = placement();
        forward_kinematics(pose)
            .0
            .map(|p| position + rotation.rotate_vector(to_pose_numbers(p, hand)))
    }

    /// Joint positions in the hand's own frame, so captures compare across placements.
    fn in_hand_frame(joints: &HandJoints) -> HandJoints {
        let (origin, rotation) = hand_frame(joints).unwrap();
        joints.map(|p| rotation.invert().rotate_vector(p - origin))
    }

    /// Largest angle between a captured bone and the expected bone, measured
    /// as each bone's direction to its child in the hand frame.
    fn max_bone_angle(captured: &FingerBones, expected: &Pose) -> Deg<f32> {
        let a = in_hand_frame(&forward_kinematics(&Pose::from_finger_bones(captured)).0);
        let b = in_hand_frame(&forward_kinematics(expected).0);
        (2..HAND_JOINT_COUNT)
            .filter(|bone| !TIPS.contains(bone))
            .map(|bone| Deg::from((a[bone + 1] - a[bone]).angle(b[bone + 1] - b[bone])))
            .fold(Deg(0.0), |m, d| if d > m { d } else { m })
    }

    #[test]
    fn open_hand_captures_the_bind_pose() {
        let bind = open_right_hand().finger_bones();
        for hand in [Handedness::Left, Handedness::Right] {
            let captured = capture_finger_bones(&tracked(&open_right_hand(), hand), hand).unwrap();
            for (bone, (a, b)) in captured.iter().zip(&bind).enumerate() {
                assert!(a.dot(*b).abs() > 1.0 - 1e-5, "{hand:?} bone {bone}");
            }
        }
    }

    #[test]
    fn curled_hands_reproduce_their_bone_directions() {
        // A real hand keeps its bone lengths; the authored poses do not quite
        // (the fist moves the metacarpal bases ~5 mm), so test on the bind
        // skeleton posed with each pose's rotations.
        let open = open_right_hand();
        let rigid = |pose: Pose| Pose {
            bone_positions: open.bone_positions.clone(),
            ..pose
        };
        let fist = rigid(fist_right_hand());
        let partial = rigid(open.blend_per_finger(
            &fist,
            &FingerAmounts {
                thumb: -0.5,
                index: 0.2,
                middle: 0.6,
                ring: 1.0,
                pinky: 0.4,
            },
        ));
        for pose in [&fist, &partial] {
            for hand in [Handedness::Left, Handedness::Right] {
                let captured = capture_finger_bones(&tracked(pose, hand), hand).unwrap();
                let error = max_bone_angle(&captured, pose);
                assert!(error < Deg(0.1), "{hand:?}: {error:?}");
            }
        }
        // Negative control: the open hand is far from the fist.
        assert!(max_bone_angle(&open.finger_bones(), &fist) > Deg(30.0));
    }

    /// A right hand as OpenXR reports one, built by hand rather than from the
    /// pose data: palm down (-Y), fingers forward (-Z), thumb on the left (-X).
    /// `index_curl` folds the index finger's joints down toward the palm side.
    fn openxr_right_hand(index_curl: f32) -> HandJoints {
        let mut joints = [Vector3::new(0.0, 0.0, 0.0); HAND_JOINT_COUNT];
        joints[0] = vec3(0.0, 0.0, -0.05);
        joints[1] = vec3(0.0, 0.0, 0.0);
        joints[2..6].copy_from_slice(&[
            vec3(-0.02, -0.01, -0.02),
            vec3(-0.045, -0.015, -0.045),
            vec3(-0.06, -0.015, -0.07),
            vec3(-0.07, -0.015, -0.09),
        ]);
        for (finger, x) in [-0.025, -0.008, 0.008, 0.024].into_iter().enumerate() {
            let base = 6 + finger * 5;
            let curl = if finger == 0 { index_curl } else { 0.0 };
            joints[base] = vec3(x, 0.0, -0.02);
            joints[base + 1] = vec3(x, 0.0, -0.09);
            // Each later segment pitches a further `curl` toward the palm.
            let mut position = joints[base + 1];
            for (step, length) in [0.04, 0.025, 0.02].into_iter().enumerate() {
                let angle = curl * (step + 1) as f32;
                position += vec3(0.0, -angle.sin(), -angle.cos()) * length;
                joints[base + 2 + step] = position;
            }
        }
        joints
    }

    /// How far the index proximal bone points along the palm normal (a unit
    /// direction's z), for a capture redrawn on the rig.
    fn index_pitch(bones: &FingerBones) -> f32 {
        let joints = in_hand_frame(&forward_kinematics(&Pose::from_finger_bones(bones)).0);
        (joints[8] - joints[7]).normalize().z
    }

    #[test]
    fn a_real_right_hand_curls_the_glove_the_way_the_fist_does() {
        let fist_direction = index_pitch(&fist_right_hand().finger_bones())
            - index_pitch(&open_right_hand().finger_bones());
        let curl = |hand| {
            let open = capture_finger_bones(&openxr_right_hand(0.0), hand).unwrap();
            let curled = capture_finger_bones(&openxr_right_hand(0.8), hand).unwrap();
            index_pitch(&curled) - index_pitch(&open)
        };
        let right = curl(Handedness::Right);
        assert!(
            right * fist_direction > 0.0 && right.abs() > 0.5,
            "{right} vs {fist_direction}"
        );
        // Captured as the wrong hand, the finger bends backward.
        assert!(curl(Handedness::Left) * fist_direction < 0.0);
    }

    #[test]
    fn the_runtime_wrist_joint_does_not_tilt_the_capture() {
        let joints = tracked(&fist_right_hand(), Handedness::Left);
        let mut moved = joints;
        moved[1] += vec3(0.0, 0.005, 0.003);
        assert_eq!(
            capture_finger_bones(&joints, Handedness::Left),
            capture_finger_bones(&moved, Handedness::Left)
        );
    }

    #[test]
    fn degenerate_joints_capture_nothing() {
        let zeros = [Vector3::new(0.0, 0.0, 0.0); HAND_JOINT_COUNT];
        assert!(capture_finger_bones(&zeros, Handedness::Right).is_none());
        let mut collapsed = tracked(&open_right_hand(), Handedness::Right);
        collapsed[8] = collapsed[7];
        assert!(capture_finger_bones(&collapsed, Handedness::Right).is_none());
        let identity = Quaternion::new(1.0, 0.0, 0.0, 0.0);
        assert!(grip_transform(&zeros, &collapsed, vec3(0.0, 0.0, 0.0), identity).is_none());
    }

    #[test]
    fn grip_transform_recovers_the_item_in_controller_space() {
        // The glove in controller space (mirrored for the left), and the
        // same hand tracked under a controller pose: the item the hand holds
        // must land back at its controller-space pose.
        for hand in [Handedness::Right, Handedness::Left] {
            let glove = forward_kinematics(&fist_right_hand())
                .0
                .map(|p| hand.mirror_point(p));
            let controller = Quaternion::from_axis_angle(Vector3::unit_y(), Deg(-40.0));
            let controller_position = vec3(2.0, 3.0, 4.0);
            let tracked = glove.map(|p| controller_position + controller.rotate_vector(p));
            let offset = vec3(0.05, -0.02, 0.1);
            let rotation = Quaternion::from_axis_angle(Vector3::unit_x(), Deg(25.0));
            let (o, r) = grip_transform(
                &tracked,
                &glove,
                controller_position + controller.rotate_vector(offset),
                controller * rotation,
            )
            .unwrap();
            assert!((o - offset).magnitude() < 1e-5, "{hand:?}");
            assert!(r.dot(rotation).abs() > 1.0 - 1e-5, "{hand:?}");
            let (p, q) = hand_pose(&tracked, &glove).unwrap();
            assert!((p - controller_position).magnitude() < 1e-5, "{hand:?}");
            assert!(q.dot(controller).abs() > 1.0 - 1e-5, "{hand:?}");
        }
    }
}
