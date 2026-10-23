//! Tracked-hand capture: OpenXR `XR_EXT_hand_tracking` joint positions to the
//! glove's finger bones, for authoring grips with real hands.
//!
//! The 26 OpenXR joints (palm, wrist, thumb metacarpal..tip, index..little
//! metacarpal..tip) share indices 1..=25 with the SteamVR skeleton the glove
//! poses use (index 0 is the palm there, the root here). Only joint positions
//! are used. The glove keeps its own size: its palm is fitted rigidly over the
//! tracked palm, then each finger is solved, with the glove's bone lengths and
//! within simple joint limits, to put its tip on the tracked tip while bending
//! like the tracked finger. Bone roll is not tracked, so the thumb's
//! opposition twist is the glove's own.
use cgmath::{
    InnerSpace, Matrix, Matrix3, Quaternion, Rad, Rotation, Rotation3, SquareMatrix, Vector3,
};
use rapier3d::na;

use crate::hand_pose::{FingerBones, Pose, fist_right_hand, open_right_hand};
use crate::physics::util::{nquat_to_quat, vec_to_nvec};
use crate::vr_config::Handedness;

pub const HAND_JOINT_COUNT: usize = 26;

/// OpenXR joint positions, indexed like `XrHandJointEXT`.
pub type HandJoints = [Vector3<f32>; HAND_JOINT_COUNT];

/// Fingertip joints, thumb to little.
pub(crate) const TIPS: [usize; 5] = [5, 10, 15, 20, 25];

/// Palm joints the glove's palm is fitted to, and their weights: the knuckles
/// most, as the fingers hang from them; the wrist least, as runtimes place it
/// differently.
const PALM: [(usize, f32); 10] = [
    (1, 0.25),
    (2, 0.5),
    (6, 1.0),
    (11, 1.0),
    (16, 1.0),
    (21, 1.0),
    (7, 2.0),
    (12, 2.0),
    (17, 2.0),
    (22, 2.0),
];

/// Each finger, thumb to little: the bone it hangs from (its metacarpal; the
/// wrist for the thumb) and the three bones solved to reach its tip.
const FINGERS: [(usize, [usize; 3]); 5] = [
    (1, [2, 3, 4]),
    (6, [7, 8, 9]),
    (11, [12, 13, 14]),
    (16, [17, 18, 19]),
    (21, [22, 23, 24]),
];

/// Middle metacarpal base to tip along the bones: a hand's size, whatever
/// its curl (and wherever a runtime puts the wrist).
const MIDDLE_CHAIN: [usize; 5] = [11, 12, 13, 14, 15];

/// Joint limits, degrees. A finger's knuckle flexes and spreads; its two far
/// joints, and the thumb's, only hinge. Flexion is limited from straight
/// (in line with the parent bone): a joint bends back only a little past it.
const KNUCKLE_EXTENSION: f32 = 25.0;
const HINGE_EXTENSION: f32 = 15.0;
const FLEXION: f32 = 120.0;
/// A knuckle's swing off its flex (its spread) either way.
const KNUCKLE_SWING: f32 = 40.0;
/// The thumb's base flexes within this range of the open glove's, and swings
/// off it within a cone this wide.
const THUMB_FLEX: (f32, f32) = (-60.0, 90.0);
const THUMB_SWING: f32 = 45.0;

/// Reaching the tracked tip (per finger length) against keeping each joint
/// at the tracked finger's angle (per radian), which shares out the curl.
const TIP_WEIGHT: f32 = 100.0;
const SOLVE_ITERATIONS: usize = 50;

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

const IDENTITY: Quaternion<f32> = Quaternion::new(1.0, 0.0, 0.0, 0.0);

/// World positions and rotations of a pose's bones 0..=25 (SteamVR numbers).
pub(crate) fn forward_kinematics(pose: &Pose) -> (HandJoints, [Quaternion<f32>; HAND_JOINT_COUNT]) {
    let mut positions = [pose.bone_positions[0]; HAND_JOINT_COUNT];
    let mut rotations = [pose.bone_rotations[0]; HAND_JOINT_COUNT];
    for bone in 1..HAND_JOINT_COUNT {
        let p = parent(bone);
        positions[bone] = positions[p] + rotations[p].rotate_vector(pose.bone_positions[bone]);
        rotations[bone] = rotations[p] * pose.bone_rotations[bone];
    }
    (positions, rotations)
}

/// The glove a tracked hand is fitted with: its open joints in
/// controller-grip space as drawn for one hand, and `basis`, which carries a
/// bone's world rotation delta in pose numbers into that space
/// (`basis · Δ · basisᵀ`; improper when it mirrors, as pose numbers are
/// left-handed).
#[derive(Clone, Copy, Debug)]
pub struct GloveRig {
    pub open: HandJoints,
    pub basis: Matrix3<f32>,
}

