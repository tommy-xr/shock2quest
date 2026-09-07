//! SteamVR hand poses for the vr_glove GLB model.
//!
//! Pose data is transcribed from the SteamVR Unity plugin's reference pose
//! assets (`SteamVR_Skeleton_Pose`, right hand): 31 bones in the standard
//! SteamVR hand skeleton order (root, wrist, 4 thumb + 4x5 finger bones,
//! then 5 aux bones). The glove GLB's 26 skin joints correspond exactly to
//! SteamVR bones 0..=25 (verified by bone name); the aux bones are not skin
//! joints and are ignored.
//!
//! The GLB rig (exported with FBX2glTF) does not share the pose data's
//! coordinate conventions: the source data is left-handed (Unity), and every
//! joint's local frame in the GLB differs from the pose data's frame by a
//! constant rotation. Poses are therefore retargeted per joint via
//! [`HandPoseRetarget`] instead of being written to the rig directly.

use cgmath::{InnerSpace, Matrix3, Matrix4, Quaternion, Vector3};
use dark::{glb_model::GlbModel, glb_skeleton::GlbSkeleton};

/// GLB skin joints driven by pose data. Joint 0 (Root) carries the exporter's
/// unit-conversion scale and joint 1 (wrist) is the hand's attachment point -
/// both keep their bind transforms.
const FIRST_POSED_JOINT: usize = 2;
const LAST_POSED_JOINT: usize = 25;

/// A hand pose: per-bone parent-relative translations and rotations, in the
/// SteamVR Unity plugin's conventions.
#[derive(Debug, Clone)]
pub struct Pose {
    /// Bone rotations as quaternions
    pub bone_rotations: Vec<Quaternion<f32>>,
    pub bone_positions: Vec<Vector3<f32>>,
}

/// One finger of the hand.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Finger {
    Thumb,
    Index,
    Middle,
    Ring,
    Pinky,
}

impl Finger {
    pub const ALL: [Finger; 5] = [
        Finger::Thumb,
        Finger::Index,
        Finger::Middle,
        Finger::Ring,
        Finger::Pinky,
    ];

    /// This finger's chain of SteamVR bones, metacarpal first. The one table:
    /// everything that has to know which bones a finger owns - the blend, the
    /// contact fit's phalanx capsules - derives it from here.
    pub fn bones(self) -> std::ops::RangeInclusive<usize> {
        match self {
            Finger::Thumb => 2..=5,
            Finger::Index => 6..=10,
            Finger::Middle => 11..=15,
            Finger::Ring => 16..=20,
            Finger::Pinky => 21..=25,
        }
    }

    /// The finger a SteamVR bone belongs to, if any.
    fn of_bone(bone: usize) -> Option<Finger> {
        Finger::ALL
            .into_iter()
            .find(|finger| finger.bones().contains(&bone))
    }

    /// Where this finger's curl lands in a [`FingerAmounts`].
    pub fn set(self, amounts: &mut FingerAmounts, curl: f32) {
        match self {
            Finger::Thumb => amounts.thumb = curl,
            Finger::Index => amounts.index = curl,
            Finger::Middle => amounts.middle = curl,
            Finger::Ring => amounts.ring = curl,
            Finger::Pinky => amounts.pinky = curl,
        }
    }

    /// This finger's curl out of a [`FingerAmounts`].
    pub fn curl_of(self, amounts: &FingerAmounts) -> f32 {
        match self {
            Finger::Thumb => amounts.thumb,
            Finger::Index => amounts.index,
            Finger::Middle => amounts.middle,
            Finger::Ring => amounts.ring,
            Finger::Pinky => amounts.pinky,
        }
    }

    /// A blend that curls only this finger - what the contact fit poses the
    /// rig with while it searches one finger at a time.
    pub fn alone(self, curl: f32) -> FingerAmounts {
        let mut amounts = FingerAmounts::default();
        self.set(&mut amounts, curl);
        amounts
    }
}

/// Per-finger blend amounts (0.0 = first pose, 1.0 = second pose) for
/// [`Pose::blend_per_finger`]. `Default` leaves every finger at 0.0.
#[derive(Debug, Clone, Copy, Default)]
pub struct FingerAmounts {
    pub thumb: f32,
    pub index: f32,
    pub middle: f32,
    pub ring: f32,
    pub pinky: f32,
}

impl FingerAmounts {
    /// The blend amount for a SteamVR bone index (0.0 for non-finger bones).
    fn for_bone(&self, bone: usize) -> f32 {
        Finger::of_bone(bone).map_or(0.0, |finger| finger.curl_of(self))
    }
}

