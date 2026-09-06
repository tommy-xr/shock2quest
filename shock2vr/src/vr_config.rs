use std::collections::HashMap;

use cgmath::{Deg, Quaternion, Rotation3, Vector3, vec3};
use dark::properties::PropModelName;

use crate::runtime_props::{RuntimePropVrGripOffset, RuntimePropVrGunWield};
use once_cell::sync::Lazy;
use shipyard::{EntityId, Get, View, World};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Handedness {
    Left,
    Right,
}

impl Handedness {
    /// The reflection that turns right-handed geometry into this hand's.
    ///
    /// Every first-person asset the player wears or wields is authored for the
    /// **right** hand - the glove model, the melee `_h` arm rigs - so the left
    /// hand draws the mirror image, reflected across the hand frame's own X.
    /// This is the single definition of what "the other hand" means, shared by
    /// [`crate::hand_glove`] and the melee wield, so the arm and the glove
    /// cannot disagree about which way round the left hand is.
    ///
    /// Held guns reflect across a different plane of their own frame - see
    /// [`Self::gun_mirror`] - because their authored axes differ from the arm
    /// rigs'.
    ///
    /// A reflection has a negative determinant, which reverses the triangle
    /// winding the GPU sees. Geometry that culls backfaces must flip its
    /// front-face winding to match - [`dark::model::Model::apply_local_transform`]
    /// does that for meshes seated by a local transform. The glove needs
    /// nothing because GLB meshes are drawn double-sided.
    pub fn mirror(self) -> cgmath::Matrix4<f32> {
        use cgmath::{Matrix4, SquareMatrix};

        match self {
            Handedness::Right => Matrix4::identity(),
            Handedness::Left => Matrix4::from_nonuniform_scale(-1.0, 1.0, 1.0),
        }
    }

    /// [`Self::mirror`] applied to a hand-local point. Derived from the matrix
    /// rather than spelled out again, so the two cannot drift apart.
    pub fn mirror_point(self, point: Vector3<f32>) -> Vector3<f32> {
        use cgmath::Transform;

        self.mirror().transform_vector(point)
    }

    /// The reflection that turns a right-handed 25AE gun view model into this
    /// hand's, in the model's own frame.
    ///
    /// The `_h` guns bake a right hand onto the grip, so a left-hand wield
    /// draws the mirror image. Their authored frame runs the barrel along -X
    /// with +Y up, so the mirror plane is the one holding the barrel and the
    /// sights - it negates Z, "across the gun" - and the barrel keeps pointing
    /// the same way. The grip's thumb-side offset
    /// ([`VRHandModelPerHandAdjustments::flip_x`]) and the muzzle vhots
    /// reflect with it; the negative determinant is what
    /// [`dark::model::Model::apply_local_transform`] flips the winding for.
    pub fn gun_mirror(self) -> cgmath::Matrix4<f32> {
        use cgmath::{Matrix4, SquareMatrix};

        match self {
            Handedness::Right => Matrix4::identity(),
            Handedness::Left => Matrix4::from_nonuniform_scale(1.0, 1.0, -1.0),
        }
    }
}

/// The index a hand occupies in a `[_; 2]` of per-hand state.
///
/// One conversion, crate-wide, so per-hand arrays built in one module can be
/// read in another. Anything that pairs the hands in an array (latches,
/// per-hand gestures) uses THIS ordering; a local array in some other order
/// must not be indexed with it.
pub fn hand_slot(hand: Handedness) -> usize {
    match hand {
        Handedness::Left => 0,
        Handedness::Right => 1,
    }
}

#[derive(Clone, Debug)]
pub struct VRHandModelPerHandAdjustments {
    pub offset: Vector3<f32>,
    pub rotation: Quaternion<f32>,
}

impl VRHandModelPerHandAdjustments {
    pub fn new() -> VRHandModelPerHandAdjustments {
        VRHandModelPerHandAdjustments {
            offset: Vector3::new(0.0, 0.0, 0.0),
            rotation: Quaternion::new(1.0, 0.0, 0.0, 0.0),
        }
    }

    pub fn rotate_y(self, angle: Deg<f32>) -> VRHandModelPerHandAdjustments {
        VRHandModelPerHandAdjustments {
            rotation: self.rotation * Quaternion::from_angle_y(angle),
            ..self
        }
    }

    /// The left hand's grip for a gun whose model is drawn reflected by
    /// [`Handedness::gun_mirror`]: the thumb-side component of the hand-local
    /// offset mirrors with the geometry, so the reflected gun seats on the
    /// left palm exactly where the authored one seats on the right. Only for
    /// models that ARE reflected (the `_h` set) - an unmirrored world model
    /// keeps the same grip in both hands.
    pub fn flip_x(self) -> VRHandModelPerHandAdjustments {
        VRHandModelPerHandAdjustments {
            offset: vec3(-self.offset.x, self.offset.y, self.offset.z),
            ..self
        }
    }