/// The glove fitted over a tracked hand.
#[derive(Clone, Debug)]
pub struct HandFit {
    /// The controller-grip pose that draws the glove over the hand.
    pub position: Vector3<f32>,
    pub rotation: Quaternion<f32>,
    pub bones: FingerBones,
    /// The fitted glove's joints, in the tracked hand's space.
    pub joints: HandJoints,
    /// Fitted to tracked fingertip distance (thumb..little): what the glove
    /// could not reach, in the tracked hand's units.
    pub tip_errors: [f32; 5],
    /// The tracked hand's size over the glove's.
    pub size_ratio: f32,
    /// Each finger's solved angles and the limits they kept to.
    #[cfg(test)]
    pub(crate) solved: [(Angles, [(f32, f32); 3], f32); 5],
}

/// Fit `rig`'s glove over `tracked` (in the same units), or `None` for
/// degenerate joints. Either hand produces right-hand bones; the glove
/// mirrors them for the left.
pub fn fit_hand(tracked: &HandJoints, rig: &GloveRig) -> Option<HandFit> {
    let (position, rotation) = palm_fit(tracked, &rig.open)?;
    let to_rig = rotation.invert();
    let local = tracked.map(|p| to_rig.rotate_vector(p - position));
    let open = &rig.open;
    let curls = curls(rig.basis);
    let mut deltas = [IDENTITY; HAND_JOINT_COUNT];
    #[cfg(test)]
    let mut solved = [([0.0; 5], [(0.0, 0.0); 3], 0.0); 5];
    for (finger, &(base, bones)) in FINGERS.iter().enumerate() {
        // A metacarpal follows its tracked direction: cupped about its curl
        // axis as the glove cups, then swung the rest of the way.
        if base != 1 {
            let from = direction(open[bones[0]] - open[base])?;
            let to = direction(local[bones[0]] - local[base])?;
            let axis = curls[base].0;
            let cup = turn(axis, signed_angle(from, to, axis));
            deltas[base] =
                (Quaternion::between_vectors(cup.rotate_vector(from), to) * cup).normalize();
        }
        let chain = Chain::new(open, &local, deltas[base], base, bones, &curls, finger == 0)?;
        let angles = chain.solve();
        #[cfg(test)]
        {
            solved[finger] = (angles, chain.limits, chain.max_swing);
        }
        let turns = chain.deltas(&angles);
        for (bone, turn) in bones.into_iter().zip(turns) {
            deltas[bone] = turn;
        }
        // The tip bone keeps its local rotation.
        deltas[bones[2] + 1] = turns[2];
    }
    let mut joints = *open;
    for bone in 2..HAND_JOINT_COUNT {
        let p = parent(bone);
        joints[bone] = joints[p] + deltas[p].rotate_vector(open[bone] - open[p]);
    }
    let joints = joints.map(|p| position + rotation.rotate_vector(p));
    let length = |joints: &HandJoints| {
        MIDDLE_CHAIN
            .windows(2)
            .map(|pair| (joints[pair[1]] - joints[pair[0]]).magnitude())
            .sum::<f32>()
    };
    Some(HandFit {
        position,
        rotation,
        bones: finger_bones(&deltas, rig.basis),
        tip_errors: TIPS.map(|tip| (joints[tip] - tracked[tip]).magnitude()),
        joints,
        size_ratio: length(tracked) / length(open),
        #[cfg(test)]
        solved,
    })
}

/// The rigid pose putting `open`'s palm over `tracked`'s (weighted Kabsch),
/// or `None` when the palm joints do not span a plane.
fn palm_fit(tracked: &HandJoints, open: &HandJoints) -> Option<(Vector3<f32>, Quaternion<f32>)> {
    let total: f32 = PALM.iter().map(|(_, weight)| weight).sum();
    let centroid = |joints: &HandJoints| {
        PALM.iter()
            .map(|&(joint, weight)| joints[joint] * weight)
            .sum::<Vector3<f32>>()
            / total
    };
    let (from, to) = (centroid(open), centroid(tracked));
    let covariance = PALM
        .iter()
        .fold(na::Matrix3::zeros(), |sum, &(joint, weight)| {
            sum + vec_to_nvec(open[joint] - from)
                * vec_to_nvec(tracked[joint] - to).transpose()
                * weight
        });
    let svd = covariance.svd(true, true);
    let (u, v) = (svd.u?, svd.v_t?.transpose());
    let values = svd.singular_values;
    let (smallest, _) = values.argmin();
    let largest = values.max();
    let second = values.sum() - largest - values[smallest];
    if !(largest.is_finite() && second > largest * 1e-4) {
        return None;
    }
    // A reflection would turn the glove into the other hand.
    let mut correction = na::Matrix3::identity();
    correction[(smallest, smallest)] = (v * u.transpose()).determinant().signum();
    let rotation = nquat_to_quat(na::UnitQuaternion::from_matrix(
        &(v * correction * u.transpose()),
    ));
    Some((to - rotation.rotate_vector(from), rotation))
}

/// Each bone's curl in a rig's space (`basis`): the axis its joint turns
/// about, relative to its parent bone, from the pose data's open hand to its
/// fist, and how far the fist turns it. Turning positively about it curls.
fn curls(basis: Matrix3<f32>) -> [(Vector3<f32>, f32); HAND_JOINT_COUNT] {
    let (_, open) = forward_kinematics(&open_right_hand());
    let (_, fist) = forward_kinematics(&fist_right_hand());
    let delta = |bone: usize| fist[bone] * open[bone].invert();
    // A mirroring basis turns rotations the other way.
    let sign = basis.determinant().signum();
    std::array::from_fn(|bone| {
        let zero = Vector3::new(0.0, 0.0, 0.0);
        if bone < 2 {
            return (zero, 0.0);
        }
        let relative = rotation_vector(delta(parent(bone)).invert() * delta(bone));
        let axis = direction(basis * relative * sign).unwrap_or(zero);
        (axis, relative.magnitude())
    })
}