impl Pose {
    /// Interpolate between two poses (0.0 = self, 1.0 = other). Drives analog
    /// hand state, e.g. open hand -> fist by grip squeeze.
    /// (Positions are blended alongside rotations for completeness, but
    /// [`HandPoseRetarget::apply`] only consumes rotations.)
    pub fn blend(&self, other: &Pose, amount: f32) -> Pose {
        self.blend_with(other, |_| amount)
    }

    /// Interpolate toward `other` with an independent amount per finger -
    /// e.g. a trigger half-pull curls only the index finger.
    pub fn blend_per_finger(&self, other: &Pose, amounts: &FingerAmounts) -> Pose {
        self.blend_with(other, |bone| amounts.for_bone(bone))
    }

    fn blend_with(&self, other: &Pose, amount_for_bone: impl Fn(usize) -> f32) -> Pose {
        debug_assert_eq!(
            self.bone_rotations.len(),
            other.bone_rotations.len(),
            "blending poses with different bone counts"
        );
        let bone_rotations = self
            .bone_rotations
            .iter()
            .zip(&other.bone_rotations)
            .enumerate()
            .map(|(bone, (a, b))| {
                // Slerp along the shortest arc
                let b = if a.dot(*b) < 0.0 { -*b } else { *b };
                a.slerp(b, amount_for_bone(bone))
            })
            .collect();
        let bone_positions = self
            .bone_positions
            .iter()
            .zip(&other.bone_positions)
            .enumerate()
            .map(|(bone, (a, b))| a + (b - a) * amount_for_bone(bone))
            .collect();
        Pose {
            bone_rotations,
            bone_positions,
        }
    }
}

/// Convert a Unity-convention (left-handed) quaternion to the GLB's
/// right-handed space: a mirror across the YZ plane.
fn mirror_x(q: Quaternion<f32>) -> Quaternion<f32> {
    Quaternion::new(q.s, q.v.x, -q.v.y, -q.v.z)
}

/// Rotation of the GLB wrist frame relative to the pose data's wrist frame
/// (after handedness conversion). Solved once by aligning the five metacarpal
/// bind offsets of the two rigs (Kabsch fit); see projects/vr-gloves.md.
fn wrist_frame() -> Quaternion<f32> {
    Quaternion::new(0.998_773_2, -0.008_677_66, -0.034_176_15, 0.034_767_6)
}

/// Extract the rotation part of a TRS matrix (columns normalized to strip
/// scale; assumes a positive-determinant transform - a mirrored rig, e.g. a
/// left hand authored as a mirrored right hand, would need sign handling).
fn rotation_of(m: &Matrix4<f32>) -> Quaternion<f32> {
    let rot = Matrix3::from_cols(
        m.x.truncate().normalize(),
        m.y.truncate().normalize(),
        m.z.truncate().normalize(),
    );
    Quaternion::from(rot).normalize()
}

/// Per-joint retargeting from SteamVR pose space onto the glove GLB rig.
///
/// Both rigs describe the same physical hand, but each GLB joint's local
/// frame is rotated by a constant basis change `C_j` relative to the pose
/// data's frame for that bone. Because the GLB's bind pose is the same
/// physical pose as the plugin's reference open-hand/bind pose, every `C_j`
/// can be recovered recursively from the two bind poses:
///
/// ```text
/// C_j = mirror(reference_rotation_j)^-1 * C_parent(j) * glb_bind_rotation_j
/// ```
///
/// seeded with [`wrist_frame`]. A pose rotation then maps onto the rig as:
///
/// ```text
/// glb_local_j = C_parent(j)^-1 * mirror(pose_rotation_j) * C_j
/// ```
///
/// This reproduces the GLB bind pose exactly when applying the reference
/// pose, and only ever writes rotations to the rig (via
/// [`GlbModel::set_joint_rotation`]), so bone lengths cannot stretch.
pub struct HandPoseRetarget {
    /// Basis change `C_j` per joint
    frames: Vec<Quaternion<f32>>,
    /// Parent joint index per joint
    parents: Vec<Option<usize>>,
}

/// Last posed joint clamped to what the skeleton and pose data actually have.
fn posed_joint_range(joint_count: usize, bone_count: usize) -> std::ops::RangeInclusive<usize> {
    let last = LAST_POSED_JOINT
        .min(joint_count.saturating_sub(1))
        .min(bone_count.saturating_sub(1));
    FIRST_POSED_JOINT..=last
}

