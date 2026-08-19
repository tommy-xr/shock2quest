use std::collections::HashMap;

use cgmath::{Deg, Quaternion, Rotation3, Vector3, vec3};
use dark::properties::PropModelName;

use crate::runtime_props::RuntimePropVrGripOffset;
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
        // Melee first-person models (_h) deliberately have NO entry: their
        // grip is not a constant. The held body is the melee contact collider
        // (#942/#978) and has to sit on the *rendered* weapon head, which is
        // only known once the arm is posed - so the wield computes it
        // (`melee_contact_offset`) and stores it per entity as
        // `RuntimePropVrGripOffset`, which this table defers to. Leaving them
        // unmapped also keeps the default 180-degree projectile rotation they
        // had before VR wielded them.
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

/// Skeleton joints of the melee `_h` rigs: root, shoulder, **elbow**, **fist**,
/// **weapon** (see `cargo run -p shock2vr --example melee_grip`).
const MELEE_ARM_JOINT: usize = 2;
const MELEE_GRIP_JOINT: usize = 3;
const MELEE_WEAPON_JOINT: usize = 4;

/// The posed arm's joints, in model space, that the wield is built from:
/// elbow, fist and the weapon joint at the business end of the blade/head.
#[derive(Clone, Copy, Debug)]
pub struct MeleePosedArm {
    pub elbow: Vector3<f32>,
    pub fist: Vector3<f32>,
    pub weapon: Vector3<f32>,
}

impl MeleePosedArm {
    /// Read the three joints out of a posed melee `_h` skeleton
    /// (`AnimationPlayer::get_transforms`). `None` for a rig that does not
    /// have them - a patched or modded mesh must leave the wield unseated,
    /// not panic the frame.
    pub fn from_joints(joints: &[cgmath::Matrix4<f32>]) -> Option<MeleePosedArm> {
        Some(MeleePosedArm {
            elbow: joints.get(MELEE_ARM_JOINT)?.w.truncate(),
            fist: joints.get(MELEE_GRIP_JOINT)?.w.truncate(),
            weapon: joints.get(MELEE_WEAPON_JOINT)?.w.truncate(),
        })
    }
}

/// Rotation that takes the posed arm out of model space and into the hand's
/// own frame: the forearm runs back along the hand frame's **+Z** - the axis
/// [`crate::hand_forearm`] hangs the empty hand's sleeve along, and the
/// opposite of the -Z the fingers/aim point down - and the weapon is rolled
/// into the hand's vertical plane so it stands up out of the fist.
///
/// Derived from the rig rather than hand-tuned: a single authored yaw can only
/// satisfy one of the two, and the shipped 120 degrees satisfied the weapon,
/// leaving the forearm 71-80 degrees off the hand's own arm axis (it stuck out
/// sideways past the wrist instead of running back toward the elbow).
fn melee_wield_alignment(arm: MeleePosedArm) -> Quaternion<f32> {
    use cgmath::{Rad, Rotation};

    let forearm = arm.elbow - arm.fist;
    let weapon = arm.weapon - arm.fist;
    let align = Quaternion::from_arc(forearm, Vector3::unit_z(), None);
    let aligned_weapon = align.rotate_vector(weapon);
    // Roll about the forearm until the weapon lies in the hand's YZ plane,
    // pointing along +Y (up out of the fist) rather than across the palm.
    let roll = Quaternion::from_angle_z(Rad(aligned_weapon.x.atan2(aligned_weapon.y)));
    roll * align
}

/// Where the weapon joint ends up, in hand-local space, once the arm is seated
/// by [`melee_wield_alignment`] - i.e. where the *rendered* head of the weapon
/// is, and therefore where the held entity's body (the melee contact collider)
/// has to sit for the drawn weapon and the damage volume to be the same place.
/// The wield stores it as `RuntimePropVrGripOffset`;
/// `cargo run -p shock2vr --example melee_grip` prints it per model.
///
/// Known limitation: the authored contact volume is a ~5 cm sphere, so this
/// arms the weapon's *head* and nothing else along its length. Covering the
/// whole blade needs a collider shaped like the weapon, which is a bigger
/// change than putting the existing one in the right place.
pub fn melee_contact_offset(arm: MeleePosedArm) -> Vector3<f32> {
    use cgmath::Rotation;

    melee_wield_alignment(arm).rotate_vector(arm.weapon - arm.fist)
}

/// Model-space transform that seats a posed melee `_h` arm's baked fist on the
/// tracked hand, with the forearm along the hand's arm axis.
///
/// This is a *render* correction rather than a grip offset because the held
/// entity's rigid body is the melee contact collider, and that has its own job:
/// sitting on the rendered weapon head (#942/#978). The two are two halves of
/// one placement and are derived from the same posed arm, so they cannot drift:
/// the entity is at `hand + contact`, this cancels that same `contact`, and the
/// weapon joint therefore renders exactly on the collider for any rig.
pub fn melee_wield_pose_correction(arm: MeleePosedArm) -> cgmath::Matrix4<f32> {
    use cgmath::Matrix4;

    Matrix4::from_translation(-melee_contact_offset(arm))
        * Matrix4::from(melee_wield_alignment(arm))
        * Matrix4::from_translation(-arm.fist)
}