/// Pose-number finger bones from per-bone world deltas in a rig's space.
fn finger_bones(deltas: &[Quaternion<f32>; HAND_JOINT_COUNT], basis: Matrix3<f32>) -> FingerBones {
    let (_, open) = forward_kinematics(&open_right_hand());
    let world: [Quaternion<f32>; HAND_JOINT_COUNT] = std::array::from_fn(|bone| {
        let delta = basis.transpose() * Matrix3::from(deltas[bone]) * basis;
        (Quaternion::from(delta) * open[bone]).normalize()
    });
    std::array::from_fn(|i| {
        let bone = i + 2;
        (world[parent(bone)].invert() * world[bone]).normalize()
    })
}

/// The angle turning `from` onto `to` about `axis`, both projected onto the
/// plane it turns in.
fn signed_angle(from: Vector3<f32>, to: Vector3<f32>, axis: Vector3<f32>) -> f32 {
    let from = from - axis * from.dot(axis);
    let to = to - axis * to.dot(axis);
    axis.dot(from.cross(to)).atan2(from.dot(to))
}

fn turn(axis: Vector3<f32>, angle: f32) -> Quaternion<f32> {
    Quaternion::from_axis_angle(axis, Rad(angle))
}

/// The turn by `v`'s length about `v`.
fn turn_by(v: Vector3<f32>) -> Quaternion<f32> {
    let angle = v.magnitude();
    if angle < 1e-9 {
        IDENTITY
    } else {
        turn(v / angle, angle)
    }
}

/// `q` as the vector `turn_by` turns by: its axis, as long as its angle.
fn rotation_vector(q: Quaternion<f32>) -> Vector3<f32> {
    let q = if q.s < 0.0 { -q } else { q };
    direction(q.v).map_or(Vector3::new(0.0, 0.0, 0.0), |axis| {
        axis * 2.0 * q.v.magnitude().atan2(q.s)
    })
}

/// A finger's solved angles: its first bone's flex, that bone's swing (two
/// parts), and its two hinges.
type Angles = [f32; 5];

/// One finger's three solved bones, in the open glove's palm space. Its
/// angles turn from the open glove, curling positive: the first bone flexes
/// about its curl axis (twisting as the glove's does) after a swing square
/// to it (spread, and whatever else flex alone cannot reach); the other two
/// only hinge.
struct Chain {
    /// Where the first bone starts, and the turn of the bone it hangs from.
    origin: Vector3<f32>,
    parent: Quaternion<f32>,
    /// The open glove's bones.
    offsets: [Vector3<f32>; 3],
    flex: Vector3<f32>,
    swing: [Vector3<f32>; 2],
    hinges: [Vector3<f32>; 2],
    /// Flex and hinge limits, and the largest swing.
    limits: [(f32, f32); 3],
    max_swing: f32,
    /// The tracked finger's angles and tip.
    tracked: Angles,
    tip: Vector3<f32>,
    length: f32,
}

impl Chain {
    fn new(
        open: &HandJoints,
        tracked: &HandJoints,
        parent: Quaternion<f32>,
        base: usize,
        bones: [usize; 3],
        curls: &[(Vector3<f32>, f32); HAND_JOINT_COUNT],
        thumb: bool,
    ) -> Option<Self> {
        let joints = [base, bones[0], bones[1], bones[2], bones[2] + 1];
        let open_bones = [0, 1, 2, 3].map(|k| open[joints[k + 1]] - open[joints[k]]);
        let first = direction(open_bones[1])?;
        let flex = curls[bones[0]].0;
        let across = direction(flex - first * flex.dot(first))?;
        // Limits from straight (in line with the parent bone), but taking
        // in the glove's own open hand and fist, whose bones need not lie
        // along a real hand's.
        let flexion = |k: usize, extension: f32| {
            let (axis, fist) = curls[joints[k + 1]];
            let bend = signed_angle(open_bones[k], open_bones[k + 1], axis);
            let extension = extension.to_radians();
            (
                (-extension - bend).min(-extension),
                (FLEXION.to_radians() - bend).max(fist + 5f32.to_radians()),
            )
        };
        let base_limits = if thumb {
            (THUMB_FLEX.0.to_radians(), THUMB_FLEX.1.to_radians())
        } else {
            flexion(0, KNUCKLE_EXTENSION)
        };
        let mut chain = Self {
            origin: open[base] + parent.rotate_vector(open_bones[0]),
            parent,
            offsets: [open_bones[1], open_bones[2], open_bones[3]],
            flex,
            swing: [across, first.cross(across)],
            hinges: [curls[bones[1]].0, curls[bones[2]].0],
            limits: [
                base_limits,
                flexion(1, HINGE_EXTENSION),
                flexion(2, HINGE_EXTENSION),
            ],
            max_swing: if thumb { THUMB_SWING } else { KNUCKLE_SWING }.to_radians(),
            tracked: [0.0; 5],
            tip: tracked[joints[4]],
            length: open_bones[1..].iter().map(|bone| bone.magnitude()).sum(),
        };
        let targets = [1, 2, 3].map(|k| direction(tracked[joints[k + 1]] - tracked[joints[k]]));
        chain.tracked = chain.tracked_angles([targets[0]?, targets[1]?, targets[2]?]);
        Some(chain)
    }