impl HandPoseRetarget {
    /// Build the retarget for the right-hand glove model, using the open-hand
    /// reference pose (identical to the plugin's bind pose) for calibration.
    pub fn for_right_glove(skeleton: &GlbSkeleton) -> Self {
        // Guard against being handed a non-glove skeleton: the calibration
        // below is only meaningful for the SteamVR hand rig.
        let wrist_name = skeleton
            .node_index_for_joint(1)
            .and_then(|n| skeleton.get_node(n))
            .and_then(|n| n.name.clone());
        if skeleton.joint_count() != 26 || wrist_name.as_deref() != Some("wrist_r") {
            tracing::warn!(
                "HandPoseRetarget::for_right_glove: skeleton does not look like the SteamVR right glove ({} joints, wrist joint {:?})",
                skeleton.joint_count(),
                wrist_name
            );
        }
        Self::new(skeleton, &open_right_hand())
    }

    fn new(skeleton: &GlbSkeleton, reference: &Pose) -> Self {
        let joint_count = skeleton.joint_count();
        let identity = Quaternion::new(1.0, 0.0, 0.0, 0.0);
        let mut frames = vec![identity; joint_count];
        let mut parents = vec![None; joint_count];

        if joint_count > 1 {
            frames[1] = wrist_frame();
        }

        for joint in posed_joint_range(joint_count, reference.bone_rotations.len()) {
            let Some(node) = skeleton
                .node_index_for_joint(joint)
                .and_then(|n| skeleton.get_node(n))
            else {
                continue;
            };
            let Some(parent) = node
                .parent_index
                .and_then(|p| skeleton.joint_index_for_node(p))
            else {
                continue;
            };
            // The recursion needs the parent's frame to be computed already;
            // SteamVR skin joint order is topologically sorted so this holds.
            debug_assert!(parent < joint, "joint {joint} precedes its parent {parent}");
            parents[joint] = Some(parent);
            let glb_bind = rotation_of(&node.local_transform);
            let reference_bind = mirror_x(reference.bone_rotations[joint]);
            frames[joint] = (reference_bind.conjugate() * frames[parent] * glb_bind).normalize();
        }

        Self { frames, parents }
    }

    /// Apply `pose` to the glove model's finger joints (the wrist and root
    /// keep their bind transforms).
    pub fn apply(&self, pose: &Pose, model: &mut GlbModel) {
        for joint in posed_joint_range(self.frames.len(), pose.bone_rotations.len()) {
            let Some(parent) = self.parents[joint] else {
                continue;
            };
            let rotation = (self.frames[parent].conjugate()
                * mirror_x(pose.bone_rotations[joint])
                * self.frames[joint])
                .normalize();
            model.set_joint_rotation(joint, rotation);
        }
    }
}

