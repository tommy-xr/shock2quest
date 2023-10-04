use std::collections::{HashMap, HashSet};

use cgmath::{vec3, Angle, Deg, Quaternion, Rotation3, Vector3};
use once_cell::sync::Lazy;

struct VRHandModelPerHandAdjustments {
    offset: Vector3<f32>,
    rotation: Quaternion<f32>,
    scale: Vector3<f32>,
}

impl VRHandModelPerHandAdjustments {
    pub fn new() -> VRHandModelPerHandAdjustments {
        VRHandModelPerHandAdjustments {
            offset: Vector3::new(0.0, 0.0, 0.0),
            rotation: Quaternion::new(0.0, 0.0, 0.0, 1.0),
            scale: Vector3::new(1.0, 1.0, 1.0),
        }
    }

    pub fn rotate_y(self, angle: Deg<f32>) -> VRHandModelPerHandAdjustments {
        VRHandModelPerHandAdjustments {
            rotation: self.rotation * Quaternion::from_angle_y(angle),
            ..self
        }
    }

    pub fn flip_x(self) -> VRHandModelPerHandAdjustments {
        VRHandModelPerHandAdjustments {
            scale: vec3(-self.scale.x, self.scale.y, self.scale.z),
            ..self
        }
    }

    pub fn flip_z(self) -> VRHandModelPerHandAdjustments {
        VRHandModelPerHandAdjustments {
            scale: vec3(self.scale.x, self.scale.y, -self.scale.z),
            ..self
        }
    }
}

struct VRHandModelAdjustments {
    left_hand: VRHandModelPerHandAdjustments,
    right_hand: VRHandModelPerHandAdjustments,
}

impl VRHandModelAdjustments {
    pub fn new(
        left_hand: VRHandModelPerHandAdjustments,
        right_hand: VRHandModelPerHandAdjustments,
    ) -> VRHandModelAdjustments {
        VRHandModelAdjustments {
            left_hand,
            right_hand,
        }
    }
}

static HAND_MODEL_POSITIONING: Lazy<HashMap<&str, VRHandModelAdjustments>> = Lazy::new(|| {
    let mut map = HashMap::new();

    let held_weapon_right = VRHandModelPerHandAdjustments::new().rotate_y(Deg(90.0));
    let held_weapon_left = held_weapon_right.flip_x();

    // ATEK
    map.insert(
        "atek_h",
        VRHandModelAdjustments::new(held_weapon_left, held_weapon_right),
    );

    // AMP
    map.insert(
        "amp_h",
        VRHandModelAdjustments::new(held_weapon_left, held_weapon_right),
    );

    // LASEHAND
    map.insert(
        "lasehand",
        VRHandModelAdjustments::new(held_weapon_left, held_weapon_right),
    );

    map
});

pub fn is_allowed_hand_model(model_name: &str) -> bool {
    HAND_MODEL_POSITIONING.contains_key(model_name)
}

pub fn get_vr_hand_model_adjustments(
    model_name: &str,
    is_left_hand: bool,
) -> VRHandModelPerHandAdjustments {
    let maybe_adjustments = HAND_MODEL_POSITIONING.get(model_name);

    if maybe_adjustments.is_none() {
        return VRHandModelPerHandAdjustments::new();
    }

    let adjustments = maybe_adjustments.unwrap();

    if is_left_hand {
        adjustments.left_hand
    } else {
        adjustments.right_hand
    }
}