    /// The three bones' world turns for `angles`.
    fn deltas(&self, angles: &Angles) -> [Quaternion<f32>; 3] {
        let swing = turn_by(self.swing[0] * angles[1] + self.swing[1] * angles[2]);
        let first = self.parent * turn(self.flex, angles[0]) * swing;
        let second = first * turn(self.hinges[0], angles[3]);
        let third = second * turn(self.hinges[1], angles[4]);
        [first, second, third]
    }

    fn tip(&self, angles: &Angles) -> Vector3<f32> {
        let deltas = self.deltas(angles);
        self.origin
            + (0..3)
                .map(|k| deltas[k].rotate_vector(self.offsets[k]))
                .sum::<Vector3<f32>>()
    }

    /// The angles that point each bone along the tracked one's direction
    /// (`targets`), as near as the joints allow: the first bone's flex is
    /// all the turn about its curl axis, its swing the rest.
    fn tracked_angles(&self, targets: [Vector3<f32>; 3]) -> Angles {
        let along = self.parent.invert().rotate_vector(targets[0]);
        let flex = signed_angle(self.offsets[0], along, self.flex);
        let unflexed = turn(self.flex, -flex).rotate_vector(along);
        let swing = rotation_vector(Quaternion::between_vectors(
            self.offsets[0].normalize(),
            unflexed,
        ));
        let mut angles = [
            flex,
            swing.dot(self.swing[0]),
            swing.dot(self.swing[1]),
            0.0,
            0.0,
        ];
        for k in 0..2 {
            let before = self.deltas(&angles)[k];
            let target = before.invert().rotate_vector(targets[k + 1]);
            angles[3 + k] = signed_angle(self.offsets[k + 1], target, self.hinges[k]);
        }
        self.limit(angles)
    }

    fn limit(&self, mut angles: Angles) -> Angles {
        let (min, max) = self.limits[0];
        angles[0] = angles[0].clamp(min, max);
        let swing = angles[1].hypot(angles[2]);
        if swing > self.max_swing {
            angles[1] *= self.max_swing / swing;
            angles[2] *= self.max_swing / swing;
        }
        // The swing's first part turns the bone the way flex does: their sum
        // is the bend the flex limits hold.
        let bend = angles[0] + angles[1];
        angles[1] += bend.clamp(min, max) - bend;
        for k in 0..2 {
            angles[3 + k] = angles[3 + k].clamp(self.limits[1 + k].0, self.limits[1 + k].1);
        }
        angles
    }

    /// The tip miss (per finger length, weighted) and each angle's departure
    /// from the tracked finger's.
    fn residuals(&self, angles: &Angles) -> Residuals {
        let miss = (self.tip(angles) - self.tip) * (TIP_WEIGHT / self.length);
        Residuals::from_fn(|row, _| match row {
            0..3 => miss[row],
            row => angles[row - 3] - self.tracked[row - 3],
        })
    }

    /// The angles nearest the tracked finger's that put the tip on the
    /// tracked tip, or as near it as the limits allow.
    fn solve(&self) -> Angles {
        let solved = self.least_squares(self.tracked);
        // A straight finger longer than the tracked one is stuck: every
        // bend moves its tip across the miss, not along it. Try bent too.
        let [flex, sideways, across, middle, end] = self.tracked;
        let alternative =
            self.least_squares([flex + 0.15, sideways, across, middle + 0.3, end + 0.3]);
        let cost = |angles: &Angles| self.residuals(angles).norm_squared();
        if cost(&alternative) < cost(&solved) {
            alternative
        } else {
            solved
        }
    }

    /// The angles from `start`, kept within the limits each step, that
    /// minimize the squared residuals: Levenberg-Marquardt.
    fn least_squares(&self, start: Angles) -> Angles {
        const STEP: f32 = 1e-3;
        let mut angles = self.limit(start);
        let mut current = self.residuals(&angles);
        let mut damping = 1e-3;
        for _ in 0..SOLVE_ITERATIONS {
            let jacobian =
                na::SMatrix::<f32, 8, 5>::from_columns(&std::array::from_fn::<_, 5, _>(|i| {
                    let mut nudged = angles;
                    nudged[i] += STEP;
                    (self.residuals(&nudged) - current) / STEP
                }));
            let normal = jacobian.transpose() * jacobian;
            let damped = normal
                + na::SMatrix::<f32, 5, 5>::from_diagonal(&(normal.diagonal() * damping))
                + na::SMatrix::<f32, 5, 5>::identity() * 1e-9;
            let Some(step) = damped.lu().solve(&(-jacobian.transpose() * current)) else {
                break;
            };
            let candidate = self.limit(std::array::from_fn(|i| angles[i] + step[i]));
            let moved = self.residuals(&candidate);
            let (before, after) = (current.norm_squared(), moved.norm_squared());
            if after < before {
                (angles, current) = (candidate, moved);
                damping *= 0.3;
                if before - after <= before * 1e-6 {
                    break;
                }
            } else {
                damping *= 10.0;
                if damping > 1e6 {
                    break;
                }
            }
        }
        angles
    }
}