/// Returns the open hand pose for the right hand (the plugin's bind pose)
pub fn open_right_hand() -> Pose {
    let positions = vec![
        Vector3::new(0.0, 0.0, 0.0), // Wrist
        Vector3::new(-0.034037687, 0.03650266, 0.16472164),
        Vector3::new(-0.012083233, 0.028070247, 0.025049694),
        Vector3::new(0.040405963, -0.000000051561553, 0.000000045447194),
        Vector3::new(0.032516792, -0.000000051137583, -0.000000012933195),
        Vector3::new(0.030463902, 0.00000016269207, 0.0000000792839),
        Vector3::new(0.0006324522, 0.026866155, 0.015001948),
        Vector3::new(0.074204385, 0.005002201, -0.00023377323),
        Vector3::new(0.043930072, 0.000000059567498, 0.00000018367103),
        Vector3::new(0.02869547, -0.00000009398158, -0.00000012649753),
        Vector3::new(0.022821384, -0.00000014365155, 0.00000007651614),
        Vector3::new(0.0021773134, 0.007119544, 0.016318738),
        Vector3::new(0.07095288, -0.00077883265, -0.000997186),
        Vector3::new(0.043108486, -0.00000009950596, -0.0000000067041825),
        Vector3::new(0.033266045, -0.00000001320567, -0.000000021670374),
        Vector3::new(0.025892371, 0.00000009984198, -0.0000000020352908),
        Vector3::new(0.0005134356, -0.0065451227, 0.016347693),
        Vector3::new(0.06587581, -0.0017857892, -0.00069344096),
        Vector3::new(0.04069671, -0.000000095347104, -0.000000022934731),
        Vector3::new(0.028746964, 0.00000010089892, 0.000000045306827),
        Vector3::new(0.022430236, 0.00000010846127, -0.000000017428562),
        Vector3::new(-0.002478151, -0.01898137, 0.015213584),
        Vector3::new(0.0628784, -0.0028440945, -0.0003315112),
        Vector3::new(0.030219711, -0.00000003418319, -0.00000009332872),
        Vector3::new(0.018186597, -0.0000000050220166, -0.00000020934549),
        Vector3::new(0.01801794, -0.0000000200012, 0.0000000659746),
        Vector3::new(-0.0060591106, 0.05628522, 0.060063843),
        Vector3::new(-0.04041555, -0.043017667, 0.019344581),
        Vector3::new(-0.03935372, -0.07567404, 0.047048334),
        Vector3::new(-0.038340144, -0.09098663, 0.08257892),
        Vector3::new(-0.031805996, -0.08721431, 0.12101539),
    ];

    let rotations = vec![
        Quaternion::new(-0.00000004371139, -6.123234e-17, 1.0, 6.123234e-17),
        Quaternion::new(-0.055146642, -0.078608155, -0.92027926, 0.3792963),
        Quaternion::new(0.5674181, -0.46411175, -0.623374, 0.2721063),
        Quaternion::new(0.9948384, 0.08293856, -0.019454371, -0.055129882),
        Quaternion::new(0.9747928, -0.0032133153, -0.021866836, 0.22201493),
        Quaternion::new(1.0, 0.0, 0.0, 0.0), // Identity quaternion
        Quaternion::new(0.42197865, -0.6442515, -0.42213318, -0.4782025),
        Quaternion::new(0.9953317, 0.0070068412, 0.039123755, -0.08794935),
        Quaternion::new(0.9978909, 0.045808382, -0.0021422536, 0.0459431),
        Quaternion::new(0.9996488, 0.0018504566, 0.022782495, 0.013409463),
        Quaternion::new(1.0, 0.0, 0.0, 0.0), // Identity quaternion
        Quaternion::new(0.54127645, -0.546723, -0.46074906, -0.44252017),
        Quaternion::new(0.9802945, -0.16726136, 0.0789587, -0.06936778),
        Quaternion::new(0.99794674, 0.018492563, -0.013192348, -0.05988611),
        Quaternion::new(0.9973939, -0.003327809, 0.028225154, 0.066315144),
        Quaternion::new(0.9991947, 0.0, 0.0, -0.040125635),
        Quaternion::new(0.5501435, -0.5166922, -0.4298879, -0.49554786),
        Quaternion::new(0.9904201, -0.058696117, 0.10181952, -0.072495356),
        Quaternion::new(0.999545, -0.0022397265, -0.0000039300317, -0.030081047),
        Quaternion::new(0.9991019, -0.00072132144, 0.012692659, -0.040420394),
        Quaternion::new(1.0, 0.0, 0.0, 0.0), // Identity quaternion
        Quaternion::new(0.52394, -0.5269183, -0.32674035, -0.5840246),
        Quaternion::new(0.9866093, -0.059614867, 0.13516304, -0.06913207),
        Quaternion::new(0.99431664, 0.0018961236, 0.00013150928, -0.10644623),
        Quaternion::new(0.99593055, -0.00201019, 0.052079126, 0.073525675),
        Quaternion::new(1.0, 0.0, 0.0, 0.0), // Identity quaternion
        Quaternion::new(0.73723847, 0.20274544, 0.59426665, 0.2494411),
        Quaternion::new(-0.29033053, 0.6235274, -0.66380864, -0.29373443),
        Quaternion::new(-0.18704711, 0.6780625, -0.6592852, -0.26568344),
        Quaternion::new(-0.18303718, 0.7367927, -0.6347571, -0.14393571),
        Quaternion::new(-0.0036594148, 0.7584072, -0.6393418, -0.12667806),
    ];

    Pose {
        bone_rotations: rotations,
        bone_positions: positions,
    }
}

