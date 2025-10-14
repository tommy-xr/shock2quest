use std::collections::HashMap;

use cgmath::{vec3, Deg, Quaternion, Rotation3, Vector3};
use dark::properties::PropModelName;
use once_cell::sync::Lazy;
use shipyard::{EntityId, Get, View, World};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Handedness {
    Left,
    Right,
}

#[derive(Clone, Debug)]
pub struct VRHandModelPerHandAdjustments {
    pub offset: Vector3<f32>,
    pub rotation: Quaternion<f32>,
    pub scale: Vector3<f32>,
}

impl VRHandModelPerHandAdjustments {
    pub fn new() -> VRHandModelPerHandAdjustments {
        VRHandModelPerHandAdjustments {
            offset: Vector3::new(0.0, 0.0, 0.0),
            rotation: Quaternion::new(1.0, 0.0, 0.0, 0.0),
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
}

#[derive(Clone)]
struct VRHandModelAdjustments {
    left_hand: VRHandModelPerHandAdjustments,
    right_hand: VRHandModelPerHandAdjustments,
    projectile_rotation: Quaternion<f32>,
}

impl VRHandModelAdjustments {
    pub fn new(
        left_hand: VRHandModelPerHandAdjustments,
        right_hand: VRHandModelPerHandAdjustments,
        projectile_rotation: Quaternion<f32>,
    ) -> VRHandModelAdjustments {
        VRHandModelAdjustments {
            left_hand,
            right_hand,
            projectile_rotation,
        }
    }

    pub fn with_projectile_rotation(
        self,
        projectile_rotation: Quaternion<f32>,
    ) -> VRHandModelAdjustments {
        VRHandModelAdjustments {
            projectile_rotation,
            ..self
        }
    }
}

static HAND_MODEL_POSITIONING: Lazy<HashMap<&str, VRHandModelAdjustments>> = Lazy::new(|| {
    let mut map = HashMap::new();

    let held_weapon_right = VRHandModelPerHandAdjustments::new().rotate_y(Deg(-90.0));
    let held_weapon_left = held_weapon_right.clone().flip_x();
    let held_weapon = VRHandModelAdjustments::new(
        held_weapon_left,
        held_weapon_right,
        Quaternion::from_angle_y(Deg(0.0)),
    );

    let held_item_hand = VRHandModelPerHandAdjustments::new().rotate_y(Deg(180.0));
    let held_item = VRHandModelAdjustments::new(
        held_item_hand.clone(),
        held_item_hand,
        Quaternion::from_angle_y(Deg(0.0)),
    );

    let default = VRHandModelAdjustments::new(
        VRHandModelPerHandAdjustments::new(),
        VRHandModelPerHandAdjustments::new(),
        Quaternion::from_angle_y(Deg(-180.0)),
    );

    // Hand model adjustments for VR
    // Specify overrides for particular models with how they should be oriented
    // relative ot the virtual hand
    let items = vec![
        // Weapons
        ("atek_h", held_weapon.clone()),
        ("amp_h", held_weapon.clone()),
        (
            "lasehand",
            held_weapon
                .clone()
                .with_projectile_rotation(Quaternion::from_angle_y(Deg(12.))),
        ),
        ("empgun", held_weapon.clone()),
        ("wrench_h", default.clone()),
        ("sg_w", held_weapon.clone()),
        // World items
        ("battery", held_item.clone()),
        ("batteryb", held_item.clone()),
        ("gameboy", held_item.clone()),
        ("gamecart", held_item.clone()),
        ("nanocan", held_item.clone()),
    ];

    items.iter().for_each(|(name, adjustments)| {
        map.insert(*name, adjustments.clone());
    });

    map
});

pub fn is_allowed_hand_model(model_name: &str) -> bool {
    HAND_MODEL_POSITIONING.contains_key(model_name)
}

pub fn get_vr_hand_model_adjustments_from_entity(
    entity_id: EntityId,
    world: &World,
    handedness: Handedness,
) -> VRHandModelPerHandAdjustments {
    let v_model_name = world.borrow::<View<PropModelName>>().unwrap();
    let maybe_model_name = v_model_name
        .get(entity_id)
        .map(|sz| sz.0.to_ascii_lowercase());

    if let Ok(model_name) = maybe_model_name {
        get_vr_hand_model_adjustments_from_model(&model_name, handedness)
    } else {
        VRHandModelPerHandAdjustments::new()
    }
}

pub fn get_projectile_rotation_from_entity(entity_id: EntityId, world: &World) -> Quaternion<f32> {
    let v_model_name = world.borrow::<View<PropModelName>>().unwrap();
    let maybe_model_name = v_model_name
        .get(entity_id)
        .map(|sz| sz.0.to_ascii_lowercase());

    if let Ok(model_name) = maybe_model_name {
        get_vr_projectile_rotation_from_model(&model_name)
    } else {
        Quaternion::from_angle_y(Deg(180.0))
    }
}

pub fn get_vr_hand_model_adjustments_from_model(
    model_name: &str,
    handedness: Handedness,
) -> VRHandModelPerHandAdjustments {
    let maybe_adjustments = HAND_MODEL_POSITIONING.get(model_name);

    if maybe_adjustments.is_none() {
        return VRHandModelPerHandAdjustments::new();
    }

    let adjustments = maybe_adjustments.unwrap();

    if handedness == Handedness::Left {
        adjustments.left_hand.clone()
    } else {
        adjustments.right_hand.clone()
    }
}

fn get_vr_projectile_rotation_from_model(model_name: &str) -> Quaternion<f32> {
    let maybe_adjustments = HAND_MODEL_POSITIONING.get(model_name);

    if maybe_adjustments.is_none() {
        return Quaternion::from_angle_y(Deg(180.0));
    }

    maybe_adjustments.unwrap().projectile_rotation
}
