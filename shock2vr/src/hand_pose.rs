use cgmath::{Matrix4, Quaternion, SquareMatrix, Vector3};
use std::collections::HashMap;

/// Joint indices for hand bones
pub mod joint_indices {
    // Root and wrist
    pub const WRIST: usize = 0;
    pub const FOREARM_STUB: usize = 1;

    // Thumb (2-5)
    pub const THUMB_METACARPAL: usize = 2;
    pub const THUMB_PROXIMAL: usize = 3;
    pub const THUMB_INTERMEDIATE: usize = 4;
    pub const THUMB_DISTAL: usize = 5;

    // Index finger (6-10)
    pub const INDEX_METACARPAL: usize = 6;
    pub const INDEX_PROXIMAL: usize = 7;
    pub const INDEX_INTERMEDIATE: usize = 8;
    pub const INDEX_DISTAL: usize = 9;
    pub const INDEX_TIP: usize = 10;

    // Middle finger (11-15)
    pub const MIDDLE_METACARPAL: usize = 11;
    pub const MIDDLE_PROXIMAL: usize = 12;
    pub const MIDDLE_INTERMEDIATE: usize = 13;
    pub const MIDDLE_DISTAL: usize = 14;
    pub const MIDDLE_TIP: usize = 15;

    // Ring finger (16-20)
    pub const RING_METACARPAL: usize = 16;
    pub const RING_PROXIMAL: usize = 17;
    pub const RING_INTERMEDIATE: usize = 18;
    pub const RING_DISTAL: usize = 19;
    pub const RING_TIP: usize = 20;

    // Pinky finger (21-25)
    pub const PINKY_METACARPAL: usize = 21;
    pub const PINKY_PROXIMAL: usize = 22;
    pub const PINKY_INTERMEDIATE: usize = 23;
    pub const PINKY_DISTAL: usize = 24;
    pub const PINKY_TIP: usize = 25;

    // Additional thumb bones (26-30)
    pub const THUMB_AUX: usize = 26;
    pub const INDEX_AUX: usize = 27;
    pub const MIDDLE_AUX: usize = 28;
    pub const RING_AUX: usize = 29;
    pub const PINKY_AUX: usize = 30;
}

/// Creates a map of joint relationships where key is child joint index and value is parent joint index
pub fn joint_relationships() -> HashMap<usize, usize> {
    use joint_indices::*;

    let mut relationships = HashMap::new();

    // Forearm stub connects to wrist
    relationships.insert(FOREARM_STUB, WRIST);

    // Thumb chain (starts from wrist)
    relationships.insert(THUMB_METACARPAL, WRIST);
    relationships.insert(THUMB_PROXIMAL, THUMB_METACARPAL);
    relationships.insert(THUMB_INTERMEDIATE, THUMB_PROXIMAL);
    relationships.insert(THUMB_DISTAL, THUMB_INTERMEDIATE);

    // Index finger chain (starts from wrist)
    relationships.insert(INDEX_METACARPAL, WRIST);
    relationships.insert(INDEX_PROXIMAL, INDEX_METACARPAL);
    relationships.insert(INDEX_INTERMEDIATE, INDEX_PROXIMAL);
    relationships.insert(INDEX_DISTAL, INDEX_INTERMEDIATE);
    relationships.insert(INDEX_TIP, INDEX_DISTAL);

    // Middle finger chain (starts from wrist)
    relationships.insert(MIDDLE_METACARPAL, WRIST);
    relationships.insert(MIDDLE_PROXIMAL, MIDDLE_METACARPAL);
    relationships.insert(MIDDLE_INTERMEDIATE, MIDDLE_PROXIMAL);
    relationships.insert(MIDDLE_DISTAL, MIDDLE_INTERMEDIATE);
    relationships.insert(MIDDLE_TIP, MIDDLE_DISTAL);

    // Ring finger chain (starts from wrist)
    relationships.insert(RING_METACARPAL, WRIST);
    relationships.insert(RING_PROXIMAL, RING_METACARPAL);
    relationships.insert(RING_INTERMEDIATE, RING_PROXIMAL);
    relationships.insert(RING_DISTAL, RING_INTERMEDIATE);
    relationships.insert(RING_TIP, RING_DISTAL);

    // Pinky finger chain (starts from wrist)
    relationships.insert(PINKY_METACARPAL, WRIST);
    relationships.insert(PINKY_PROXIMAL, PINKY_METACARPAL);
    relationships.insert(PINKY_INTERMEDIATE, PINKY_PROXIMAL);
    relationships.insert(PINKY_DISTAL, PINKY_INTERMEDIATE);
    relationships.insert(PINKY_TIP, PINKY_DISTAL);

    // Additional thumb bones (assuming they chain from thumb tip)
    relationships.insert(THUMB_AUX, THUMB_DISTAL);
    relationships.insert(RING_AUX, RING_DISTAL);
    relationships.insert(PINKY_AUX, PINKY_DISTAL);
    relationships.insert(INDEX_AUX, INDEX_DISTAL);
    relationships.insert(MIDDLE_AUX, MIDDLE_DISTAL);

    relationships
}