pub fn get_vr_hand_model_adjustments_from_entity(
    entity_id: EntityId,
    world: &World,
    handedness: Handedness,
) -> VRHandModelPerHandAdjustments {
    // A wield that had to compute its own grip (melee `_h`: the collider goes
    // on the rendered weapon head, which is only known once the arm is posed)
    // stored it on the entity. It is hand-agnostic by construction.
    if let Some(grip) = world
        .borrow::<View<RuntimePropVrGripOffset>>()
        .ok()
        .and_then(|view| view.get(entity_id).ok().map(|grip| grip.0))
    {
        return VRHandModelPerHandAdjustments::new().with_offset(grip);
    }

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

    /// The melee subset of [`VR_25AE_VIEW_MODELS`]: skinned arm rigs, seated
    /// from the posed skeleton rather than from a static grip entry.
    const MELEE_VIEW_MODELS: &[&str] = &["wrench_h", "rapier_h", "shard_h", "psword_h"];

    /// Every VR-wieldable 25AE gun must have a grip entry, or it would anchor
    /// at the model origin with no rotation. The melee `_h` are the deliberate
    /// exception - see `melee_view_models_have_no_static_grip`.
    #[test]
    fn every_vr_view_model_has_a_grip_entry() {
        for name in VR_25AE_VIEW_MODELS {
            if MELEE_VIEW_MODELS.contains(name) {
                continue;
            }
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

    /// A melee `_h` must NOT have a static grip entry: its grip is computed
    /// per wield (`RuntimePropVrGripOffset`) because the collider has to land
    /// on the rendered weapon head. A table entry here would take precedence
    /// for anything that looks the model up by name and silently reintroduce a
    /// fixed offset the render correction is not cancelling.
    #[test]
    fn melee_view_models_have_no_static_grip() {
        for name in MELEE_VIEW_MODELS {
            assert!(
                !HAND_MODEL_POSITIONING.contains_key(name),
                "{name} must take its grip from the posed rig, not this table"
            );
            // ... and stay on the unmapped default projectile rotation they
            // had before VR wielded them.
            assert_eq!(
                get_vr_projectile_rotation_from_model(name),
                Quaternion::from_angle_y(Deg(180.0))
            );
        }
    }

    /// The four shipped melee `_h` rigs, posed to the player-melee idle's
    /// final frame, as `cargo run -p shock2vr --example melee_grip` prints
    /// them. Test data so the seating maths can be checked without assets.
    fn posed_melee_arms() -> [(&'static str, MeleePosedArm); 4] {
        [
            (
                "wrench_h",
                MeleePosedArm {
                    elbow: vec3(-0.4087, 0.2532, 0.1699),
                    fist: vec3(-0.4767, 0.1516, 0.5323),
                    weapon: vec3(-0.0019, 0.7427, 0.8214),
                },
            ),
            (
                "rapier_h",
                MeleePosedArm {
                    elbow: vec3(-0.3454, 0.2872, 0.2055),
                    fist: vec3(-0.4700, 0.2143, 0.5563),
                    weapon: vec3(-0.0936, 1.3100, 0.7363),
                },
            ),
            (
                "shard_h",
                MeleePosedArm {
                    elbow: vec3(-0.3454, 0.2872, 0.2055),
                    fist: vec3(-0.4700, 0.2143, 0.5563),
                    weapon: vec3(-0.0999, 1.3987, 0.5924),
                },
            ),
            (
                "psword_h",
                MeleePosedArm {
                    elbow: vec3(-0.3454, 0.2872, 0.2055),
                    fist: vec3(-0.4642, 0.2175, 0.5483),
                    weapon: vec3(-0.1333, 1.3595, 0.5553),
                },
            ),
        ]
    }

    /// The seated arm must run back along the hand frame's +Z - the axis the
    /// empty hand's sleeve (`hand_forearm`) uses - with the weapon standing up
    /// out of the fist in the hand's vertical plane. The shipped single yaw
    /// left the forearm 71-80 degrees off this, poking out sideways past the
    /// wrist.
    #[test]
    fn melee_wield_seats_the_forearm_on_the_hands_arm_axis() {
        use cgmath::{InnerSpace, Rotation};

        for (name, arm) in posed_melee_arms() {
            let alignment = melee_wield_alignment(arm);
            let forearm = alignment.rotate_vector(arm.elbow - arm.fist);
            let angle = forearm
                .normalize()
                .dot(Vector3::unit_z())
                .acos()
                .to_degrees();
            assert!(angle < 0.5, "{name} forearm {angle} deg off the hand's +Z");

            let weapon = alignment.rotate_vector(arm.weapon - arm.fist);
            assert!(weapon.x.abs() < 1e-3, "{name} weapon out of the YZ plane");
            assert!(weapon.y > 0.0, "{name} weapon points down out of the fist");
        }
    }

    /// The whole point of the arrangement: the entity sits at
    /// `hand + melee_contact_offset` (its body is the contact collider) and the
    /// render correction runs inside that transform, so the model's *weapon
    /// joint* must land exactly on the collider and its *fist* exactly on the
    /// hand. Checked as composed matrices, for every shipped rig - this is what
    /// makes "what you see is what you hit" a property rather than a
    /// measurement someone has to keep up to date.
    #[test]
    fn the_rendered_weapon_head_lands_on_the_contact_collider() {
        use cgmath::{EuclideanSpace, InnerSpace, Matrix4, Point3, Transform};

        for (name, arm) in posed_melee_arms() {
            // What `VirtualHand::SetPositionRotation` composes for a hand at
            // the origin with no rotation: entity = T(grip.offset).
            let entity = Matrix4::from_translation(melee_contact_offset(arm));
            let rendered = entity * melee_wield_pose_correction(arm);

            let head = rendered.transform_point(Point3::from_vec(arm.weapon));
            let collider = Point3::from_vec(melee_contact_offset(arm));
            assert!(
                (head - collider).magnitude() < 1e-4,
                "{name}: rendered weapon head {head:?} is off the collider {collider:?}"
            );

            let fist = rendered.transform_point(Point3::from_vec(arm.fist));
            assert!(
                fist.to_vec().magnitude() < 1e-4,
                "{name}: rendered fist {fist:?} is off the tracked hand"
            );
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
