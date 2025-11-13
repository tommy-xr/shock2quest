use cgmath::{Deg, Matrix4, Quaternion, Rotation3};
use num::Zero;
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
}

/// Creates a map of joint relationships where key is child joint index and value is parent joint index
pub fn joint_relationships() -> HashMap<usize, usize> {
    use joint_indices::*;

    let mut relationships = HashMap::new();

    // Forearm stub connects to wrist
    relationships.insert(WRIST, FOREARM_STUB);

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

    relationships
}

/// Represents a hand pose with bone positions and rotations
#[derive(Debug, Clone)]
pub struct Pose {
    /// Bone rotations as quaternions
    pub bone_rotations: Vec<Quaternion<f32>>,
}

impl Pose {
    /// Converts the pose to joint transforms suitable for skeleton overrides
    ///
    /// The skeleton system applies transforms as: parent_transform * animation_transform * local_transform
    /// where local_transform often includes translation (bone length).
    ///
    /// For proper joint rotation without bone stretching, we need to provide transforms that
    /// account for this multiplication order.
    pub fn to_joint_transforms(&self) -> std::collections::HashMap<u32, Matrix4<f32>> {
        use std::collections::HashMap;

        let mut joint_transforms = HashMap::new();
        let no_rotation = Quaternion::zero();

        for (bone_index, &rotation) in self.bone_rotations.iter().enumerate() {
            // Skip bones with no rotation (quaternion zero)
            if rotation != no_rotation {
                // For now, just apply the rotation directly
                // TODO: This may cause bone stretching because the rotation gets applied
                // to the bone's local translation. A proper fix would modify the skeleton
                // system to handle joint rotations correctly.
                let rotation_matrix = Matrix4::from(rotation);
                joint_transforms.insert(bone_index as u32, rotation_matrix);
            }
        }

        joint_transforms
    }

}

pub fn closed_fist_pose() -> Pose {
    let no_rotation = Quaternion::zero();
    let inward_rotation_1 = Quaternion::from_angle_x(Deg(20.0));  // Try X rotation for finger curl
    let inward_rotation_2 = Quaternion::from_angle_x(Deg(30.0));
    let inward_rotation_3 = Quaternion::from_angle_x(Deg(40.0));
    let inward_rotation_4 = Quaternion::from_angle_x(Deg(30.0));  // Smaller angles
    let inward_rotation_5 = Quaternion::from_angle_x(Deg(20.0));
    let bone_rotations: Vec<Quaternion<f32>> = vec![
        // Root and wrist
        no_rotation,
        no_rotation,
        // Thumb (2-5)
        no_rotation,
        no_rotation,
        no_rotation,
        no_rotation,
        // Index finger (6-10)
        inward_rotation_1,
        inward_rotation_2,
        inward_rotation_3,
        inward_rotation_4,
        inward_rotation_5,
        // Middle finger (11-15)
        inward_rotation_1,
        inward_rotation_2,
        inward_rotation_3,
        inward_rotation_4,
        inward_rotation_5,
        // Ring finger (16-20)
        inward_rotation_1,
        inward_rotation_2,
        inward_rotation_3,
        inward_rotation_4,
        inward_rotation_5,
        // Pinky finger (21-25)
        inward_rotation_1,
        inward_rotation_2,
        inward_rotation_3,
        inward_rotation_4,
        inward_rotation_5,
    ];

    Pose { bone_rotations }
}