/// Maps pose indices to SteamVR skeletal joint names based on the handFingers mask
/// This mapping reflects the order bones appear in the Unity pose data
pub fn pose_joint_map() -> Vec<&'static str> {
    vec![
        "wrist_r",                // 0
        "finger_thumb_0_r",       // 1 - forearm stub -> thumb metacarpal
        "finger_thumb_1_r",       // 2 - thumb metacarpal
        "finger_thumb_2_r",       // 3 - thumb proximal
        "finger_thumb_r_end",     // 4 - thumb intermediate
        "finger_thumb_r_end_end", // 5 - thumb distal
        "finger_index_meta_r",    // 6 - index metacarpal
        "finger_index_0_r",       // 7 - index proximal
        "finger_index_1_r",       // 8 - index intermediate
        "finger_index_2_r",       // 9 - index distal
        "finger_index_r_end",     // 10 - index tip
        "finger_middle_meta_r",   // 11 - middle metacarpal
        "finger_middle_0_r",      // 12 - middle proximal
        "finger_middle_1_r",      // 13 - middle intermediate
        "finger_middle_2_r",      // 14 - middle distal
        "finger_middle_r_end",    // 15 - middle tip
        "finger_ring_meta_r",     // 16 - ring metacarpal
        "finger_ring_0_r",        // 17 - ring proximal
        "finger_ring_1_r",        // 18 - ring intermediate
        "finger_ring_2_r",        // 19 - ring distal
        "finger_ring_r_end",      // 20 - ring tip
        "finger_pinky_meta_r",    // 21 - pinky metacarpal
        "finger_pinky_0_r",       // 22 - pinky proximal
        "finger_pinky_1_r",       // 23 - pinky intermediate
        "finger_pinky_2_r",       // 24 - pinky distal
        "finger_pinky_r_end",     // 25 - pinky tip
        "finger_thumb_r_aux",     // 26 - thumb aux bones
        "finger_index_r_aux",     // 27
        "finger_middle_r_aux",    // 28
        "finger_ring_r_aux",      // 29
        "finger_pinky_r_aux",     // 30
    ]
}

/// Represents a hand pose with bone positions and rotations
#[derive(Debug, Clone)]
pub struct Pose {
    /// Bone positions in pose space
    pub bone_positions: Vec<Vector3<f32>>,
    /// Bone rotations as quaternions
    pub bone_rotations: Vec<Quaternion<f32>>,
}

impl Pose {
    /// Creates a new pose from lists of bone positions and rotations
    pub fn new(bone_positions: Vec<Vector3<f32>>, bone_rotations: Vec<Quaternion<f32>>) -> Self {
        Self {
            bone_positions,
            bone_rotations,
        }
    }