/// A chain's residuals: the weighted tip miss, then each angle's departure.
type Residuals = na::SVector<f32, 8>;

/// A grip pose capture's joints as one log line: the tracked hand (`raw`)
/// and the glove fitted over it (`glove`, what the tip error compares), both
/// pawn-local world units at `world_scale`.
pub fn joints_log_line(
    hand: Handedness,
    model: &str,
    world_scale: f32,
    raw: &HandJoints,
    glove: &HandJoints,
) -> String {
    let list = |joints: &HandJoints| {
        joints
            .iter()
            .map(|p| format!("[{:.5},{:.5},{:.5}]", p.x, p.y, p.z))
            .collect::<Vec<_>>()
            .join(",")
    };
    format!(
        "{JOINTS_LOG} hand={} model={model} world_scale={world_scale} raw=[{}] glove=[{}]",
        match hand {
            Handedness::Left => "l",
            Handedness::Right => "r",
        },
        list(raw),
        list(glove),
    )
}

const JOINTS_LOG: &str = "SHOCK2QUEST_VR_GRIP_POSE_JOINTS";

/// Write device log text (e.g. logcat) to `path` as a hand recording (the
/// fixture format [`recorded_hands`] reads): one frame per
/// [`joints_log_line`], with its raw joints on its hand. Lines without one are
/// skipped. A test-side tool: call it from a scratch test to turn a device
/// session's captures into a fixture under `tests/fixtures/hand_recordings/`.
#[cfg(test)]
pub(crate) fn joints_log_recording(log: &str, path: &std::path::Path) -> std::io::Result<()> {
    use crate::input::recording::{InputRecorder, RecordedFrame, RecordingHeader};
    let mut recorder = InputRecorder::create(
        path,
        &RecordingHeader {
            version: 2,
            scene: String::new(),
            save: String::new(),
            presentation: crate::PresentationMode::Vr,
            experimental: Vec::new(),
            glove_forward_cm: 0.0,
        },
    )?;
    for line in log.lines() {
        let Some(fields) = line.split_once(JOINTS_LOG).map(|(_, rest)| rest) else {
            continue;
        };
        let field = |name: &str| {
            fields
                .split(' ')
                .find_map(|field| field.strip_prefix(name)?.strip_prefix('='))
        };
        let Some(raw) = field("raw")
            .and_then(|raw| serde_json::from_str::<Vec<[f32; 3]>>(raw).ok())
            .and_then(|raw| <[[f32; 3]; HAND_JOINT_COUNT]>::try_from(raw).ok())
        else {
            continue;
        };
        let mut input = crate::input_context::InputContext::default();
        let hand = match field("hand") {
            Some("l") => &mut input.left_hand,
            _ => &mut input.right_hand,
        };
        hand.joints = Some(raw.map(Vector3::from));
        recorder.record(&RecordedFrame {
            dt: 0.0,
            total: 0.0,
            input,
            triggered: Vec::new(),
            held: Vec::new(),
        })?;
    }
    recorder.finish().map(|_| ())
}