/// Returns the pointing pose for the right hand
pub fn point_right_hand() -> Pose {
    let positions = vec![
        Vector3::new(0.0, 0.0, 0.0), // Wrist
        Vector3::new(-0.034037687, 0.03650266, 0.16472164),
        Vector3::new(-0.016305087, 0.027528726, 0.017799662),
        Vector3::new(0.040405963, -0.000000051561553, 0.000000045447194),
        Vector3::new(0.032516792, -0.000000051137583, -0.000000012933195),
        Vector3::new(0.030463902, 0.00000016269207, 0.0000000792839),
        Vector3::new(0.0038021489, 0.021514187, 0.012803366),
        Vector3::new(0.074204385, 0.005002201, -0.00023377323),
        Vector3::new(0.043286677, 0.000000059333324, 0.00000018320057),
        Vector3::new(0.028275194, -0.00000009297885, -0.00000012653295),
        Vector3::new(0.022821384, -0.00000014365155, 0.00000007651614),
        Vector3::new(0.005786922, 0.0068064053, 0.016533904),
        Vector3::new(0.07095288, -0.00077883265, -0.000997186),
        Vector3::new(0.043108486, -0.00000009950596, -0.0000000067041825),
        Vector3::new(0.03326598, -0.000000017544496, -0.000000020628962),
        Vector3::new(0.025892371, 0.00000009984198, -0.0000000020352908),
        Vector3::new(0.004123044, -0.0068582613, 0.016562859),
        Vector3::new(0.06587581, -0.0017857892, -0.00069344096),
        Vector3::new(0.040331207, -0.00000009449958, -0.00000002273692),
        Vector3::new(0.028488781, 0.000000101152565, 0.000000045493586),
        Vector3::new(0.022430236, 0.00000010846127, -0.000000017428562),
        Vector3::new(0.0011314574, -0.019294508, 0.01542875),
        Vector3::new(0.0628784, -0.0028440945, -0.0003315112),
        Vector3::new(0.029874247, -0.000000034247638, -0.00000009126629),
        Vector3::new(0.017978692, -0.0000000028448923, -0.00000020797508),
        Vector3::new(0.01801794, -0.0000000200012, 0.0000000659746),
        Vector3::new(0.019716311, 0.002801723, 0.093936935),
        Vector3::new(-0.0075385696, 0.01764465, 0.10240429),
        Vector3::new(-0.0031984635, 0.0072115273, 0.11665362),
        Vector3::new(0.000026269245, -0.007118772, 0.13072418),
        Vector3::new(-0.0018780098, -0.02256182, 0.14003526),
    ];

    let rotations = vec![
        Quaternion::new(-0.00000004371139, -6.123234e-17, 1.0, 6.123234e-17),
        Quaternion::new(-0.055146642, -0.078608155, -0.92027926, 0.3792963),
        Quaternion::new(0.4622721, -0.060760066, -0.79196125, 0.3942209),
        Quaternion::new(0.933373, -0.005047277, 0.083810456, -0.34894884),
        Quaternion::new(0.98860765, 0.00009335017, -0.0014032124, -0.15050922),
        Quaternion::new(1.0, 0.0, 0.0, 0.0), // Identity quaternion
        Quaternion::new(0.39517453, -0.6173145, -0.44918522, -0.5108743),
        Quaternion::new(0.9906205, 0.045986902, 0.11017035, -0.06647379),
        Quaternion::new(0.9954967, 0.09205483, 0.00094662595, -0.022614187),
        Quaternion::new(0.9994967, 0.010468128, 0.027353302, 0.0121929655),
        Quaternion::new(1.0, 0.0, 0.0, 0.0), // Identity quaternion
        Quaternion::new(0.522315, -0.5142028, -0.4836996, -0.47834843),
        Quaternion::new(0.79466695, -0.10267462, -0.037405714, -0.59712917),
        Quaternion::new(0.7186201, -0.0031541286, -0.0979462, -0.6884634),
        Quaternion::new(0.6548623, -0.06366954, 0.00036316764, -0.7530614),
        Quaternion::new(0.9991947, 0.0, 0.0, -0.040125635),
        Quaternion::new(0.523374, -0.489609, -0.46399677, -0.52064353),
        Quaternion::new(0.7758391, -0.08626322, 0.022599243, -0.6245973),
        Quaternion::new(0.7426307, -0.0049873046, -0.039519195, -0.66851556),
        Quaternion::new(0.62664175, -0.027121458, -0.005438834, -0.7788164),
        Quaternion::new(1.0, 0.0, 0.0, 0.0), // Identity quaternion
        Quaternion::new(0.47783276, -0.47976637, -0.37993452, -0.63019824),
        Quaternion::new(0.742853, -0.09135258, 0.06652915, -0.6598471),
        Quaternion::new(0.77603596, 0.0072799353, 0.037179545, -0.62954974),
        Quaternion::new(0.6767216, -0.008087321, -0.003009417, -0.7361885),
        Quaternion::new(1.0, 0.0, 0.0, 0.0), // Identity quaternion
        Quaternion::new(0.33249632, -0.54886997, 0.1177861, -0.7578353),
        Quaternion::new(-0.114980996, 0.13243657, -0.8730836, -0.45493412),
        Quaternion::new(-0.019245595, 0.17098099, -0.92266804, -0.34507802),
        Quaternion::new(-0.064137466, 0.15011512, -0.952169, -0.25831383),
        Quaternion::new(-0.0037347008, 0.07684197, -0.97957754, -0.18576658),
    ];

    Pose {
        bone_positions: positions,
        bone_rotations: rotations,
    }
}