    /// Computes global transforms for all bones by accumulating through the hierarchy
    /// Returns (global_positions, global_transforms) where positions are world space
    /// and transforms are full 4x4 matrices for each bone
    pub fn compute_global_transforms(&self) -> (Vec<Vector3<f32>>, Vec<Matrix4<f32>>) {
        let relationships = joint_relationships();
        let mut global_positions = vec![Vector3::new(0.0, 0.0, 0.0); self.bone_positions.len()];
        let mut global_transforms = vec![Matrix4::identity(); self.bone_positions.len()];

        // Build a map from child to parent for quick lookup
        let mut child_to_parent = HashMap::new();
        for (&child, &parent) in &relationships {
            child_to_parent.insert(child, parent);
        }

        // Process bones in dependency order using a recursive approach
        fn compute_bone_transform(
            bone_index: usize,
            pose: &Pose,
            child_to_parent: &HashMap<usize, usize>,
            global_positions: &mut Vec<Vector3<f32>>,
            global_transforms: &mut Vec<Matrix4<f32>>,
            computed: &mut Vec<bool>,
        ) {
            if computed[bone_index] {
                return;
            }

            // First, ensure parent is computed
            if let Some(&parent_index) = child_to_parent.get(&bone_index) {
                if parent_index < pose.bone_positions.len() {
                    compute_bone_transform(
                        parent_index,
                        pose,
                        child_to_parent,
                        global_positions,
                        global_transforms,
                        computed,
                    );

                    // Create local transform from position and rotation
                    let local_position = pose.bone_positions[bone_index];
                    let local_rotation = pose.bone_rotations[bone_index];
                    let local_transform =
                        Matrix4::from_translation(local_position) * Matrix4::from(local_rotation);

                    // Multiply by parent's global transform
                    let parent_global = global_transforms[parent_index];
                    global_transforms[bone_index] = parent_global * local_transform;

                    // Extract global position from the transform matrix
                    global_positions[bone_index] = global_transforms[bone_index].w.truncate();
                } else {
                    // Invalid parent index, treat as root
                    let local_position = pose.bone_positions[bone_index];
                    let local_rotation = pose.bone_rotations[bone_index];
                    global_transforms[bone_index] =
                        Matrix4::from_translation(local_position) * Matrix4::from(local_rotation);
                    global_positions[bone_index] = local_position;
                }
            } else {
                // Root bone - use local transform as global
                let local_position = pose.bone_positions[bone_index];
                let local_rotation = pose.bone_rotations[bone_index];
                global_transforms[bone_index] =
                    Matrix4::from_translation(local_position) * Matrix4::from(local_rotation);
                global_positions[bone_index] = local_position;
            }

            computed[bone_index] = true;
        }

        let mut computed = vec![false; self.bone_positions.len()];

        // Compute transforms for all bones
        for bone_index in 0..self.bone_positions.len() {
            compute_bone_transform(
                bone_index,
                self,
                &child_to_parent,
                &mut global_positions,
                &mut global_transforms,
                &mut computed,
            );
        }

        (global_positions, global_transforms)
    }

    /// Returns global bone positions transformed through the hierarchy
    pub fn global_bone_positions(&self) -> Vec<Vector3<f32>> {
        self.compute_global_transforms().0
    }

    /// Returns global bone transforms as 4x4 matrices
    pub fn global_bone_transforms(&self) -> Vec<Matrix4<f32>> {
        self.compute_global_transforms().1
    }
}

/// Returns the open hand pose for the right hand
pub fn open_right_hand() -> Pose {
    let positions = vec![
        Vector3::new(0.0, 0.0, 0.0),
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

    Pose::new(positions, rotations)
}

/// Returns the pointing pose for the right hand
pub fn point_right_hand() -> Pose {
    let positions = vec![
        Vector3::new(0.0, 0.0, 0.0),
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

    Pose::new(positions, rotations)
}
