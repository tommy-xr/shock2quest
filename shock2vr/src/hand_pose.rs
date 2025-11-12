use cgmath::Vector3;
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
    pub const THUMB_AUX_1: usize = 26;
    pub const THUMB_AUX_2: usize = 27;
    pub const THUMB_AUX_3: usize = 28;
    pub const THUMB_AUX_4: usize = 29;
    pub const THUMB_AUX_5: usize = 30;
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
    relationships.insert(THUMB_AUX_1, THUMB_DISTAL);
    relationships.insert(THUMB_AUX_2, THUMB_AUX_1);
    relationships.insert(THUMB_AUX_3, THUMB_AUX_2);
    relationships.insert(THUMB_AUX_4, THUMB_AUX_3);
    relationships.insert(THUMB_AUX_5, THUMB_AUX_4);

    relationships
}

/// Represents a hand pose with bone positions
#[derive(Debug, Clone)]
pub struct Pose {
    /// Bone positions in model space
    pub bone_positions: Vec<Vector3<f32>>,
}

impl Pose {
    /// Creates a new pose from a list of bone positions
    pub fn new(bone_positions: Vec<Vector3<f32>>) -> Self {
        Self { bone_positions }
    }
}

/// Returns the pointing pose for the left hand
pub fn point_left_hand() -> Pose {
    Pose::new(vec![
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
    ])
}

/// Returns the pointing pose for the right hand
pub fn point_right_hand() -> Pose {
    Pose::new(vec![
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
    ])
}