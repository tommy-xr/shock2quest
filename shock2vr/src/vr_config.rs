use std::collections::HashMap;

use cgmath::{Deg, Quaternion, Rotation3, Vector3, vec3};
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
        // NOTE: the scale mirror is currently inert (SetPositionRotation
        // hardcodes scale 1), so the left hand holds the *unmirrored* model.
        // The offset must therefore NOT be mirrored - it seats the same
        // unmirrored geometry at the same hand-local point (verified against
        // left-hand grip screenshots).
        VRHandModelPerHandAdjustments {
            scale: vec3(-self.scale.x, self.scale.y, self.scale.z),
            ..self
        }
    }

    /// Hand-local translation (meters): +X toward the thumb side of the right
    /// hand, +Y up out of the back of the hand, -Z along the fingers.
    pub fn with_offset(self, offset: Vector3<f32>) -> VRHandModelPerHandAdjustments {
        VRHandModelPerHandAdjustments { offset, ..self }
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
        held_weapon_right.clone(),
        Quaternion::from_angle_y(Deg(0.0)),
    );

    // The wrench's long axis runs opposite the guns' after the -90 yaw (its
    // model is authored along X where guns are along Y), so it takes +90 and
    // slides toward its handle end
    let wrench_right = VRHandModelPerHandAdjustments::new()
        .rotate_y(Deg(90.0))
        .with_offset(vec3(0.0, 0.0, -0.4));

    // The Rapier, Crystal Shard and PsiSword have no world-model grip entry,
    // so wielding them must keep the unmapped default (see
    // `get_vr_hand_model_adjustments_from_model`) exactly.
    let melee_world_grip = VRHandModelAdjustments::new(
        VRHandModelPerHandAdjustments::new(),
        VRHandModelPerHandAdjustments::new(),
        Quaternion::from_angle_y(Deg(180.0)),
    );

    let held_item_hand = VRHandModelPerHandAdjustments::new().rotate_y(Deg(180.0));
    let held_item = VRHandModelAdjustments::new(
        held_item_hand.clone(),
        held_item_hand,
        Quaternion::from_angle_y(Deg(0.0)),
    );

    // Hand model adjustments for VR
    // Specify overrides for particular models with how they should be oriented
    // relative ot the virtual hand
    // A weapon whose left-hand placement is the right-hand adjustments
    // mirrored (flip_x)
    fn symmetric(right: VRHandModelPerHandAdjustments) -> VRHandModelAdjustments {
        VRHandModelAdjustments::new(
            right.clone().flip_x(),
            right,
            Quaternion::from_angle_y(Deg(0.0)),
        )
    }

    let items = vec![
        // Weapons - first-person hand models (_h). Used by flat's wield swap
        // (FLAT_WIELD_SWAP_MODELS) and, on a 25AE install, wielded directly in
        // VR (VR_25AE_VIEW_MODELS). The whole 25AE set is authored barrel
        // along -X, so one -90 yaw seats every gun; offsets are hand-fitted
        // per model to land the grip in the palm.
        (
            "atek_h",
            symmetric(held_weapon_right.clone().with_offset(vec3(0.0, 0.12, 0.10))),
        ),
        (
            "ar15_h",
            symmetric(held_weapon_right.clone().with_offset(vec3(0.0, 0.01, 0.17))),
        ),
        (
            "sg_h",
            symmetric(held_weapon_right.clone().with_offset(vec3(0.0, 0.04, 0.28))),
        ),
        (
            "empgun_h",
            symmetric(held_weapon_right.clone().with_offset(vec3(0.0, 0.01, 0.29))),
        ),
        (
            "gren_h",
            symmetric(
                held_weapon_right
                    .clone()
                    .with_offset(vec3(0.0, 0.11, -0.56)),
            ),
        ),
        (
            "sfg_h",
            symmetric(
                held_weapon_right
                    .clone()
                    .with_offset(vec3(0.0, 0.27, -0.21)),
            ),
        ),
        (
            "fsn_h",
            symmetric(
                held_weapon_right
                    .clone()
                    .with_offset(vec3(0.0, 0.27, -0.63)),
            ),
        ),
        (
            "al_h",
            symmetric(held_weapon_right.clone().with_offset(vec3(0.0, 0.09, 0.0))),
        ),
        (
            "viro_h",
            symmetric(
                held_weapon_right
                    .clone()
                    .with_offset(vec3(0.0, 0.22, -0.90)),
            ),
        ),
        (
            "amp_h",
            symmetric(held_weapon_right.clone().with_offset(vec3(0.0, 0.0, 0.40))),
        ),
        (
            "lasehand",
            symmetric(held_weapon_right.clone().with_offset(vec3(0.0, 0.12, 0.27)))
                .with_projectile_rotation(Quaternion::from_angle_y(Deg(12.))),
        ),
        // Melee first-person models (_h): LGMM skinned meshes posed to the
        // player-melee idle (see ChangeModel in mission_core). Unlike the guns
        // these entries deliberately DO NOT move the model to seat its baked
        // fist in the palm: the held entity's body *is* the melee contact
        // collider (#942/#978), so a grip offset here carries the damage
        // volume off the weapon and silently disarms it. Each melee `_h`
        // therefore keeps its world model's physical grip, and the posed arm
        // is seated by a render-only model-space correction instead
        // (`melee_wield_pose_correction`).
        ("wrench_h", symmetric(wrench_right.clone())),
        ("rapier_h", melee_world_grip.clone()),
        ("shard_h", melee_world_grip.clone()),
        ("psword_h", melee_world_grip.clone()),
        // Weapons - world models, kept when held in VR (#352): the _h meshes
        // have faces stripped for the fixed flat camera. sg_w/empgun predate
        // this and show the world models grip fine with the same offsets.
        (
            "atek_w",
            symmetric(
                held_weapon_right
                    .clone()
                    .with_offset(vec3(0.02, 0.04, -0.045)),
            ),
        ),
        (
            "ar15_w",
            symmetric(
                held_weapon_right
                    .clone()
                    .with_offset(vec3(-0.19, 0.0, -0.045)),
            ),
        ),
        (
            "sg_w",
            symmetric(
                held_weapon_right
                    .clone()
                    .with_offset(vec3(-0.21, 0.0, -0.045)),
            ),
        ),
        (
            "laser",
            held_weapon
                .clone()
                .with_projectile_rotation(Quaternion::from_angle_y(Deg(12.))),
        ),
        ("empgun", held_weapon.clone()),
        ("gren_w", held_weapon.clone()),
        ("fsn_w", held_weapon.clone()),
        ("sfg_w", held_weapon.clone()),
        ("amp_w", held_weapon.clone()),
        ("viro_w", held_weapon.clone()),
        ("al_w", held_weapon.clone()),
        // The wrench's handle runs along the weapon axis, so it takes the
        // held-weapon rotation (handle through the fist, head forward)
        ("wrench_w", symmetric(wrench_right.clone())),
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

/// First-person models the flat wield swap may apply. Frozen to the set that
/// was allowed before the VR `_h` route grew the grip table, so flat behavior
/// (which models donate vhots via the swap) is unchanged by VR tuning entries.
const FLAT_WIELD_SWAP_MODELS: &[&str] = &["atek_h", "amp_h", "lasehand", "wrench_h"];

pub fn is_allowed_hand_model(model_name: &str) -> bool {
    let name = model_name.to_ascii_lowercase();
    FLAT_WIELD_SWAP_MODELS.contains(&name.as_str())
}

/// The 25th Anniversary Edition's remastered first-person gun models
/// (`obj/*_h.bin`, LGMD), shipped in `mods/sshock2ee.kpf` which outranks every
/// classic archive - so on a 25AE install these names always resolve to the
/// remastered meshes. mesh_audit measures them 0-10.5% open edges versus
/// 19-38% for the classic `_h` set, and they are authored barrel-along -X
/// with real muzzle vhots, so VR can wield them from any viewpoint.
///
/// Melee `_h` models (wrench/rapier/shard/psword) are LGMM skinned meshes
/// (classic geometry + remastered `PMNM` chunk); VR wields them posed to the
/// player-melee idle clip (see `Effect::ChangeModel` in mission_core). The
/// 25AE-only `pipewrench_h` has no gamesys template (it ships as a KEX squirrel
/// script we don't run), so it is not listed.
const VR_25AE_VIEW_MODELS: &[&str] = &[
    "atek_h", "ar15_h", "sg_h", "lasehand", "empgun_h", "gren_h", "sfg_h", "fsn_h", "al_h",
    "viro_h", "amp_h", "wrench_h", "rapier_h", "shard_h", "psword_h",
];

/// Whether `model_name` is a first-person view model VR should wield in place
/// of the world model (only meaningful on a 25AE install, where the remastered
/// copy is what resolves).
pub fn is_vr_view_model(model_name: &str) -> bool {
    let name = model_name.to_ascii_lowercase();
    VR_25AE_VIEW_MODELS.contains(&name.as_str())
}

/// Skeleton joint the melee `_h` rigs pose as the weapon-holding fist (root,
/// shoulder, elbow, **fist**, weapon tip - see
/// `cargo run -p shock2vr --example melee_grip`).
pub const MELEE_GRIP_JOINT: usize = 3;

/// Yaw that points the authored ready-stance weapon up and forward out of the
/// fist once the arm is anchored on the tracked hand.
const MELEE_GRIP_YAW: Deg<f32> = Deg(120.0);

/// Model-space transform that seats a posed melee `_h` arm's baked fist
/// (`posed_fist`, from [`MELEE_GRIP_JOINT`]) on the tracked hand.
///
/// This is deliberately a *render* correction rather than a grip offset: the
/// held entity's rigid body is the melee contact collider, so moving the
/// entity to align the arm would carry the damage volume ~0.7 units off the
/// weapon and the wield would look right while doing nothing (#942/#978). The
/// entity keeps its world model's grip, and this cancels that grip before
/// applying the arm alignment, so the rendered result is grip-independent.
pub fn melee_wield_pose_correction(
    model_name: &str,
    posed_fist: Vector3<f32>,
) -> cgmath::Matrix4<f32> {
    use cgmath::{Matrix4, SquareMatrix};

    let grip = get_vr_hand_model_adjustments_from_model(model_name, Handedness::Right);
    let grip_matrix = Matrix4::from_translation(grip.offset) * Matrix4::from(grip.rotation);
    grip_matrix.invert().unwrap_or_else(Matrix4::identity)
        * Matrix4::from_angle_y(MELEE_GRIP_YAW)
        * Matrix4::from_translation(-posed_fist)
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Every VR-wieldable 25AE view model must have a grip entry, or it would
    /// anchor at the model origin with no rotation.
    #[test]
    fn every_vr_view_model_has_a_grip_entry() {
        for name in VR_25AE_VIEW_MODELS {
            assert!(
                HAND_MODEL_POSITIONING.contains_key(name),
                "missing HAND_MODEL_POSITIONING entry for {name}"
            );
        }
    }

    /// Flat's wield swap is frozen: growing the VR grip table must not change
    /// which models flat swaps to (and thereby its vhot donors).
    #[test]
    fn flat_wield_swap_set_is_frozen() {
        for name in ["atek_h", "amp_h", "lasehand", "wrench_h"] {
            assert!(is_allowed_hand_model(name));
        }
        for name in ["sg_h", "ar15_h", "empgun_h", "atek_w", "battery"] {
            assert!(!is_allowed_hand_model(name), "{name} must not swap in flat");
        }
    }

    #[test]
    fn vr_view_model_lookup_is_case_insensitive() {
        assert!(is_vr_view_model("ATEK_H"));
        assert!(is_vr_view_model("lasehand"));
        assert!(is_vr_view_model("WRENCH_H"));
        assert!(is_vr_view_model("psword_h"));
        assert!(!is_vr_view_model("pipewrench_h"));
        assert!(!is_vr_view_model("atek_w"));
    }
}