/// Returns the fist pose for the right hand (transcribed from the SteamVR
/// Unity plugin's fallback_fist reference pose)
pub fn fist_right_hand() -> Pose {
    let positions = vec![
        Vector3::new(-0.0, 0.0, 0.0),
        Vector3::new(-0.034037687, 0.03650266, 0.16472164),
        Vector3::new(-0.016305087, 0.027528726, 0.017799662),
        Vector3::new(0.040405963, -5.1561553e-08, 4.5447194e-08),
        Vector3::new(0.032516792, -5.1137583e-08, -1.2933195e-08),
        Vector3::new(0.030463902, 1.6269207e-07, 7.92839e-08),
        Vector3::new(0.0038021489, 0.021514187, 0.012803366),
        Vector3::new(0.074204385, 0.005002201, -0.00023377323),
        Vector3::new(0.043286677, 5.9333324e-08, 1.8320057e-07),
        Vector3::new(0.028275194, -9.297885e-08, -1.2653295e-07),
        Vector3::new(0.022821384, -1.4365155e-07, 7.651614e-08),
        Vector3::new(0.005786922, 0.0068064053, 0.016533904),
        Vector3::new(0.07095288, -0.00077883265, -0.000997186),
        Vector3::new(0.043108486, -9.950596e-08, -6.7041825e-09),
        Vector3::new(0.03326598, -1.7544496e-08, -2.0628962e-08),
        Vector3::new(0.025892371, 9.984198e-08, -2.0352908e-09),
        Vector3::new(0.004123044, -0.0068582613, 0.016562859),
        Vector3::new(0.06587581, -0.0017857892, -0.00069344096),
        Vector3::new(0.040331207, -9.449958e-08, -2.273692e-08),
        Vector3::new(0.028488781, 1.01152565e-07, 4.5493586e-08),
        Vector3::new(0.022430236, 1.0846127e-07, -1.7428562e-08),
        Vector3::new(0.0011314574, -0.019294508, 0.01542875),
        Vector3::new(0.0628784, -0.0028440945, -0.0003315112),
        Vector3::new(0.029874247, -3.4247638e-08, -9.126629e-08),
        Vector3::new(0.017978692, -2.8448923e-09, -2.0797508e-07),
        Vector3::new(0.01801794, -2.00012e-08, 6.59746e-08),
        Vector3::new(0.019716311, 0.002801723, 0.093936935),
        Vector3::new(-0.0075385696, 0.01764465, 0.10240429),
        Vector3::new(-0.0031984635, 0.0072115273, 0.11665362),
        Vector3::new(2.6269245e-05, -0.007118772, 0.13072418),
        Vector3::new(-0.0018780098, -0.02256182, 0.14003526),
    ];

    let rotations = vec![
        Quaternion::new(-4.371139e-08, -6.123234e-17, 1.0, 6.123234e-17),
        Quaternion::new(-0.055146642, -0.078608155, -0.92027926, 0.3792963),
        Quaternion::new(0.48333195, -0.2257035, -0.836342, 0.12641343),
        Quaternion::new(0.89433527, -0.01330204, 0.0829018, -0.43944824),
        Quaternion::new(0.80864674, 0.00072834245, -0.0012028969, -0.58829284),
        Quaternion::new(1.0, -1.3877788e-17, -1.3877788e-17, -5.551115e-17),
        Quaternion::new(0.39517453, -0.6173145, -0.44918522, -0.5108743),
        Quaternion::new(0.67689514, -0.041852362, 0.11180638, -0.72633374),
        Quaternion::new(0.56458294, -0.0005700487, 0.115204416, -0.81729656),
        Quaternion::new(0.7452787, -0.010756178, 0.027241308, -0.66610956),
        Quaternion::new(1.0, 6.938894e-18, 1.9428903e-16, -1.348151e-33),
        Quaternion::new(0.522315, -0.5142028, -0.4836996, -0.47834843),
        Quaternion::new(0.68225396, -0.09487112, -0.05422859, -0.7229027),
        Quaternion::new(0.6382125, 0.0076794685, -0.09769542, -0.7635977),
        Quaternion::new(0.6548623, -0.06366954, 0.00036316764, -0.7530614),
        Quaternion::new(0.9991947, 1.1639192e-17, -5.602331e-17, -0.040125635),
        Quaternion::new(0.523374, -0.489609, -0.46399677, -0.52064353),
        Quaternion::new(0.7000152, -0.088269405, 0.012672794, -0.7085384),
        Quaternion::new(0.66427904, -0.0005935501, -0.039828163, -0.74642265),
        Quaternion::new(0.62664175, -0.027121458, -0.005438834, -0.7788164),
        Quaternion::new(1.0, 6.938894e-18, -9.62965e-35, -1.3877788e-17),
        Quaternion::new(0.47783276, -0.47976637, -0.37993452, -0.63019824),
        Quaternion::new(0.7144873, -0.094065815, 0.062634066, -0.69046116),
        Quaternion::new(0.7017823, 0.00313052, 0.03775632, -0.7113834),
        Quaternion::new(0.6767216, -0.008087321, -0.003009417, -0.7361885),
        Quaternion::new(1.0, 0.0, 0.0, 1.9081958e-17),
        Quaternion::new(0.33249632, -0.54886997, 0.1177861, -0.7578353),
        Quaternion::new(-0.114980996, 0.13243657, -0.8730836, -0.45493412),
        Quaternion::new(-0.019245595, 0.17098099, -0.92266804, -0.34507802),
        Quaternion::new(-0.064137466, 0.15011512, -0.952169, -0.25831383),
        Quaternion::new(-0.0037347008, 0.07684197, -0.97957754, -0.18576658),
    ];

    Pose {
        bone_positions: positions,
        bone_rotations: rotations,
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use cgmath::Point3;
    use collision::Aabb3;
    use dark::importers::skeleton_from_glb_bytes;

    fn glove_model() -> GlbModel {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../assets/vr_glove_model.glb");
        let bytes = std::fs::read(path).expect("read vr_glove_model.glb");
        let skeleton = skeleton_from_glb_bytes(&bytes).expect("glove GLB has a skeleton");
        let unit = Aabb3::new(Point3::new(0.0, 0.0, 0.0), Point3::new(0.0, 0.0, 0.0));
        GlbModel::new(Vec::new(), unit, skeleton)
    }

    /// World-space joint positions (the Root joint carries the exporter's
    /// x100 unit scale, so these are in meters).
    fn joint_positions(model: &mut GlbModel) -> Vec<Vector3<f32>> {
        (0..model.skeleton().joint_count())
            .map(|joint| {
                let node = model.skeleton().node_index_for_joint(joint).unwrap();
                model.get_global_transform(node).unwrap().w.truncate()
            })
            .collect()
    }

    fn parent_joints(model: &GlbModel) -> Vec<Option<usize>> {
        (0..model.skeleton().joint_count())
            .map(|joint| {
                let node = model.skeleton().node_index_for_joint(joint).unwrap();
                model
                    .skeleton()
                    .get_node(node)
                    .and_then(|n| n.parent_index)
                    .and_then(|p| model.skeleton().joint_index_for_node(p))
            })
            .collect()
    }

    fn segment_lengths(positions: &[Vector3<f32>], parents: &[Option<usize>]) -> Vec<f32> {
        parents
            .iter()
            .enumerate()
            .map(|(joint, parent)| match parent {
                Some(p) => (positions[joint] - positions[*p]).magnitude(),
                None => 0.0,
            })
            .collect()
    }

    /// Posing must never stretch bones: every parent->child segment keeps its
    /// bind length under every pose.
    #[test]
    fn posing_preserves_bone_lengths() {
        let mut model = glove_model();
        let parents = parent_joints(&model);
        let bind_lengths = segment_lengths(&joint_positions(&mut model), &parents);
        let retarget = HandPoseRetarget::for_right_glove(model.skeleton());

        for (name, pose) in [
            ("open", open_right_hand()),
            ("point", point_right_hand()),
            ("fist", fist_right_hand()),
        ] {
            retarget.apply(&pose, &mut model);
            let lengths = segment_lengths(&joint_positions(&mut model), &parents);
            for (joint, (bind, posed)) in bind_lengths.iter().zip(&lengths).enumerate() {
                assert!(
                    (bind - posed).abs() < 1e-4,
                    "{name}: joint {joint} segment length changed: bind {bind} vs posed {posed}"
                );
            }
        }
    }

    /// The open-hand reference pose IS the GLB's bind pose, so applying it
    /// must reproduce the bind joint positions exactly.
    #[test]
    fn open_pose_matches_bind_pose() {
        let mut model = glove_model();
        let bind = joint_positions(&mut model);
        let retarget = HandPoseRetarget::for_right_glove(model.skeleton());

        retarget.apply(&open_right_hand(), &mut model);
        let posed = joint_positions(&mut model);
        for (joint, (b, p)) in bind.iter().zip(&posed).enumerate() {
            let err = (b - p).magnitude();
            assert!(
                err < 1e-4,
                "joint {joint} moved {err} m under the open (bind) pose"
            );
        }
    }

    /// The fist pose must actually curl the fingers: fingertips end up much
    /// closer to the wrist than in the open pose.
    #[test]
    fn fist_pose_curls_fingers() {
        let mut model = glove_model();
        let retarget = HandPoseRetarget::for_right_glove(model.skeleton());
        let wrist = joint_positions(&mut model)[1];

        // SteamVR joint order: tips are 10 (index), 15 (middle), 20 (ring), 25 (pinky)
        for tip in [10usize, 15, 20, 25] {
            let open_dist = (joint_positions(&mut model)[tip] - wrist).magnitude();
            retarget.apply(&fist_right_hand(), &mut model);
            let fist_dist = (joint_positions(&mut model)[tip] - wrist).magnitude();
            assert!(
                fist_dist < open_dist - 0.05,
                "tip joint {tip}: fist ({fist_dist} m) should be >5cm closer to wrist than open ({open_dist} m)"
            );
            retarget.apply(&open_right_hand(), &mut model);
        }
    }

    /// Blending open->fist matches the endpoints exactly and produces a
    /// genuine intermediate pose at the midpoint.
    #[test]
    fn blend_interpolates_between_poses() {
        let mut model = glove_model();
        let retarget = HandPoseRetarget::for_right_glove(model.skeleton());
        let open = open_right_hand();
        let fist = fist_right_hand();

        let tip = 15; // middle fingertip
        retarget.apply(&open, &mut model);
        let open_tip = joint_positions(&mut model)[tip];
        retarget.apply(&fist, &mut model);
        let fist_tip = joint_positions(&mut model)[tip];

        retarget.apply(&open.blend(&fist, 0.0), &mut model);
        assert!((joint_positions(&mut model)[tip] - open_tip).magnitude() < 1e-5);
        retarget.apply(&open.blend(&fist, 1.0), &mut model);
        assert!((joint_positions(&mut model)[tip] - fist_tip).magnitude() < 1e-5);

        retarget.apply(&open.blend(&fist, 0.5), &mut model);
        let mid_tip = joint_positions(&mut model)[tip];
        let from_open = (mid_tip - open_tip).magnitude();
        let from_fist = (mid_tip - fist_tip).magnitude();
        assert!(
            from_open > 0.01 && from_fist > 0.01,
            "midpoint should be a real intermediate (from_open {from_open} m, from_fist {from_fist} m)"
        );
    }

    /// Per-finger blending curls only the selected finger: with index=1.0 the
    /// index tip matches the fist, while the middle tip stays at the open pose.
    #[test]
    fn per_finger_blend_curls_only_selected_finger() {
        let mut model = glove_model();
        let retarget = HandPoseRetarget::for_right_glove(model.skeleton());
        let open = open_right_hand();
        let fist = fist_right_hand();

        let index_tip = 10;
        let middle_tip = 15;
        retarget.apply(&open, &mut model);
        let open_positions = joint_positions(&mut model);
        retarget.apply(&fist, &mut model);
        let fist_index_tip = joint_positions(&mut model)[index_tip];

        let trigger_pull = open.blend_per_finger(
            &fist,
            &FingerAmounts {
                index: 1.0,
                ..Default::default()
            },
        );
        retarget.apply(&trigger_pull, &mut model);
        let positions = joint_positions(&mut model);

        let index_err = (positions[index_tip] - fist_index_tip).magnitude();
        let middle_err = (positions[middle_tip] - open_positions[middle_tip]).magnitude();
        assert!(
            index_err < 1e-5,
            "index tip should match the fist (err {index_err} m)"
        );
        assert!(
            middle_err < 1e-5,
            "middle tip should stay at the open pose (err {middle_err} m)"
        );
    }
}