/// Each frame's tracked joints `[left, right]` from an input recording
/// (`ToggleInputRecording`), skipping frames that track neither hand: real
/// hands to test the mapping against.
#[cfg(test)]
pub(crate) fn recorded_hands(
    path: &std::path::Path,
) -> std::io::Result<Vec<[Option<HandJoints>; 2]>> {
    let (_, frames) = crate::input::recording::read_recording(path)?;
    Ok(frames
        .into_iter()
        .map(|frame| [frame.input.left_hand.joints, frame.input.right_hand.joints])
        .filter(|hands| hands.iter().any(Option::is_some))
        .collect())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::hand_pose::FingerAmounts;
    use cgmath::{Deg, vec3};

    /// Metres, like the pose data.
    const REACHED: f32 = 0.003;

    fn placement() -> (Vector3<f32>, Quaternion<f32>) {
        (
            vec3(0.4, 1.2, -0.3),
            Quaternion::from_axis_angle(vec3(0.3, 1.0, -0.2).normalize(), Deg(70.0)),
        )
    }

    /// The pose data is a right hand in left-handed (Unity) numbers: a real
    /// right hand's numbers are those mirrored, a left hand's those as they
    /// are.
    fn pose_basis(hand: Handedness) -> Matrix3<f32> {
        match hand {
            Handedness::Right => Matrix3::new(-1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0),
            Handedness::Left => Matrix3::identity(),
        }
    }

    /// A glove made of the pose data's own joints, for `hand`.
    pub(crate) fn pose_rig(hand: Handedness) -> GloveRig {
        let basis = pose_basis(hand);
        GloveRig {
            open: forward_kinematics(&open_right_hand()).0.map(|p| basis * p),
            basis,
        }
    }

    /// A pose's joints as tracking would report them for `hand`, placed
    /// somewhere arbitrary in the tracking space.
    pub(crate) fn tracked(pose: &Pose, hand: Handedness) -> HandJoints {
        let (position, rotation) = placement();
        forward_kinematics(pose)
            .0
            .map(|p| position + rotation.rotate_vector(pose_basis(hand) * p))
    }

    /// Asserts the fit's angles keep to the joint limits.
    pub(crate) fn assert_within_limits(fit: &HandFit) {
        for (finger, (angles, limits, max_swing)) in fit.solved.iter().enumerate() {
            let within =
                |angle: f32, (min, max): (f32, f32)| (min - 1e-4..=max + 1e-4).contains(&angle);
            assert!(
                within(angles[0], limits[0])
                    && within(angles[0] + angles[1], limits[0])
                    && angles[1].hypot(angles[2]) <= max_swing + 1e-4
                    && within(angles[3], limits[1])
                    && within(angles[4], limits[2]),
                "finger {finger}: {:?} degrees",
                angles.map(f32::to_degrees)
            );
        }
    }

    /// `tracked` fitted with the pose data's own glove, within the limits.
    fn fit(tracked: &HandJoints, hand: Handedness) -> HandFit {
        let fit = fit_hand(tracked, &pose_rig(hand)).unwrap();
        assert_within_limits(&fit);
        fit
    }

    /// Largest angle between a fitted bone and the expected one.
    fn max_bone_angle(fitted: &FingerBones, expected: &Pose) -> Deg<f32> {
        let a = forward_kinematics(&Pose::from_finger_bones(fitted)).0;
        let b = forward_kinematics(expected).0;
        (2..HAND_JOINT_COUNT)
            .filter(|bone| !TIPS.contains(bone))
            .map(|bone| Deg::from((a[bone + 1] - a[bone]).angle(b[bone + 1] - b[bone])))
            .fold(Deg(0.0), |m, d| if d > m { d } else { m })
    }

    /// `pose` with the open hand's bone offsets and palm: a hand of the
    /// glove's size whose fingers alone curl. (The authored fist moves the
    /// metacarpal bases ~5 mm and cups the palm, which tilts a palm fit.)
    fn rigid(pose: Pose) -> Pose {
        let open = open_right_hand();
        let mut bone_rotations = pose.bone_rotations;
        for bone in [0, 1, 6, 11, 16, 21] {
            bone_rotations[bone] = open.bone_rotations[bone];
        }
        Pose {
            bone_positions: open.bone_positions,
            bone_rotations,
        }
    }

    /// The glove's own hands, curled as when holding something.
    pub(crate) fn grips() -> [(&'static str, Pose); 4] {
        let open = open_right_hand();
        let fist = fist_right_hand();
        let amounts = |thumb, index, middle, ring, pinky| FingerAmounts {
            thumb,
            index,
            middle,
            ring,
            pinky,
        };
        [
            ("half", rigid(open.blend(&fist, 0.5))),
            (
                "wrap",
                rigid(open.blend_per_finger(&fist, &amounts(0.3, 0.6, 0.7, 0.75, 0.8))),
            ),
            (
                "pinch",
                rigid(open.blend_per_finger(&fist, &amounts(0.35, 0.5, 0.3, 0.3, 0.3))),
            ),
            ("fist", rigid(fist)),
        ]
    }

    /// `joints` scaled by `factor` about the wrist.
    fn scaled(joints: &HandJoints, factor: f32) -> HandJoints {
        joints.map(|p| joints[1] + (p - joints[1]) * factor)
    }

    fn reached(errors: &[f32; 5]) -> bool {
        errors.iter().all(|e| *e <= REACHED)
    }

    /// Asserts each fitted tip is on the tracked one (within `reached`), or
    /// the tracked tip is beyond the glove finger's length and the glove
    /// comes as near as a straight finger aimed at it would.
    pub(crate) fn assert_reached(
        fit: &HandFit,
        tracked: &HandJoints,
        open: &HandJoints,
        reached: f32,
        name: &str,
    ) {
        for (finger, &(_, bones)) in FINGERS.iter().enumerate() {
            let error = fit.tip_errors[finger];
            let length: f32 = (bones[0]..=bones[2])
                .map(|bone| (open[bone + 1] - open[bone]).magnitude())
                .sum();
            let shortfall = (tracked[TIPS[finger]] - fit.joints[bones[0]]).magnitude() - length;
            assert!(
                error <= reached || (shortfall > 0.0 && error <= shortfall + reached),
                "{name} finger {finger}: {error} off, {shortfall} out of reach; {:?}",
                fit.tip_errors
            );
        }
    }

    #[test]
    fn the_open_hand_fits_the_bind_pose() {
        let bind = open_right_hand().finger_bones();
        for hand in [Handedness::Left, Handedness::Right] {
            let fit = fit(&tracked(&open_right_hand(), hand), hand);
            for (bone, (a, b)) in fit.bones.iter().zip(&bind).enumerate() {
                assert!(a.dot(*b).abs() > 1.0 - 1e-5, "{hand:?} bone {bone}");
            }
            assert!(fit.tip_errors.iter().all(|e| *e < 1e-5), "{hand:?}");
            assert!((fit.size_ratio - 1.0).abs() < 1e-4, "{hand:?}");
        }
    }

    #[test]
    fn the_gloves_own_curled_hands_fit_unchanged() {
        for (name, pose) in grips() {
            for hand in [Handedness::Left, Handedness::Right] {
                let fit = fit(&tracked(&pose, hand), hand);
                let error = max_bone_angle(&fit.bones, &pose);
                assert!(error < Deg(2.0), "{hand:?} {name}: {error:?}");
                assert!(
                    fit.tip_errors.iter().all(|e| *e < 0.0005),
                    "{hand:?} {name}: {:?}",
                    fit.tip_errors
                );
            }
        }
        // Negative control: the open hand is far from the fist.
        assert!(max_bone_angle(&open_right_hand().finger_bones(), &fist_right_hand()) > Deg(30.0));
    }

    #[test]
    fn tips_reach_hands_larger_and_smaller_than_the_glove() {
        for hand in [Handedness::Left, Handedness::Right] {
            let open = pose_rig(hand).open;
            for (name, pose) in grips() {
                for factor in [0.9, 1.15] {
                    let joints = scaled(&tracked(&pose, hand), factor);
                    let fit = fit(&joints, hand);
                    let name = format!("{hand:?} {name} x{factor}");
                    assert_reached(&fit, &joints, &open, REACHED, &name);
                    // A larger hand's straighter fingers outreach the glove's.
                    if factor < 1.0 {
                        assert!(reached(&fit.tip_errors), "{name}: {:?}", fit.tip_errors);
                    }
                    assert!((fit.size_ratio - factor).abs() < 0.01, "{}", fit.size_ratio);
                }
            }
        }
    }

    #[test]
    fn the_thumb_reaches_from_a_shifted_base() {
        for hand in [Handedness::Left, Handedness::Right] {
            for (name, pose) in grips() {
                let hand_joints = tracked(&pose, hand);
                // 15 mm toward the wrist, or across the palm: where a real
                // thumb's base sits differs from the glove's.
                for toward in [1, 22] {
                    let mut joints = hand_joints;
                    let shift = (joints[toward] - joints[2]).normalize() * 0.015;
                    for joint in &mut joints[2..=5] {
                        *joint += shift;
                    }
                    let errors = fit(&joints, hand).tip_errors;
                    assert!(
                        reached(&errors),
                        "{hand:?} {name} toward {toward}: {errors:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn an_unreachable_tip_comes_as_near_as_the_limits_allow() {
        // Open and 20% larger: the glove's straight fingers fall short, but
        // only by what they lack in length.
        for hand in [Handedness::Left, Handedness::Right] {
            let joints = scaled(&tracked(&open_right_hand(), hand), 1.2);
            let fit = fit(&joints, hand);
            assert!(
                fit.tip_errors.iter().all(|e| *e > 0.005),
                "{:?}",
                fit.tip_errors
            );
            assert_reached(&fit, &joints, &pose_rig(hand).open, 0.0005, "open x1.2");
        }
    }

    /// A right hand as OpenXR reports one, built by hand rather than from the
    /// pose data: palm down (-Y), fingers forward (-Z), thumb on the left (-X).
    /// `index_curl` folds the index finger's joints down toward the palm side.
    pub(crate) fn openxr_right_hand(index_curl: f32) -> HandJoints {
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

    /// Hands not made from the pose data, in metres: the built one, and the
    /// synthetic recording's open hand (its pinch moves the index tip alone,
    /// sideways onto the thumb's, which no finger reaches).
    pub(crate) fn foreign_hands() -> [(&'static str, HandJoints); 2] {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/hand_recordings/synthetic.jsonl");
        let recorded = recorded_hands(&path).unwrap();
        // The fixture is pawn-local world units at world scale 1.
        let metres = |joints: HandJoints| joints.map(|p| p * crate::METERS_PER_WORLD_UNIT);
        [
            ("built, index curled", openxr_right_hand(0.6)),
            ("recorded open", metres(recorded[0][1].unwrap())),
        ]
    }

    #[test]
    fn tips_reach_hands_built_by_hand_and_recorded() {
        let open = pose_rig(Handedness::Right).open;
        // Straight and smaller than the glove: its straight fingers overshoot,
        // and must bend back onto the tips.
        let straight = openxr_right_hand(0.0);
        let smaller = ("built, straight, x0.9", scaled(&straight, 0.9));
        for (name, joints) in foreign_hands().into_iter().chain([smaller]) {
            assert_reached(
                &fit(&joints, Handedness::Right),
                &joints,
                &open,
                REACHED,
                name,
            );
        }
    }

    #[test]
    fn a_left_hand_fits_as_the_mirrored_right() {
        let mirror = |joints: &HandJoints| joints.map(|p| vec3(-p.x, p.y, p.z));
        let mut hands = foreign_hands().map(|(_, joints)| joints).to_vec();
        hands.extend(grips().map(|(_, pose)| tracked(&pose, Handedness::Right)));
        for right in hands {
            let a = fit(&right, Handedness::Right);
            let b = fit(&mirror(&right), Handedness::Left);
            for (bone, (a, b)) in a.bones.iter().zip(&b.bones).enumerate() {
                assert!(a.dot(*b).abs() > 1.0 - 1e-4, "bone {bone}");
            }
            for (a, b) in a.tip_errors.iter().zip(&b.tip_errors) {
                assert!((a - b).abs() < 5e-4, "{:?} vs {:?}", a, b);
            }
        }
    }

    #[test]
    fn a_curled_tracked_finger_curls_the_glove_the_way_the_fist_does() {
        // The index proximal's direction, captured open and curled, against
        // the fist's: curling the real finger moves the glove's toward it.
        let index = |bones: &FingerBones| {
            let joints = forward_kinematics(&Pose::from_finger_bones(bones)).0;
            (joints[8] - joints[7]).normalize()
        };
        let fist = index(&fist_right_hand().finger_bones());
        let angle = |curl| {
            let bones = fit(&openxr_right_hand(curl), Handedness::Right).bones;
            Deg::from(index(&bones).angle(fist))
        };
        assert!(
            angle(0.8) < angle(0.0) - Deg(20.0),
            "{:?} vs {:?}",
            angle(0.8),
            angle(0.0)
        );
    }

    #[test]
    fn recordings_yield_their_tracked_hands() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/hand_recordings/synthetic.jsonl");
        // Its first frame has no `thumbstick_click`, as older recordings do not.
        let frames = recorded_hands(&path).unwrap();
        assert!(!frames.is_empty());
        for [left, right] in &frames {
            assert!(left.is_none(), "the fixture tracks the right hand only");
            let right = right.expect("the right hand is tracked");
            assert!(fit_hand(&right, &pose_rig(Handedness::Right)).is_some());
        }
        // The fixture opens the hand, then pinches thumb to index.
        let pinch = |hands: &[Option<HandJoints>; 2]| {
            let joints = hands[1].unwrap();
            (joints[5] - joints[10]).magnitude()
        };
        assert!(pinch(&frames[0]) > 0.02 / crate::METERS_PER_WORLD_UNIT);
        assert!(pinch(frames.last().unwrap()) < 1e-4);
    }

    #[test]
    fn capture_log_lines_round_trip_into_a_hand_recording() {
        let raw = tracked(&open_right_hand(), Handedness::Left);
        let glove = raw.map(|p| p + vec3(0.01, 0.0, 0.0));
        let line = joints_log_line(Handedness::Left, "mug", 0.85, &raw, &glove);
        assert!(line.starts_with(
            "SHOCK2QUEST_VR_GRIP_POSE_JOINTS hand=l model=mug world_scale=0.85 raw=[["
        ));
        // As logcat prints it, among other lines.
        let log = format!(
            "10-22 17:51:56.072 I RustStdoutStderr: {line}\n\
             10-22 17:51:56.073 I RustStdoutStderr: SHOCK2QUEST_VR_GRIP_POSE_TOOL tip_error_mm=[1]\n"
        );
        let root = crate::test_support::TempDir::new("joints-log");
        let path = root.path().join("capture.jsonl");
        joints_log_recording(&log, &path).unwrap();
        let frames = recorded_hands(&path).unwrap();
        assert_eq!(frames.len(), 1);
        let [left, right] = frames[0];
        assert!(right.is_none());
        for (read, wrote) in left.expect("the left hand").iter().zip(&raw) {
            assert!((read - wrote).magnitude() < 1e-4, "{read:?} vs {wrote:?}");
        }
    }

    #[test]
    fn the_runtime_wrist_joint_barely_moves_the_glove() {
        let joints = tracked(&rigid(fist_right_hand()), Handedness::Left);
        let mut moved = joints;
        moved[1] += vec3(0.0, 0.005, 0.003);
        let (a, b) = (
            fit(&joints, Handedness::Left),
            fit(&moved, Handedness::Left),
        );
        for tip in TIPS {
            let shift = (a.joints[tip] - b.joints[tip]).magnitude();
            assert!(shift < 0.001, "tip {tip} moved {shift}");
        }
    }

    #[test]
    fn degenerate_joints_fit_nothing() {
        let rig = pose_rig(Handedness::Right);
        let zeros = [Vector3::new(0.0, 0.0, 0.0); HAND_JOINT_COUNT];
        assert!(fit_hand(&zeros, &rig).is_none());
        let mut collapsed = tracked(&open_right_hand(), Handedness::Right);
        collapsed[8] = collapsed[7];
        assert!(fit_hand(&collapsed, &rig).is_none());
    }

    #[test]
    fn the_fit_draws_the_glove_where_its_controller_holds_it() {
        // The open glove tracked under a controller pose: drawing it there
        // needs that controller pose back.
        for hand in [Handedness::Right, Handedness::Left] {
            let rig = pose_rig(hand);
            let controller = Quaternion::from_axis_angle(Vector3::unit_y(), Deg(-40.0));
            let controller_position = vec3(2.0, 3.0, 4.0);
            let tracked = rig
                .open
                .map(|p| controller_position + controller.rotate_vector(p));
            let fit = fit_hand(&tracked, &rig).unwrap();
            assert!(
                (fit.position - controller_position).magnitude() < 1e-5,
                "{hand:?}"
            );
            assert!(fit.rotation.dot(controller).abs() > 1.0 - 1e-5, "{hand:?}");
        }
    }
}