    /// Hand-local translation (world units, 1 unit ~ 0.762 m - the space
    /// `hand_position` itself is in): +X toward the thumb side of the right
    /// hand, +Y up out of the back of the hand, -Z along the fingers.
    pub fn with_offset(self, offset: Vector3<f32>) -> VRHandModelPerHandAdjustments {
        VRHandModelPerHandAdjustments { offset, ..self }
    }
}

#[derive(Clone)]
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

    let held_weapon_right = VRHandModelPerHandAdjustments::new().rotate_y(Deg(-90.0));
    // World models are drawn unmirrored in either hand, so the left grip is
    // the right grip as-is.
    let held_weapon_left = held_weapon_right.clone();
    let held_weapon = VRHandModelAdjustments::new(held_weapon_left, held_weapon_right.clone());

    // The wrench's long axis runs opposite the guns' after the -90 yaw (its
    // model is authored along X where guns are along Y), so it takes +90 and
    // slides toward its handle end
    let wrench_right = VRHandModelPerHandAdjustments::new()
        .rotate_y(Deg(90.0))
        .with_offset(vec3(0.0, 0.0, -0.4));

    let held_item_hand = VRHandModelPerHandAdjustments::new().rotate_y(Deg(180.0));
    let held_item = VRHandModelAdjustments::new(held_item_hand.clone(), held_item_hand);

    // Hand model adjustments for VR
    // Specify overrides for particular models with how they should be oriented
    // relative ot the virtual hand
    // A 25AE `_h` gun: the left hand draws the model reflected
    // (`Handedness::gun_mirror`), so its grip is the right grip reflected too
    // (`flip_x`).
    fn symmetric(right: VRHandModelPerHandAdjustments) -> VRHandModelAdjustments {
        VRHandModelAdjustments::new(right.clone().flip_x(), right)
    }
    // A world model: drawn unmirrored in either hand, so both grips are the
    // right-hand one.
    fn same_grip(right: VRHandModelPerHandAdjustments) -> VRHandModelAdjustments {
        VRHandModelAdjustments::new(right.clone(), right)
    }

    let items = vec![
        // Weapons - first-person hand models (_h). Used by flat's wield swap
        // (FLAT_WIELD_SWAP_MODELS) and, on a 25AE install, wielded directly in
        // VR (VR_25AE_VIEW_MODELS). The whole 25AE set is authored barrel
        // along -X, so one -90 yaw seats every gun.
        //
        // The offsets put the model's own PISTOL GRIP in the glove's fist,
        // read off the side-on silhouette `cargo run -p shock2vr --example
        // gun_hand_islands` prints and then corrected against `debug_weapons`
        // captures. They were previously fitted to where each model's *baked*
        // hand sat, which for `ar15_h` is a rest hand on the receiver and for
        // `sg_h` one on the pump - invisible while that hand was the one
        // drawn, wrong the moment the player's own glove replaced it.
        //
        // Hand-local world units at the wield's own scale: multiplied by
        // `gun_wield_scale` alongside the geometry (see
        // `gun_wield_adjustments`), so the pair is one uniform scale about the
        // grip. The oversized weapons (`sfg_h`, `fsn_h`, `gren_h`, `al_h`,
        // `viro_h`) have no authored grip to find; their offsets put the fist
        // on the nearest thing a hand could hold and leave the far end alone.
        (
            "atek_h",
            symmetric(
                held_weapon_right
                    .clone()
                    .with_offset(vec3(0.0, 0.073, 0.020)),
            ),
        ),
        (
            "ar15_h",
            symmetric(
                held_weapon_right
                    .clone()
                    .with_offset(vec3(0.0, 0.123, -0.469)),
            ),
        ),
        (
            "sg_h",
            symmetric(
                held_weapon_right
                    .clone()
                    .with_offset(vec3(0.0, -0.025, -0.693)),
            ),
        ),
        (
            "empgun_h",
            symmetric(
                held_weapon_right
                    .clone()
                    .with_offset(vec3(0.0, 0.083, -0.473)),
            ),
        ),
        (
            "gren_h",
            symmetric(
                held_weapon_right
                    .clone()
                    .with_offset(vec3(0.0, 0.111, -0.960)),
            ),
        ),
        (
            "sfg_h",
            symmetric(
                held_weapon_right
                    .clone()
                    .with_offset(vec3(0.0, 0.466, -0.553)),
            ),
        ),
        (
            "fsn_h",
            symmetric(
                held_weapon_right
                    .clone()
                    .with_offset(vec3(0.0, 0.310, -0.516)),
            ),
        ),
        (
            "al_h",
            symmetric(
                held_weapon_right
                    .clone()
                    .with_offset(vec3(0.0, 0.206, -0.442)),
            ),
        ),
        (
            "viro_h",
            symmetric(
                held_weapon_right
                    .clone()
                    .with_offset(vec3(0.0, 0.111, -0.710)),
            ),
        ),
        (
            "amp_h",
            symmetric(held_weapon_right.clone().with_offset(vec3(0.0, 0.0, 0.40))),
        ),
        (
            "lasehand",
            symmetric(
                held_weapon_right
                    .clone()
                    .with_offset(vec3(0.0, 0.096, 0.190)),
            ),
        ),
        // Melee first-person models (_h) deliberately have NO entry: their
        // grip is not a constant. The held body is the melee contact collider
        // (#942/#978) and has to sit on the *rendered* weapon head, which is
        // only known once the arm is posed - so the wield computes it
        // (`melee_contact_offset`) and stores it per entity as
        // `RuntimePropVrGripOffset`, which this table defers to.
        // Weapons - world models, kept when held in VR (#352): the _h meshes
        // have faces stripped for the fixed flat camera. sg_w/empgun predate
        // this and show the world models grip fine with the same offsets.
        (
            "atek_w",
            same_grip(
                held_weapon_right
                    .clone()
                    .with_offset(vec3(0.02, 0.04, -0.045)),
            ),
        ),
        (
            "ar15_w",
            same_grip(
                held_weapon_right
                    .clone()
                    .with_offset(vec3(-0.19, 0.0, -0.045)),
            ),
        ),
        (
            "sg_w",
            same_grip(
                held_weapon_right
                    .clone()
                    .with_offset(vec3(-0.21, 0.0, -0.045)),
            ),
        ),
        ("laser", held_weapon.clone()),
        ("empgun", held_weapon.clone()),
        ("gren_w", held_weapon.clone()),
        ("fsn_w", held_weapon.clone()),
        ("sfg_w", held_weapon.clone()),
        ("amp_w", held_weapon.clone()),
        ("viro_w", held_weapon.clone()),
        ("al_w", held_weapon.clone()),
        // The wrench's handle runs along the weapon axis, so it takes the
        // held-weapon rotation (handle through the fist, head forward)
        ("wrench_w", same_grip(wrench_right.clone())),
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

/// The subset of [`VR_25AE_VIEW_MODELS`] VR wields as a rigid gun: the static
/// LGMD meshes whose baked hand is stripped so the player's own glove can hold
/// them ([`dark::importers::VrHeldGunModel`], [`gun_wield_scale`]).
///
/// Keyed on the model name, like every other per-model table here, rather than
/// on gamesys metadata: the strip and the glove have to agree about which
/// models they apply to, and a name cannot disagree with itself. It excludes
/// the melee rigs (skinned, seated from their own posed skeleton) and the psi
/// amp, whose forearm is modelled into the amp's own `ND-amp_h.psd` - nothing
/// isolates it, so there is nothing to strip and nothing for a glove to
/// replace. `the_view_models_split_into_guns_and_melee_and_the_amp` pins the
/// partition.
const VR_25AE_GUN_MODELS: &[&str] = &[
    "atek_h", "ar15_h", "sg_h", "lasehand", "empgun_h", "gren_h", "sfg_h", "fsn_h", "al_h",
    "viro_h",
];

// --- Magazine anchors ---------------------------------------------------------
//
// Where a held clip has to reach to load a wielded gun, in the *weapon's own
// authored model frame* - NOT the hand-local space the grip offsets above use.
// The 25AE guns are authored barrel along -X, so +X is back toward the
// shooter, +Y up out of the gun body, +Z across it. World units (1 unit =
// 0.762 m), the space `RuntimePropTransform` lives in.

/// Per-model magazine anchors, keyed like [`HAND_MODEL_POSITIONING`]
/// (lowercased `PropModelName`). Where the art has a clip part (`ar15_h`'s
/// `@s02_cli`, `fsn_h`'s `@01_core`, found with `cargo dv <model>.bin
/// --debug-subobjects`) the anchor is its centre; the rest sit on the grip or
/// loading port. Every value is placed against the rendered wield with the
/// `clip_zone` dev param (which also marks the model origin), not from the
/// dump alone - the pistol's origin, for one, renders at the wrist, well
/// behind where its sub-object bounds suggest.
///
/// A model with no entry keeps the zone on its own origin: a classic install's
/// world models; the energy weapons (`lasehand`, `empgun_h`), which recharge
/// rather than take a clip; `sfg_h`, whose dump shows a stray part 5 m off the
/// model and is not trusted yet; and `al_h` / `viro_h`, not measured yet.
static MAGAZINE_ANCHORS: Lazy<HashMap<&str, Vector3<f32>>> = Lazy::new(|| {
    HashMap::from([
        ("atek_h", vec3(-0.20, -0.15, 0.0)),
        ("ar15_h", vec3(-0.04, -0.20, 0.0)),
        ("sg_h", vec3(0.08, -0.10, 0.0)),
        ("gren_h", vec3(0.10, -0.05, 0.0)),
        ("fsn_h", vec3(-0.65, 0.0, 0.0)),
    ])
});

/// Where `entity_id`'s magazine is, in its model's local frame - the origin
/// for a model with no entry.
pub fn magazine_anchor_from_entity(world: &World, entity_id: EntityId) -> Vector3<f32> {
    // The anchor is authored against the model as shipped, and a VR wield draws
    // the gun shrunk to life size - so the anchor rides the same scale, or the
    // clip zone would sit off in space beside a 40%-size gun.
    let scale = gun_wield_scale_of_entity(world, entity_id);
    model_name_lower(world, entity_id)
        .map(|name| scale * magazine_anchor_from_model(&name))
        .unwrap_or(vec3(0.0, 0.0, 0.0))
}

/// [`magazine_anchor_from_entity`] by model name (any case).
fn magazine_anchor_from_model(model_name: &str) -> Vector3<f32> {
    MAGAZINE_ANCHORS
        .get(model_name.to_ascii_lowercase().as_str())
        .copied()
        .unwrap_or(vec3(0.0, 0.0, 0.0))
}

/// `entity_id`'s `PropModelName`, lowercased - the key every per-model table
/// in this file is looked up by.
fn model_name_lower(world: &World, entity_id: EntityId) -> Option<String> {
    world
        .borrow::<View<PropModelName>>()
        .ok()
        .and_then(|names| {
            names
                .get(entity_id)
                .ok()
                .map(|name| name.0.to_ascii_lowercase())
        })
}

/// Whether `model_name` is a first-person view model VR should wield in place
/// of the world model (only meaningful on a 25AE install, where the remastered
/// copy is what resolves).
pub fn is_vr_view_model(model_name: &str) -> bool {
    let name = model_name.to_ascii_lowercase();
    VR_25AE_VIEW_MODELS.contains(&name.as_str())
}

/// Whether `model_name` is one of the rigid gun view models VR draws without
/// its baked hand, with the player's glove on it instead
/// ([`VR_25AE_GUN_MODELS`]).
pub fn is_vr_gun_view_model(model_name: &str) -> bool {
    let name = model_name.to_ascii_lowercase();
    VR_25AE_GUN_MODELS.contains(&name.as_str())
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
/// own frame: the forearm runs back along the hand frame's **+Z** - the
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
/// The rigid-body origin remains on this weapon joint. `Effect::ChangeModel`
/// separately fits the body's collider around the rendered weapon vertices,
/// including the handle/blade extending back from the joint.
/// Hand-agnostic: [`melee_wield_alignment`] rolls the weapon into the hand's
/// YZ plane, so this lands on the hand's own centreline (x = 0) and the
/// left-hand mirror - a reflection across exactly that axis - leaves it where
/// it is. The damage volume is in the same place for either hand; only the
/// geometry around it swaps sides. Pinned by
/// `the_contact_point_is_on_the_hands_centreline`, because the left-hand
/// wield's "baked fist lands on the tracked hand" depends on it.
pub fn melee_contact_offset(arm: MeleePosedArm) -> Vector3<f32> {
    melee_contact_offset_scaled(arm, melee_wield_scale())
}

fn melee_contact_offset_scaled(arm: MeleePosedArm, scale: f32) -> Vector3<f32> {
    use cgmath::Rotation;

    scale * melee_wield_alignment(arm).rotate_vector(arm.weapon - arm.fist)
}

/// Live tuning for how large a wielded melee view model is drawn - see
/// [`crate::dev_params::MELEE_WIELD_SCALE`]. Both halves of the placement read
/// it, so the grip and the contact collider scale together and the
/// "rendered weapon head lands on the collider" invariant is unaffected.
fn melee_wield_scale() -> f32 {
    crate::dev_params::get(crate::dev_params::MELEE_WIELD_SCALE)
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
///
/// `handedness` picks which arm is drawn: the rigs are authored right-handed,
/// so the left hand gets the whole seated arm reflected by
/// [`Handedness::mirror`], as the outermost factor. The contact offset needs no
/// mirror of its own - it already sits on the axis being reflected - so the
/// invariant above holds for both hands. The negative determinant the
/// reflection introduces is what
/// [`dark::model::Model::apply_local_transform`] flips the front-face winding
/// for, so the mirrored arm is not drawn inside-out.
pub fn melee_wield_pose_correction(
    arm: MeleePosedArm,
    handedness: Handedness,
) -> cgmath::Matrix4<f32> {
    melee_wield_pose_correction_scaled(arm, melee_wield_scale(), handedness)
}

fn melee_wield_pose_correction_scaled(
    arm: MeleePosedArm,
    scale: f32,
    handedness: Handedness,
) -> cgmath::Matrix4<f32> {
    use cgmath::Matrix4;

    handedness.mirror()
        * Matrix4::from_translation(-melee_contact_offset_scaled(arm, scale))
        * Matrix4::from_scale(scale)
        * Matrix4::from(melee_wield_alignment(arm))
        * Matrix4::from_translation(-arm.fist)
}

/// Live tuning for how large a wielded rigid gun `_h` view model is drawn -
/// see [`crate::dev_params::GUN_WIELD_SCALE`]. Every half of the placement is
/// scaled by the value the wield recorded on
/// [`crate::runtime_props::RuntimePropVrGunWield`] - the geometry and its
/// muzzle vhots when the model is applied, the hand-local grip offset and the
/// magazine anchor when they are read - so the gun shrinks about the grip
/// point and stays seated, aimed and reloadable at any scale.
///
/// The player's glove is drawn on it at life size, unscaled: that is the whole
/// point of shrinking the gun.
pub fn gun_wield_scale() -> f32 {
    crate::dev_params::get(crate::dev_params::GUN_WIELD_SCALE)
}

/// The local transform a VR wield bakes into a first-person gun model: the
/// left hand's reflection, and the life-size shrink. One of the two halves of
/// a uniform scale about the grip point; [`gun_wield_adjustments`] is the
/// other. Applied to the geometry AND its muzzle vhots, so the shot still
/// leaves the drawn barrel.
pub fn gun_wield_model_transform(handedness: Handedness, scale: f32) -> cgmath::Matrix4<f32> {
    cgmath::Matrix4::from_scale(scale) * handedness.gun_mirror()
}

/// The hand-local seat of a gun wielded at `scale`: the model's authored grip
/// entry, its offset scaled with the geometry so the grip point itself does
/// not move off the hand.
fn gun_wield_adjustments(
    model_name: &str,
    handedness: Handedness,
    scale: f32,
) -> VRHandModelPerHandAdjustments {
    let adjustments = get_vr_hand_model_adjustments_from_model(model_name, handedness);
    VRHandModelPerHandAdjustments {
        offset: scale * adjustments.offset,
        ..adjustments
    }
}

/// The scale `entity_id`'s wield baked into its gun model, or 1.0 for anything
/// that is not a scaled gun wield (world models, melee rigs, the psi amp).
///
/// Everything measured against the gun's own geometry reads it: the grip offset
/// and magazine anchor below, and the clip-insert zone's radii
/// (`crate::mission::reload::clip_insert_radii`).
pub fn gun_wield_scale_of_entity(world: &World, entity_id: EntityId) -> f32 {
    world
        .borrow::<View<RuntimePropVrGunWield>>()
        .ok()
        .and_then(|wields| wields.get(entity_id).ok().map(|wield| wield.0))
        .unwrap_or(1.0)
}

pub fn get_vr_hand_model_adjustments_from_entity(
    entity_id: EntityId,
    world: &World,
    handedness: Handedness,
) -> VRHandModelPerHandAdjustments {
    // A wield that had to compute its own grip (melee `_h`: the collider goes
    // on the rendered weapon head, which is only known once the arm is posed)
    // stored it on the entity. It is hand-agnostic by construction: it lands on
    // the hand's own centreline, which is the axis a left-hand wield mirrors
    // across (see `melee_contact_offset`). The *mesh* is not hand-agnostic -
    // that mirror is baked into the model by `Effect::ChangeModel`.
    if let Some(grip) = world
        .borrow::<View<RuntimePropVrGripOffset>>()
        .ok()
        .and_then(|view| view.get(entity_id).ok().map(|grip| grip.0))
    {
        return VRHandModelPerHandAdjustments::new().with_offset(grip);
    }

    // A scaled gun wield seats the same authored grip on a shrunk model, so the
    // hand-local offset shrinks with the geometry: together they are one
    // uniform scale about the grip point, which is what keeps the gun in the
    // fist at any [`gun_wield_scale`].
    let scale = gun_wield_scale_of_entity(world, entity_id);
    match model_name_lower(world, entity_id) {
        Some(model_name) => gun_wield_adjustments(&model_name, handedness, scale),
        None => VRHandModelPerHandAdjustments::new(),
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

#[cfg(test)]
mod tests {
    use cgmath::InnerSpace;

    use super::*;

    /// Every wielded view model is either a gun (glove on it, arm stripped), a
    /// melee rig (posed skeleton, arm kept) or the amp (arm kept) - exactly
    /// one of the three. A model that fell out of all three would wield with
    /// neither a glove nor a hand of its own.
    #[test]
    fn the_view_models_split_into_guns_and_melee_and_the_amp() {
        let mut covered = VR_25AE_GUN_MODELS.to_vec();
        covered.extend_from_slice(MELEE_VIEW_MODELS);
        covered.push("amp_h");

        for name in VR_25AE_VIEW_MODELS {
            assert!(
                covered.contains(name),
                "{name} is in none of the three sets"
            );
        }
        for name in &covered {
            assert!(
                VR_25AE_VIEW_MODELS.contains(name),
                "{name} is not a view model"
            );
        }
        assert_eq!(
            covered.len(),
            VR_25AE_VIEW_MODELS.len(),
            "a model is in two sets"
        );
        assert!(!is_vr_gun_view_model("amp_h"));
        assert!(!is_vr_gun_view_model("wrench_h"));
        assert!(is_vr_gun_view_model("ATEK_H"));
    }

    /// The gun wield's two halves are one uniform scale about the grip point:
    /// the model (and its muzzle vhots) shrink about the model origin, and the
    /// hand-local grip offset shrinks with them. So at any scale the authored
    /// grip point stays exactly on the tracked hand, and the muzzle stays down
    /// the barrel it was authored on - the shot still leaves the drawn gun.
    ///
    /// Exercised through the scale-explicit composition rather than the
    /// dev-param registry, so it neither mutates process-global state nor
    /// depends on the default staying 0.4 - the same shape as
    /// `melee_wield_scale_keeps_the_fist_and_the_collider_honest`.
    #[test]
    fn the_gun_wield_scale_keeps_the_grip_in_the_hand_and_the_muzzle_on_the_barrel() {
        use cgmath::{EuclideanSpace, Matrix4, Point3, Rotation, Transform, point3};

        for name in VR_25AE_GUN_MODELS {
            for handedness in [Handedness::Right, Handedness::Left] {
                // Two points in the model's own authored frame - the space
                // vhots live in, and the space the wield transform consumes.
                // The grip: the authored seat says where the model origin goes
                // relative to the hand, so read back through the rotation (and
                // the mirror, which is part of the seat) for the point that
                // lands ON the hand.
                let authored = gun_wield_adjustments(name, handedness, 1.0);
                let grip_model = handedness
                    .gun_mirror()
                    .transform_vector(authored.rotation.invert().rotate_vector(-authored.offset));
                // The muzzle: somewhere down the authored barrel (-X).
                let muzzle_model = point3(-0.6, 0.0, 0.0);
                let unscaled = |scale: f32| {
                    let seat = gun_wield_adjustments(name, handedness, scale);
                    Matrix4::from_translation(seat.offset)
                        * Matrix4::from(seat.rotation)
                        * gun_wield_model_transform(handedness, scale)
                };
                let unscaled_muzzle = unscaled(1.0).transform_point(muzzle_model);

                for scale in [0.4f32, 1.0, 1.25] {
                    // Exactly what the wield composes: the seat this scale
                    // resolves to, carrying the model transform it bakes.
                    let seated = unscaled(scale);

                    let grip = seated.transform_point(Point3::from_vec(grip_model));
                    assert!(
                        grip.to_vec().magnitude() < 1e-4,
                        "{name} ({handedness:?}) at {scale}: grip {grip:?} left the tracked hand"
                    );

                    let muzzle = seated.transform_point(muzzle_model);
                    assert!(
                        (muzzle.to_vec() - scale * unscaled_muzzle.to_vec()).magnitude() < 1e-4,
                        "{name} ({handedness:?}) at {scale}: muzzle {muzzle:?} is not {scale}x the authored {unscaled_muzzle:?}"
                    );
                }
            }
        }
    }

    /// The melee subset of [`VR_25AE_VIEW_MODELS`]: skinned arm rigs, seated
    /// from the posed skeleton rather than from a static grip entry.
    const MELEE_VIEW_MODELS: &[&str] = &["wrench_h", "rapier_h", "shard_h", "psword_h"];

    /// A typo'd anchor key would silently leave that gun's zone on its
    /// origin instead of failing.
    #[test]
    fn every_magazine_anchor_names_a_known_gun_model() {
        for name in MAGAZINE_ANCHORS.keys() {
            assert!(
                VR_25AE_VIEW_MODELS.contains(name) && !MELEE_VIEW_MODELS.contains(name),
                "MAGAZINE_ANCHORS key {name} is not a known gun model"
            );
        }
    }

    /// A model without an entry keeps the v1 behaviour: the zone on its origin.
    #[test]
    fn an_unknown_model_keeps_its_magazine_on_the_origin() {
        assert_eq!(magazine_anchor_from_model("atek"), vec3(0.0, 0.0, 0.0));
        assert_ne!(magazine_anchor_from_model("ar15_h"), vec3(0.0, 0.0, 0.0));
        // Any case, like the other by-name lookups in this file.
        assert_eq!(
            magazine_anchor_from_model("AR15_h"),
            magazine_anchor_from_model("ar15_h")
        );
    }

    /// Every VR-wieldable 25AE gun must have a grip entry, or it would anchor
    /// at the model origin with no rotation. The melee `_h` are the deliberate
    /// exception - see `melee_view_models_have_no_static_grip`.
    /// The gun mirror keeps the barrel (-X) and the sights (+Y) and swaps the
    /// side the baked hand is on, so a left-hand wield still aims where the
    /// controller points. The right hand is the authored model, untouched.
    #[test]
    fn the_gun_mirror_reflects_across_the_gun_only() {
        use cgmath::{Matrix4, SquareMatrix, Transform};

        let left = Handedness::Left.gun_mirror();
        assert_eq!(
            left.transform_vector(vec3(-1.0, 0.0, 0.0)),
            vec3(-1.0, 0.0, 0.0)
        );
        assert_eq!(
            left.transform_vector(vec3(0.0, 1.0, 0.0)),
            vec3(0.0, 1.0, 0.0)
        );
        assert_eq!(
            left.transform_vector(vec3(0.0, 0.0, 1.0)),
            vec3(0.0, 0.0, -1.0)
        );
        assert!(
            left.determinant() < 0.0,
            "a reflection, so the winding must flip"
        );
        assert_eq!(Handedness::Right.gun_mirror(), Matrix4::identity());
    }

    /// The left grip is the right grip with its thumb-side component
    /// reflected, matching the reflected geometry it seats.
    #[test]
    fn the_left_grip_mirrors_the_thumb_side_offset() {
        let right = VRHandModelPerHandAdjustments::new().with_offset(vec3(0.05, 0.12, 0.10));
        let left = right.clone().flip_x();
        assert_eq!(left.offset, vec3(-0.05, 0.12, 0.10));
        assert_eq!(left.rotation, right.rotation);
    }

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

    /// The seated arm must run back along the hand frame's +Z, with the weapon
    /// standing up out of the fist in the hand's vertical plane. The shipped
    /// single yaw
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
    /// Both hands: the rigs are authored right-handed and the left hand draws
    /// them mirrored, so this is exactly the assertion a mirror could silently
    /// break - a reflection applied to the model but not to the contact offset
    /// would leave the damage volume on the wrong side of the hand while the
    /// weapon still looked right.
    #[test]
    fn the_rendered_weapon_head_lands_on_the_contact_collider() {
        use cgmath::{EuclideanSpace, InnerSpace, Matrix4, Point3, Transform};

        for (name, arm) in posed_melee_arms() {
            for hand in [Handedness::Right, Handedness::Left] {
                // What `VirtualHand::SetPositionRotation` composes for a hand
                // at the origin with no rotation: entity = T(grip.offset).
                let entity = Matrix4::from_translation(melee_contact_offset(arm));
                let rendered = entity * melee_wield_pose_correction(arm, hand);

                let head = rendered.transform_point(Point3::from_vec(arm.weapon));
                let collider = Point3::from_vec(melee_contact_offset(arm));
                assert!(
                    (head - collider).magnitude() < 1e-4,
                    "{name} ({hand:?}): rendered weapon head {head:?} is off the collider {collider:?}"
                );

                let fist = rendered.transform_point(Point3::from_vec(arm.fist));
                assert!(
                    fist.to_vec().magnitude() < 1e-4,
                    "{name} ({hand:?}): rendered fist {fist:?} is off the tracked hand"
                );
            }
        }
    }

    /// The contact point must sit on the hand's own centreline (x = 0).
    ///
    /// This is what lets the collider be hand-agnostic while the mesh is not:
    /// the left-hand mirror reflects across exactly this axis, so a contact
    /// point on it does not move, and the damage volume is in the same place
    /// for either hand. It is a consequence of `melee_wield_alignment` rolling
    /// the weapon into the YZ plane rather than something anyone tuned - but
    /// the left-hand wield's "baked fist lands on the tracked hand" depends on
    /// it, so a change to the alignment that broke it must fail here rather
    /// than quietly put the left hand's fist off the controller.
    #[test]
    fn the_contact_point_is_on_the_hands_centreline() {
        for (name, arm) in posed_melee_arms() {
            let contact = melee_contact_offset(arm);
            assert!(
                contact.x.abs() < 1e-4,
                "{name}: contact {contact:?} is off the hand's centreline, so the \
                 left-hand mirror would move the damage volume"
            );
        }
    }

    /// A mirror that silently no-ops is the likely failure mode, so pin that
    /// the left hand really is drawn as the reflection of the right: a
    /// negative-determinant transform (which is also what flips the winding)
    /// that maps every mesh vertex to the right hand's with X negated.
    ///
    /// Note the three *joints* cannot witness this: `melee_wield_alignment`
    /// seats the whole rig on the hand's x=0 plane by construction (fist at the
    /// origin, forearm along +Z, weapon in the YZ plane), so they land in the
    /// same place either way. It is the arm and weapon geometry *around* that
    /// plane which swaps sides - hence the off-axis probes, which stand in for
    /// mesh vertices.
    #[test]
    fn the_left_hand_wields_the_mirror_image_of_the_right() {
        use cgmath::{EuclideanSpace, InnerSpace, Point3, SquareMatrix, Transform};

        for (name, arm) in posed_melee_arms() {
            let right = melee_wield_pose_correction(arm, Handedness::Right);
            let left = melee_wield_pose_correction(arm, Handedness::Left);

            assert!(
                right.determinant() > 0.0,
                "{name}: the right hand must not be mirrored"
            );
            assert!(
                left.determinant() < 0.0,
                "{name}: the left hand is not mirrored - the arm would render right-handed"
            );

            let probes = [
                arm.fist + vec3(0.1, 0.0, 0.0),
                arm.fist + vec3(0.0, 0.1, 0.0),
                arm.fist + vec3(0.0, 0.0, 0.1),
                arm.elbow + vec3(0.05, -0.05, 0.05),
                arm.weapon + vec3(-0.05, 0.05, 0.05),
            ];
            let mut saw_a_difference = false;
            for probe in probes {
                let probe = Point3::from_vec(probe);
                let r = right.transform_point(probe);
                let l = left.transform_point(probe);
                let mirrored = Point3::new(-r.x, r.y, r.z);
                assert!(
                    (l - mirrored).magnitude() < 1e-4,
                    "{name}: left-hand vertex {l:?} is not the right hand's {r:?} mirrored"
                );
                saw_a_difference |= (l - r).magnitude() > 1e-3;
            }
            assert!(
                saw_a_difference,
                "{name}: the left hand renders identically to the right - the mirror no-opped"
            );

            // The properties the alignment guarantees survive the reflection:
            // the forearm still runs back along the hand's +Z, and the weapon
            // still stands up out of the fist.
            let forearm = left.transform_point(Point3::from_vec(arm.elbow))
                - left.transform_point(Point3::from_vec(arm.fist));
            assert!(
                forearm.normalize().dot(Vector3::unit_z()) > 0.999,
                "{name}: the mirrored forearm left the hand's arm axis"
            );
            assert!(
                melee_contact_offset(arm).y > 0.0,
                "{name}: the weapon points down out of the fist"
            );
        }
    }

    /// The wield-scale knob must move the drawn weapon *without* breaking the
    /// two things the placement guarantees: the baked fist stays on the
    /// controller and the rendered weapon head stays on the contact collider.
    /// Exercised through the scale-explicit form rather than the dev-param
    /// registry, so it neither mutates process-global state other tests
    /// simulate against nor depends on the default staying 1.0.
    #[test]
    fn melee_wield_scale_keeps_the_fist_and_the_collider_honest() {
        use cgmath::{EuclideanSpace, InnerSpace, Matrix4, Point3, Transform};

        for (name, arm) in posed_melee_arms() {
            for hand in [Handedness::Right, Handedness::Left] {
                let full = melee_contact_offset_scaled(arm, 1.0).magnitude();
                for scale in [0.5f32, 1.0, 1.25] {
                    let contact = melee_contact_offset_scaled(arm, scale);
                    assert!(
                        (contact.magnitude() - scale * full).abs() < 1e-4,
                        "{name} ({hand:?}) at {scale}: reach {} is not {scale}x the authored {full}",
                        contact.magnitude()
                    );

                    let entity = Matrix4::from_translation(contact);
                    let rendered = entity * melee_wield_pose_correction_scaled(arm, scale, hand);

                    let fist = rendered.transform_point(Point3::from_vec(arm.fist));
                    assert!(
                        fist.to_vec().magnitude() < 1e-4,
                        "{name} ({hand:?}) at {scale}: rendered fist {fist:?} left the tracked hand"
                    );
                    let head = rendered.transform_point(Point3::from_vec(arm.weapon));
                    assert!(
                        (head - Point3::from_vec(contact)).magnitude() < 1e-4,
                        "{name} ({hand:?}) at {scale}: rendered weapon head {head:?} left the collider"
                    );
                }
            }
        }
    }

    /// The 25AE first-person gun models are authored barrel-along -X, and
    /// `weapon_script::create_projectile` fires every VR weapon down that axis
    /// rather than carrying per-weapon aim corrections. So each of their grips
    /// must seat that -X out of the hand (-Z): an entry that seats a gun some
    /// other way would silently mis-aim it.
    ///
    /// Scoped to the 25AE view models on purpose. Several classic world models
    /// (`atek_w`, `ar15_w`, `sg_w`, `gren_w`, `viro_w`, `al_w`) are authored
    /// barrel-along Z - their grips satisfy this assertion but their barrels
    /// do not, which is the pre-existing 90 degree VR mis-aim tracked in #1034.
    /// Asserting over them would certify that gap as correct.
    ///
    /// The melee `_h` are excluded twice over: they fire nothing, and they
    /// deliberately have no table entry at all (their grip is computed per
    /// wield - see `melee_view_models_have_no_static_grip`).
    #[test]
    fn gun_grips_aim_the_barrel_out_of_the_hand() {
        let guns = VR_25AE_VIEW_MODELS
            .iter()
            .copied()
            .filter(|name| !MELEE_VIEW_MODELS.contains(name));
        for name in guns {
            // flip_x mirrors only `scale`, so both hands share this rotation -
            // checking both is what pins that.
            for handedness in [Handedness::Left, Handedness::Right] {
                let rotation = get_vr_hand_model_adjustments_from_model(name, handedness).rotation;
                let barrel = rotation * vec3(-1.0, 0.0, 0.0);
                let hand_forward = vec3(0.0, 0.0, -1.0);
                assert!(
                    (barrel - hand_forward).magnitude() < 1e-4,
                    "{name} ({handedness:?}) points its barrel at {barrel:?}, not out of the hand"
                );
            }
        }
    }

    /// One definition of "the other hand". The glove and the melee arm rig are
    /// both authored right-handed and both drawn from `Handedness::mirror`; if
    /// that stopped being a reflection across the hand's X, a left-hand player
    /// would see a glove and an arm disagreeing about which hand they are.
    #[test]
    fn the_hand_mirror_reflects_across_the_hands_x() {
        use cgmath::{Matrix4, SquareMatrix};

        assert_eq!(Handedness::Right.mirror(), Matrix4::identity());
        assert!(
            Handedness::Left.mirror().determinant() < 0.0,
            "the left hand must be a reflection, or nothing is mirrored"
        );

        for point in [vec3(0.3, 0.4, 0.5), vec3(-1.0, 0.0, 2.0)] {
            assert_eq!(Handedness::Right.mirror_point(point), point);
            assert_eq!(
                Handedness::Left.mirror_point(point),
                vec3(-point.x, point.y, point.z),
                "the mirror must negate X only - Y/Z carry the forearm and weapon axes"
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
