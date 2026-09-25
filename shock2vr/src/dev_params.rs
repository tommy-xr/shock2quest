//! Live-tunable developer parameters.
//!
//! A small registry of runtime tuning knobs that used to be module-private
//! `const`s (and so cost a rebuild - and on the Quest, a redeploy - per
//! nudge). Each parameter is declared once in the [`dev_params!`] invocation
//! below; consumers read it through [`get`] every frame, so a [`set`] is
//! visible on the next frame with no plumbing.
//!
//! Storage is a static table plus one `AtomicU32` of `f32` bits per parameter:
//! lock-free, safe from any thread, and needing no `&mut Game`. That is what
//! lets the debug runtime's `GET`/`POST /v1/dev-params` handlers read and
//! write the registry directly (no `RuntimeCommand` round-trip), and what will
//! let the Developer menu screen do the same from the game thread.
//!
//! Reads are individually atomic but deliberately unsynchronized across a
//! frame: a [`set`] landing mid-frame can be seen by later reads in the same
//! frame (e.g. a panel hit-test and its render disagreeing by one frame).
//! That is acceptable for tuning knobs - the next frame is consistent - and
//! in the stepped debug runtime it never happens at all, because HTTP sets
//! land between frames.
//!
//! Ground rule: **if the consumer does not read the parameter every frame, it
//! does not belong in this table yet.** Values latched at construction time
//! (feature gates, collider dimensions, anything baked into a scene object)
//! would need a poke/rebuild path this registry deliberately does not have.

use std::sync::atomic::{AtomicU32, Ordering};

/// Categories are navigation metadata; only visualization descendants participate
/// in bulk on/off. Stable HTTP parameter keys remain independent of this tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DevCategory {
    Root,
    Visualizations,
    Interaction,
    Combat,
    Hands,
    Fit,
    Body,
    Weapons,
    Recoil,
    Melee,
    Throwing,
    Camera,
    Lighting,
    OrganicShine,
    Horde,
}

impl DevCategory {
    pub const ALL: [Self; 15] = [
        Self::Root,
        Self::Visualizations,
        Self::Interaction,
        Self::Combat,
        Self::Hands,
        Self::Fit,
        Self::Body,
        Self::Weapons,
        Self::Recoil,
        Self::Melee,
        Self::Throwing,
        Self::Camera,
        Self::Lighting,
        Self::OrganicShine,
        Self::Horde,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Root => "Developer",
            Self::Visualizations => "Visualizations",
            Self::Interaction => "Hands & zones",
            Self::Combat => "Combat",
            Self::Hands => "Hands & gloves",
            Self::Fit => "Fit experiment",
            Self::Body => "Body inventory",
            Self::Weapons => "Weapons",
            Self::Recoil => "Recoil",
            Self::Melee => "Melee",
            Self::Throwing => "Throwing",
            Self::Camera => "Camera & view",
            Self::Lighting => "Lighting",
            Self::OrganicShine => "Organic shine",
            Self::Horde => "Earth horde",
        }
    }

    pub fn parent(self) -> Option<Self> {
        match self {
            Self::Root => None,
            Self::Interaction | Self::Combat => Some(Self::Visualizations),
            Self::Fit => Some(Self::Hands),
            Self::Recoil | Self::Melee | Self::Throwing => Some(Self::Weapons),
            Self::OrganicShine => Some(Self::Lighting),
            _ => Some(Self::Root),
        }
    }

    pub fn contains(self, mut category: Self) -> bool {
        loop {
            if category == self {
                return true;
            }
            let Some(parent) = category.parent() else {
                return false;
            };
            category = parent;
        }
    }
}

/// How a parameter's value is interpreted and constrained.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DevParamKind {
    /// A float clamped to `min..=max`; [`set`] snaps to the nearest multiple
    /// of `step` from `min` (the grid the future menu's `<`/`>` arrows walk).
    Float { min: f32, max: f32, step: f32 },
    /// An on/off switch, stored in the same f32 bits as 0.0/1.0. Anything
    /// non-zero [`set`] receives becomes 1.0, so a caller cannot store a
    /// third state.
    Bool,
    // `Enum(&'static [&'static str])` goes here when a param needs it -
    // stored as the variant index in the same bits.
}

/// One registered parameter: a stable string key (HTTP + future persistence),
/// a human-readable label (the future menu row), its kind, and its default.
#[derive(Debug)]
pub struct DevParam {
    pub key: &'static str,
    pub label: &'static str,
    pub kind: DevParamKind,
    pub default: f32,
    /// Menu category, independent of the stable HTTP key.
    pub category: DevCategory,
    /// Settled tuning: hidden in the ordinary tree, editable under Locked.
    pub locked: bool,
    /// Optional names for the false/true states of a two-choice control.
    pub bool_labels: Option<[&'static str; 2]>,
}

/// Index into [`PARAMS`]; obtained from the generated consts or [`find`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DevParamId(usize);

/// Declares the parameter table. One line per parameter generates its
/// [`PARAMS`] entry *and* its `pub const` id, so the two cannot drift.
/// One declaration's [`DevParam`] entry. Split out of [`dev_params!`] so the
/// table can mix kinds: the outer macro matches each line as
/// `kind(args...)` and defers the shape of `args` to these arms.
macro_rules! dev_param_entry {
    (float_locked($($args:tt)*)) => { DevParam { locked: true, ..dev_param_entry!(float($($args)*)) } };
    (bool_locked($($args:tt)*)) => { DevParam { locked: true, ..dev_param_entry!(bool($($args)*)) } };
    (bool($key:literal, $label:literal, $default:expr, $off:literal, $on:literal)) => {
        DevParam {
            bool_labels: Some([$off, $on]),
            ..dev_param_entry!(bool($key, $label, $default))
        }
    };
    (float($key:literal, $label:literal, $default:expr, $min:expr, $max:expr, $step:expr)) => {
        DevParam {
            category: DevCategory::Root,
            locked: false,
            bool_labels: None,
            key: $key,
            label: $label,
            kind: DevParamKind::Float {
                min: $min,
                max: $max,
                step: $step,
            },
            default: dev_param_default!(float($key, $label, $default, $min, $max, $step)),
        }
    };
    (bool($key:literal, $label:literal, $default:expr)) => {
        DevParam {
            category: DevCategory::Root,
            locked: false,
            bool_labels: None,
            key: $key,
            label: $label,
            kind: DevParamKind::Bool,
            default: dev_param_default!(bool($key, $label, $default)),
        }
    };
}

/// One declaration's default, as the `f32` the value table stores. A `bool`
/// cannot be `as f32`, and this is the one place that conversion belongs.
macro_rules! dev_param_default {
    (float_locked($($args:tt)*)) => { dev_param_default!(float($($args)*)) };
    (bool_locked($($args:tt)*)) => { dev_param_default!(bool($($args)*)) };
    (bool($key:literal, $label:literal, $default:expr, $off:literal, $on:literal)) => {
        dev_param_default!(bool($key, $label, $default))
    };
    (float($key:literal, $label:literal, $default:expr, $min:expr, $max:expr, $step:expr)) => {
        ($default as f32)
    };
    (bool($key:literal, $label:literal, $default:expr)) => {
        if $default { 1.0f32 } else { 0.0f32 }
    };
}

macro_rules! dev_params {
    ($($(#[$doc:meta])* $id:ident = $category:ident :: $kind:ident ( $($args:tt)* )),+ $(,)?) => {
        /// Every registered parameter, in declaration order (index == id).
        pub static PARAMS: &[DevParam] = &[
            $(DevParam { category: DevCategory::$category, ..dev_param_entry!($kind($($args)*)) }),+
        ];

        /// Current values, as `f32` bits, seeded with the defaults. A sized
        /// array, not a `&[_]` slice: a borrow of interior-mutable data
        /// cannot be promoted to `'static` (E0492).
        static VALUES: [AtomicU32; [$(dev_param_default!($kind($($args)*))),+].len()] =
            [$(AtomicU32::new(dev_param_default!($kind($($args)*)).to_bits())),+];

        #[allow(non_camel_case_types, clippy::upper_case_acronyms, dead_code)]
        enum __DevParamIndex { $($id),+ }

        $($(#[$doc])* pub const $id: DevParamId = DevParamId(__DevParamIndex::$id as usize);)+
    };
}

dev_params! {
    /// Applied once to fresh horde runs; the cheat explicitly applies it mid-run.
    HORDE_START_WAVE = Horde::float("horde_start_wave", "Start / jump to wave", 1.0, 1.0, 100.0, 1.0),
    /// Pauses new installer movement/construction; existing turrets remain threats.
    HORDE_TECH_ENABLED = Horde::bool("horde_tech_enabled", "Tech builders", true),
    HORDE_TECH_WAVE = Horde::float("horde_tech_wave", "Builder first wave", 5.0, 1.0, 20.0, 1.0),
    /// Read live during construction; changing it adjusts remaining work.
    HORDE_TECH_BUILD_SECONDS = Horde::float("horde_tech_build_seconds", "Build seconds", 25.0, 5.0, 120.0, 5.0),
    /// Earliest normal-mode wave that permits growth and new pods. Protection
    /// still counts down during earlier combat; diagnostic mode bypasses this gate.
    HORDE_GROWTH_WAVE = Horde::float("horde_growth_wave", "Growth first wave", 4.0, 1.0, 20.0, 1.0),
    /// Combat seconds from bare to maximum infestation after protection expires.
    HORDE_GROWTH_SECONDS = Horde::float("horde_growth_seconds", "Growth seconds", 180.0, 30.0, 600.0, 15.0),
    /// Global VR hand-frame offset along controller-local -Z, in centimeters.
    /// Includes menu gloves, wrist UI, held items and interactions; negative pulls back.
    GLOVE_FORWARD_CM = Hands::float_locked("glove_forward_cm", "Glove forward cm", -15.0, -20.0, 20.0, 0.5),
    /// Mirrored local X offset: positive moves right for the right hand, left for the left.
    GLOVE_SIDE_CM = Hands::float("glove_side_cm", "Glove side cm", 0.0, -10.0, 10.0, 0.5),
    /// Controller-local +Y offset, rotating with the hand rather than world up.
    GLOVE_UP_CM = Hands::float("glove_up_cm", "Glove up cm", 0.0, -10.0, 10.0, 0.5),
    /// Fit-scene-only model size, about the hand origin; does not rescale tracking.
    GLOVE_FIT_SIZE = Fit::float("glove_fit_size", "Glove size", 1.0, 0.5, 1.5, 0.01),
    /// Hide the fit gloves to compare the real hand silhouette in passthrough.
    GLOVE_FIT_VISIBLE = Fit::bool("glove_fit_visible", "Fit gloves", true),
    /// Quest only: compare grip pose against gameplay's current aim pose.
    GLOVE_FIT_GRIP_POSE = Fit::bool("glove_fit_grip_pose", "Hand pose", false, "Aim", "Grip"),
    /// Quest only: show the room behind debug_gloves/debug_psi_fit. Other scenes stay opaque.
    GLOVE_FIT_PASSTHROUGH = Fit::bool("glove_fit_passthrough", "Fit passthrough", true),
    /// Live VR amp fit, relative to the authored hand placement. Also used by debug_psi_fit.
    PSI_AMP_FORWARD_CM = Hands::float("psi_amp_forward_cm", "Psi amp forward cm", 2.0, -20.0, 20.0, 0.5),
    PSI_AMP_UP_CM = Hands::float("psi_amp_up_cm", "Psi amp up cm", 0.0, -20.0, 20.0, 0.5),
    PSI_AMP_SCALE = Hands::float("psi_amp_scale", "Psi amp scale", 0.40, 0.25, 2.0, 0.01),
    /// Roll about hand-local forward (-Z), around the hand origin before fit translation.
    PSI_AMP_LEFT_ROLL_DEG = Hands::float("psi_amp_left_roll_deg", "Psi amp left roll deg", 90.0, -180.0, 180.0, 5.0),
    PSI_AMP_RIGHT_ROLL_DEG = Hands::float("psi_amp_right_roll_deg", "Psi amp right roll deg", -90.0, -180.0, 180.0, 5.0),
    /// Yaw about hand-local up (+Y), after roll and before fit translation.
    PSI_AMP_LEFT_YAW_DEG = Hands::float("psi_amp_left_yaw_deg", "Psi amp left yaw deg", 0.0, -180.0, 180.0, 5.0),
    PSI_AMP_RIGHT_YAW_DEG = Hands::float("psi_amp_right_yaw_deg", "Psi amp right yaw deg", 0.0, -180.0, 180.0, 5.0),
    /// How far ahead of the head the VR frontend/pause panel hangs, in world
    /// units. Default matches the old `ui::FRONTEND_PANEL_DISTANCE` const.
    FRONTEND_PANEL_DISTANCE = Camera::float("panel_distance", "Panel distance", 2.0, 0.5, 6.0, 0.1),
    /// How dark the world goes behind the pause panel in VR: 0 leaves it
    /// untouched, 1 blacks it out. Default matches the old
    /// `pause_menu::WORLD_DIM_STRENGTH` const.
    WORLD_DIM_STRENGTH = Camera::float("dim_strength", "Pause dim", 0.72, 0.0, 1.0, 0.02),
    /// Vertical offset applied to the whole tracked stage on the Quest, in
    /// **meters** (the stage's own unit): head, hands and the per-eye view
    /// move together, because `oculus_runtime` adds it inside its single
    /// stage-to-pawn mapping. The per-eye cap that holds a *tracked* head
    /// inside the collider crown is raised by the same offset, so the knob
    /// moves the view by exactly what it moves the hands by rather than
    /// saturating the view a couple of centimeters in (standing headroom above
    /// the collider center is only ~0.85 m, which an adult's tracked eye
    /// already nearly fills). At the default 0 the cap is unchanged.
    /// Deliberately NOT read by
    /// `player_eye_height_for` (the flat/debug camera): on the Quest the eye
    /// is the tracked pose, which never goes through that accessor - wiring
    /// the accessor instead would leave the knob inert exactly where live
    /// tuning matters while the debug runtime falsely "verified" it (the PR1
    /// deferral). Consequently the debug runtime cannot exercise this knob;
    /// it needs a worn check on device.
    EYE_HEIGHT_OFFSET = Camera::float("eye_offset", "Eye height (m)", 0.0, -0.5, 0.5, 0.02),
    /// Live override for the flat runtimes' projection FOV, in degrees. `0`
    /// (the default) means "no override - use `Game::desired_fov_deg()`";
    /// any positive value forces that FOV instead, for checking the flat
    /// projection without a rebuild. `oculus_runtime`
    /// is untouched: OpenXR view FOVs must be used as-is. (`debug_runtime`
    /// shares its single flat projection between flat and `--vr` mode, so this
    /// override reaches both there.)
    FOV_OVERRIDE_DEG = Camera::float("fov_override_deg", "FOV override", 0.0, 0.0, 120.0, 1.0),
    /// Maximum flat Q/E eye displacement in SS2 feet, read every frame.
    /// 2 feet is twice the original lean; 0 disables displacement and roll.
    /// Geometry still limits the resolved pose. VR uses tracked head motion.
    FLAT_LEAN_DISTANCE = Camera::float("flat_lean_distance", "Max lean (ft)", 2.0, 0.0, 4.0, 0.1),
    /// Scale the world ambient floor and model ambient: 0 disables, 1 preserves.
    AMBIENT_LIGHT_INTENSITY = Lighting::float("ambient_light_intensity", "Ambient intensity", 1.0, 0.0, 3.0, 0.05),
    /// Scale baked world lighting independently of the ambient floor and spotlights.
    LEVEL_LIGHT_INTENSITY = Lighting::float("level_light_intensity", "Level light intensity", 1.0, 0.0, 3.0, 0.05),
    /// Weapon-handling test overrides only: 0 follows the character sheet,
    /// 1–6 selects a live test level without changing stats or saves. Strength
    /// affects physical gun recoil and optional weight; Agility affects recoil.
    GUN_STRENGTH_OVERRIDE = Weapons::float("gun_strength_override", "Gun STR ovrd", 0.0, 0.0, 6.0, 1.0),
    GUN_AGILITY_OVERRIDE = Weapons::float("gun_agility_override", "Gun AGI ovrd", 0.0, 0.0, 6.0, 1.0),
    /// HRM test override: every HRM board - hack, repair and modify alike - is
    /// dealt all mines and every roll fails, so a critical failure (a repair
    /// destroying its gun) is drivable headlessly.
    HRM_FORCE_CRITICAL = Weapons::bool("hrm_force_critical", "Force HRM critical", false),
    /// Throw feel and balance. Launch values apply to the next release; damage
    /// values are read at impact. Tracking/teleport rejection stays fixed.
    THROW_SPEED_SCALE = Throwing::float("throw_speed_scale", "Speed scale", 1.0, 0.0, 3.0, 0.1),
    THROW_SPIN_SCALE = Throwing::float("throw_spin_scale", "Spin scale", 1.0, 0.0, 3.0, 0.1),
    THROW_MAX_SPEED = Throwing::float("throw_max_speed", "Max speed (u/s)", 12.0, 0.5, 20.0, 0.5),
    THROW_MAX_SPIN = Throwing::float("throw_max_spin", "Max spin (rad/s)", 25.0, 0.0, 50.0, 1.0),
    THROW_SMOOTHING_MS = Throwing::float("throw_smoothing_ms", "Motion smoothing (ms)", 50.0, 0.0, 100.0, 5.0),
    THROW_STRENGTH_OVERRIDE = Throwing::float("throw_strength_override", "Strength override", 0.0, 0.0, 6.0, 1.0),
    THROW_STRENGTH_BONUS = Throwing::float("throw_strength_bonus", "Strength speed bonus", 0.25, 0.0, 1.0, 0.05),
    THROW_WEIGHT_EXPONENT = Throwing::float("throw_weight_exponent", "Weight slowdown", 0.25, 0.0, 1.0, 0.05),
    THROW_STRENGTH_WEIGHT_RELIEF = Throwing::float("throw_strength_weight_relief", "Strength weight relief", 0.5, 0.0, 1.0, 0.1),
    THROW_MIN_SPEED = Throwing::float("throw_min_speed", "Min throw speed (u/s)", 1.5, 0.1, 5.0, 0.1),
    THROW_IMPACT_MIN_SPEED = Throwing::float("throw_impact_min_speed", "Min impact speed (u/s)", 2.0, 0.1, 10.0, 0.1),
    THROW_DAMAGE_SPEED = Throwing::float("throw_damage_speed", "Full damage speed (u/s)", 6.0, 0.5, 20.0, 0.5),
    THROW_ORGANIC_CAP = Throwing::float("throw_organic_cap", "Organic damage cap", 2.0, 0.0, 2.0, 1.0),
    THROW_INORGANIC_CAP = Throwing::float("throw_inorganic_cap", "Inorganic damage cap", 1.0, 0.0, 1.0, 1.0),
    THROW_DAMAGE_WINDOW = Throwing::float("throw_damage_window", "Damage window (s)", 5.0, 0.1, 10.0, 0.1),
    /// Per-axis gain for new physical gun recoil impulses (baseline and extra
    /// one-hand spring). 1 preserves the profile; 0 disables new kick on that
    /// axis. Existing displacement caps and recovery rates remain unchanged.
    GUN_KICKBACK_SCALE = Recoil::float("gun_kickback_scale", "Back scale", 4.0, 0.0, 10.0, 0.1),
    GUN_PITCH_SCALE = Recoil::float("gun_pitch_scale", "Pitch scale", 6.0, 0.0, 10.0, 0.1),
    GUN_YAW_SCALE = Recoil::float("gun_yaw_scale", "Yaw scale", 5.0, 0.0, 10.0, 0.1),
    /// Additional one-handed recoil only; support already removes this spring.
    /// Applied alongside per-axis gains after Strength, without changing weight.
    GUN_ONE_HAND_SCALE = Recoil::float("gun_one_hand_scale", "1-hand scale", 4.0, 0.0, 10.0, 0.1),
    /// Flatscreen viewmodel recoil gain, applied after the per-axis gains and
    /// Strength. The flat gun is a camera-anchored viewmodel rather than a
    /// tracked object, so the same authored kick reads at a different size on
    /// screen; this rescales it (caps included) without touching VR.
    FLAT_RECOIL_SCALE = Recoil::float("flat_recoil_scale", "Flat scale", 1.0, 0.25, 5.0, 0.25),
    /// How much of the flat viewmodel's recoil the shot itself follows, the
    /// flat stand-in for VR's muzzle-launched shots. 0 keeps recoil purely
    /// cosmetic (the shot always leaves along the crosshair); 1 makes a shot
    /// fired mid-kick ride the full displacement, so sustained fire walks up.
    /// A settled gun fires exactly on the crosshair at every setting.
    FLAT_RECOIL_AIM = Recoil::float("flat_recoil_aim", "Flat aim follow", 1.0, 0.0, 1.0, 0.1),
    /// Ceiling on how fast a physically simulated held melee weapon may be
    /// driven onto its tracked-hand target, in world units per second.
    ///
    /// The drive asks for exactly the velocity that lands the weapon on the
    /// hand this step, so in ordinary use the clamp never binds - a very fast
    /// human swing is well under it. It exists for the discontinuous case: a
    /// teleport or a restore can put the target a whole level away, and
    /// without a ceiling that becomes one enormous impulse into the geometry.
    /// Read before every physics step, so a headset can tune it without a
    /// rebuild.
    MELEE_MAX_SPEED = Melee::float_locked("melee_max_speed", "Melee speed", 60.0, 1.0, 200.0, 5.0),
    /// The angular counterpart of [`MELEE_MAX_SPEED`], in radians per second.
    MELEE_MAX_TURN = Melee::float_locked("melee_max_turn", "Melee turn", 60.0, 1.0, 200.0, 5.0),
    /// Minimum closing speed, in world units per second, at which a held melee
    /// weapon damages what it touches. This is the shipped VR melee rule: a
    /// swing damages because it was moving, not because a button was down,
    /// with the value acting as the swing/graze threshold so a weapon resting
    /// against a creature does nothing.
    ///
    /// The default sits in a measured gap, not a guessed one
    /// (`physics::held_melee_drive::free_swing_speed_separation`): a brisk
    /// swing peaks at **4.07** and ordinary walking carries the weapon at
    /// **1.80**. 2.0 was then tuned in-headset from that measurement.
    MELEE_FREE_SWING_SPEED = Melee::float_locked("melee_free_swing", "Free swing", 2.0, 0.5, 20.0, 0.5),
    /// Draw the tracked-hand glove *in addition to* a wielded weapon's own
    /// first-person model, instead of letting the weapon model stand in for the
    /// hand. `0` off, `1` on.
    ///
    /// This is the melee grip calibration instrument: the `_h` melee rigs bake
    /// their own arm and fist, and nothing else in the game shows where that
    /// baked fist lands relative to where the controller actually is. With both
    /// drawn at once the offset is directly visible - and directly tunable,
    /// since the registry reaches a headset without a rebuild.
    ///
    MELEE_GLOVE_OVERLAY = Interaction::bool("melee_glove_overlay", "Tracked-hand overlay", false),
    /// Draw the live contact volume of every held melee weapon as a world-space
    /// wireframe. `0` off, `1` on.
    ///
    /// The companion to [`MELEE_GLOVE_OVERLAY`]: that one answers "is the hand
    /// where my hand is", this one answers "is the damage volume where the
    /// weapon is". Read out of Rapier at the collider's own isometry, so it
    /// shows what is actually simulated rather than what the wield intended.
    MELEE_VOLUMES = Combat::bool("melee_volumes", "Melee contact volumes", false),
    /// Visualize shoulder backpack stow zones (cyan; green while reached).
    VR_BACKPACK_ZONES = Interaction::bool("vr_backpack_zones", "Backpack zones", false),
    VR_BACKPACK_RADIUS = Body::float("vr_backpack_radius", "Backpack reach radius (m)", 0.25, 0.18, 0.5, 0.01),
    VR_HOLSTER_RADIUS = Body::float("vr_holster_radius", "Holster reach radius (m)", 0.15, 0.14, 0.5, 0.01),
    VR_GLOVE_SPHERES = Interaction::bool("vr_glove_spheres", "Glove contact spheres", false),
    VR_GLOVE_RADIUS = Body::float("vr_glove_radius", "Glove contact radius (m)", 0.1, 0.02, 0.1, 0.005),
    VR_AMMO_POUCH_ZONES = Interaction::bool("vr_ammo_pouch_zones", "Ammo pouch zone", false),
    VR_HOLSTER_ZONES = Interaction::bool("vr_holster_zones", "Holster zones", false),
    /// Actual front of the curved belt, including the asset's authored offset.
    /// The pouch and personal card follow both belt settings with their grab targets.
    VR_BELT_DISTANCE = Body::float("vr_belt_distance", "Belt forward (m)", 0.20, 0.10, 0.45, 0.01),
    VR_BELT_DROP = Body::float("vr_belt_drop", "Belt below eyes (m)", 0.55, 0.30, 0.90, 0.01),
    /// Session-only handheld experiment, also available on Quest without CLI flags.
    VR_MFD_DEVICE = Body::bool("vr_mfd_device", "MFD device prototype", false),
    VR_MFD_FOCUS_SCAN = Body::bool("vr_mfd_focus_scan", "MFD scan automatically on focus", true),
    VR_MFD_MAP_WIDE = Body::bool("vr_mfd_map_wide", "MFD map: wider panel above", false),
    VR_MFD_BODY = Body::float("vr_mfd_body", "MFD body: 0 card, 1 upgrade, 2 magazine", 0.0, 0.0, 2.0, 1.0),
    VR_MFD_GRIP_MARGIN = Body::float("vr_mfd_grip_margin", "MFD lower grip bezel (m)", 0.035, 0.0, 0.08, 0.005),
    VR_MFD_WIDTH = Body::float("vr_mfd_width", "MFD face width (m)", 0.14, 0.08, 0.24, 0.01),
    VR_MFD_HOLOGRAM_SCREEN = Body::bool("vr_mfd_hologram_screen", "MFD hologram over screen", true),
    /// Optional download visuals; collection, sound and haptics remain active.
    VR_DOWNLOAD_PARTICLES = Body::bool("vr_download_particles", "Download particles", false),
    VR_HOLSTER_DROP = Body::float("vr_holster_drop", "Holster below eyes (m)", 0.78, 0.55, 1.1, 0.02),
    VR_HOLSTER_SIDE = Body::float("vr_holster_side", "Holster side (m)", 0.23, 0.16, 0.40, 0.01),
    VR_HOLSTER_FORWARD = Body::float("vr_holster_forward", "Holster forward (m)", 0.04, -0.20, 0.30, 0.01),

    /// VR support target/radius (cyan; green when attached) and tracked palm (amber).
    VR_SUPPORT_GRIPS = Interaction::bool("vr_support_grips", "Support grips", false),
    /// Draw the live creature damage colliders in both presentations.
    SHOW_HITBOXES = Combat::bool("show_hitboxes", "Creature hitboxes", false),
    /// Floating damage amounts labeled with the limb struck.
    DAMAGE_NUMBERS = Combat::bool("damage_numbers", "Damage numbers", false),
    /// Draw every held gun's clip-insert zone as world-space wire spheres:
    /// the enter radius (green; red while the other hand's item is inside -
    /// the gesture latches on any held item and only then asks whether it is
    /// a clip the gun takes), the larger exit
    /// radius (dim), and a small blue marker on the model origin the anchor
    /// is measured from. Answers "where do I have to bring the clip?" in a
    /// headset, and is how the per-model magazine anchors are placed.
    CLIP_ZONE = Interaction::bool("clip_zone", "Show clip-insert zone", false),
    /// Uniform scale applied to a wielded melee `_h` view model, about the
    /// baked fist so the grip stays on the controller.
    ///
    /// The 25AE first-person models are authored for a flat camera's own
    /// projection, where an oversized weapon reads better; VR draws them at
    /// true world scale, where the same exaggeration is simply a giant weapon.
    /// Measured on the shipped rigs (logged at every wield): the baked
    /// hand+forearm is ~57 cm against a real ~45 cm, and the weapons run
    /// 66-94 cm - a Wrench whose head sits 62 cm out of the fist.
    ///
    /// Unlike the rest of this table the value is read at *wield* time rather
    /// than every frame, because the correction is baked into the posed model
    /// once. It therefore takes effect on the next grab - one gesture in a
    /// headset, which is what this knob exists to serve - rather than
    /// instantly.
    ///
    /// The default is the balance point between the two, not a fit to either:
    /// the arm and the weapon are exaggerated by *different* factors, so no
    /// single scale makes both right. 0.7 draws a 46 cm Wrench on a 40 cm arm;
    /// making the arm exactly life-size (0.79) would leave the Wrench at 52,
    /// and making the Wrench right (~0.5) would leave a child's arm.
    MELEE_WIELD_SCALE = Melee::float_locked("melee_scale", "Melee scale", 0.7, 0.25, 1.5, 0.05),
    /// Authored lighting for world objects, held models and gloves. Disable
    /// only to compare against legacy shading during testing.
    OBJECT_LIGHTING = Lighting::bool("object_lighting", "Object lighting", true),
    /// Paint the mission's environment cubemap around the camera instead of
    /// the world, to check what reflections sample and that it is oriented
    /// like the level.
    ENV_MAP_PREVIEW = Lighting::bool("env_map_preview", "Env map preview", false),
    /// A spotlight along each VR hand's pointing ray.
    HAND_SPOTLIGHTS = Lighting::bool("hand_spotlights", "Hand spotlights", false),
    SPOTLIGHT_INTENSITY = Lighting::float("spotlight_intensity", "Spotlight intensity", 2.0, 0.0, 4.0, 0.1),
    /// Outer cone half-angle in degrees; the full-brightness core is half of it.
    SPOTLIGHT_CONE = Lighting::float("spotlight_cone", "Spotlight cone", 30.0, 5.0, 60.0, 1.0),
    SPOTLIGHT_RANGE = Lighting::float("spotlight_range", "Spotlight range", 10.0, 1.0, 20.0, 0.5),
    /// Multiplies every object light's brightness. The default 1.0 is the
    /// faithful value - brightness as authored, divided back down for our
    /// smaller world units - and exists to be turned up when the authored
    /// answer reads too dark on a modern display. Objects are legitimately
    /// dimmer than the walls behind them (lightmapped surfaces never fall
    /// fully dark, objects do), so this is a taste knob, not a correction.
    OBJECT_LIGHT_BRIGHTNESS = Lighting::float("object_light_brightness", "Obj light", 1.0, 0.0, 8.0, 0.1),
    /// Added to the mission's own ambient for objects only. The mission floor
    /// is often very low (medsci1 authors 0.078), which is faithful but leaves
    /// an object with no light on it nearly black; raise this to lift the
    /// shadows without touching what the lamps do.
    OBJECT_LIGHT_AMBIENT_BOOST = Lighting::float("object_light_ambient", "Obj ambient", 0.0, 0.0, 0.5, 0.01),
    /// How far light wraps past the terminator on objects. 0 is what the
    /// original did for objects - a face pointing away from a lamp gets
    /// nothing but ambient. 1 is the half-lambert it baked into *lightmaps*,
    /// which is why walls never go fully dark and props do. Raising this lifts
    /// a prop's shadowed side at the cost of the directional read that makes a
    /// lamp feel like a lamp.
    OBJECT_LIGHT_WRAP = Lighting::float("object_light_wrap", "Obj wrap", 0.0, 0.0, 1.0, 0.05),
    /// Strength of the moving highlight on wet organic surfaces (eggs, grubs,
    /// arachnids, growth). 0 leaves only the authored view-angle sheen.
    OBJECT_SPECULAR = Lighting::float("object_specular", "Obj specular", 1.5, 0.0, 4.0, 0.1),
    /// Strength of the mission's captured surroundings reflected in those same
    /// wet surfaces. 0 turns reflections off.
    OBJECT_REFLECTION = Lighting::float("object_reflection", "Obj reflection", 3.0, 0.0, 4.0, 0.1),
    /// How far the highlight on worm goo gathers onto procedural veins; 0
    /// spreads it evenly. Growth has no authored glint map of its own.
    GOO_VEINS = OrganicShine::float("goo_veins", "Goo veins", 1.0, 0.0, 1.0, 0.05),
    /// 0 keeps goo veins soft and swollen; 1 cuts them to thin, hard lines.
    GOO_VEIN_SHARPNESS = OrganicShine::float("goo_vein_sharpness", "Goo vein sharp", 0.0, 0.0, 1.0, 0.05),
    /// Goo's highlight strength relative to `object_specular`; at 1 its veins
    /// barely glint under a lamp.
    GOO_SPECULAR = OrganicShine::float("goo_specular", "Goo specular", 3.0, 0.0, 6.0, 0.1),
    /// As `goo_veins`, for the worm launcher and viral proliferator.
    WEAPON_VEINS = OrganicShine::float("weapon_veins", "Weapon veins", 1.0, 0.0, 1.0, 0.05),
    /// As `goo_vein_sharpness`, for the annelid weapons.
    WEAPON_VEIN_SHARPNESS = OrganicShine::float("weapon_vein_sharpness", "Wpn vein sharp", 0.0, 0.0, 1.0, 0.05),
    /// As `goo_specular`, for the annelid weapons.
    WEAPON_SPECULAR = OrganicShine::float("weapon_specular", "Wpn specular", 3.0, 0.0, 6.0, 0.1),
    /// Enables the detached debug ("free") camera. This is the *gate*, not
    /// the camera's own on/off: while it is false the toggle input is not
    /// even read, so a stray `Alt+V` (or controller chord) during normal play
    /// cannot detach the view. Turning it back off while detached also
    /// re-attaches the camera, so the switch is always a way out.
    FREE_CAMERA = Camera::bool("free_camera", "Free camera", false),
    /// Which pose the visibility engine culls from while the free camera is
    /// detached. Off (the default) culls from the *player*, so flying out
    /// shows exactly what the player's viewpoint decided - cells the player
    /// cannot see are genuinely absent, which is the point of the tool. On
    /// culls from the free camera instead, for when you just want to look at
    /// something. Inert while the camera is attached (the two poses are the
    /// same pose).
    FREE_CAMERA_CULL_FROM_CAMERA = Camera::bool("free_camera_cull", "Cull from cam", false),
    /// Draw the player's world position (X/Y/Z) as a text readout over the
    /// game. Laid out once on the shared HUD canvas; flat draws that canvas in
    /// screen space, VR presents it on a head-anchored panel. The coordinates a
    /// bug report needs are otherwise only reachable from a debug-runtime HTTP
    /// query, which a headset has no way to make.
    SHOW_POSITION = Visualizations::bool("show_position", "Show position", false),
    /// Draw collider shapes, contacts and joint anchors live. The desktop and
    /// debug runtime's `--debug-physics` flag seeds this switch before startup.
    DEBUG_PHYSICS = Visualizations::bool("debug_physics", "Physics wireframe", false),
    /// How fast the free camera flies, in the player's own speed units - the
    /// default IS [`PLAYER_MOVE_SPEED`], so "walking pace" cannot drift from
    /// what walking actually is. These are pre-scale SS2 units, not world
    /// units per second: the consumer divides by `dark::SCALE_FACTOR` (2.5)
    /// exactly as the player's locomotion does, so the default 25 travels 10
    /// world units a second, not 25. The range spans a crawl to a fast survey of a
    /// deck; live-tunable because the right speed depends entirely on what is
    /// being inspected.
    ///
    /// [`PLAYER_MOVE_SPEED`]: crate::mission::PLAYER_MOVE_SPEED
    FREE_CAMERA_SPEED = Camera::float(
        "free_camera_speed",
        "Cam speed",
        crate::mission::PLAYER_MOVE_SPEED,
        5.0,
        200.0,
        5.0
    ),
}

/// Every parameter with its id, in declaration order.
pub fn all() -> impl Iterator<Item = (DevParamId, &'static DevParam)> {
    PARAMS.iter().enumerate().map(|(i, p)| (DevParamId(i), p))
}

/// The static description of one parameter.
pub fn spec(id: DevParamId) -> &'static DevParam {
    &PARAMS[id.0]
}

/// Look a parameter up by its stable string key.
pub fn find(key: &str) -> Option<DevParamId> {
    PARAMS.iter().position(|p| p.key == key).map(DevParamId)
}

/// The parameter's current value.
pub fn get(id: DevParamId) -> f32 {
    f32::from_bits(VALUES[id.0].load(Ordering::Relaxed))
}

/// A [`DevParamKind::Bool`] parameter's current value. Any non-zero reading
/// is true, so this is also correct for a value written before the registry
/// normalized it.
pub fn get_bool(id: DevParamId) -> bool {
    get(id) != 0.0
}

/// Set a parameter, clamped into its range and snapped to its step grid.
/// Returns the value actually applied. A non-finite request is refused
/// (the current value is returned unchanged) so no consumer can ever read
/// a NaN out of the registry.
pub fn set(id: DevParamId, value: f32) -> f32 {
    let applied = apply(&spec(id).kind, get(id), value);
    VALUES[id.0].store(applied.to_bits(), Ordering::Relaxed);
    applied
}

/// The pure constrain step behind [`set`]: what `value` becomes under `kind`
/// when the parameter currently reads `current`. Split out so the clamp/snap
/// rules are testable without touching the process-global values (mutating
/// them from unit tests would race every parallel test that reads a live
/// parameter).
fn apply(kind: &DevParamKind, current: f32, value: f32) -> f32 {
    if !value.is_finite() {
        return current;
    }
    match *kind {
        DevParamKind::Bool => {
            if value == 0.0 {
                0.0
            } else {
                1.0
            }
        }
        DevParamKind::Float { min, max, step } => {
            if step > 0.0 {
                // Clamp the grid *index*, not the snapped value: clamping
                // afterwards would return `max` itself, which is off the
                // advertised grid whenever the range is not a whole number
                // of steps (and a future menu's arrows would then walk a
                // shifted grid).
                let max_index = ((max - min) / step).floor();
                let index = ((value - min) / step).round().clamp(0.0, max_index);
                (min + index * step).clamp(min, max)
            } else {
                value.clamp(min, max)
            }
        }
    }
}

/// Restore a parameter to its declared default, exactly. This stores the
/// default's own bits rather than routing through [`set`], whose snap grid
/// does not round-trip every default (e.g. 0.72 on a 0.02 grid lands on
/// 0.71999997 in f32) - the difference matters to anything that compares
/// against the default with `==` (the HTTP mirror's reset, a future menu
/// "reset" row).
pub fn reset(id: DevParamId) {
    VALUES[id.0].store(spec(id).default.to_bits(), Ordering::Relaxed);
}

#[cfg(test)]
mod tests {
    use super::*;

    // No test here (or anywhere in the crate) mutates the live registry:
    // the values are process globals read by parallel tests all over the
    // crate, so a unit-test `set` would be a cross-test race. The constrain
    // rules are tested through the pure [`apply`]; that `set`/`reset` really
    // move what consumers render is proven end-to-end by the SDK e2e
    // (`dev-params.e2e.test.ts`), which watches the VR panel move over HTTP.

    /// The registry replaced two consts; anything but these exact defaults
    /// is a behavior change at launch.
    #[test]
    fn locked_declarations_keep_their_normal_kind_and_defaults() {
        let float = dev_param_entry!(float_locked("test", "Test", 0.5, 0.0, 1.0, 0.1));
        let flag = dev_param_entry!(bool_locked("test", "Test", true, "Aim", "Grip"));
        assert!(float.locked && flag.locked);
        assert_eq!(float.default, 0.5);
        assert_eq!(flag.kind, DevParamKind::Bool);
        assert_eq!(flag.default, 1.0);
        assert_eq!(flag.bool_labels, Some(["Aim", "Grip"]));
        assert_eq!(apply(&float.kind, float.default, 0.8), 0.8);
    }

    #[test]
    fn defaults_equal_the_consts_they_replaced() {
        assert_eq!(get(FRONTEND_PANEL_DISTANCE), 2.0);
        assert_eq!(get(WORLD_DIM_STRENGTH), 0.72);
        assert_eq!(get(MELEE_MAX_SPEED), 60.0);
        assert_eq!(get(MELEE_MAX_TURN), 60.0);
    }

    /// Every declared default must be finite and inside its own range, or
    /// the seeded value would already violate the registry's contract.
    #[test]
    fn every_default_is_within_its_declared_range() {
        for (_, param) in all() {
            assert!(param.default.is_finite());
            match param.kind {
                DevParamKind::Float { min, max, step } => {
                    assert!(
                        (min..=max).contains(&param.default),
                        "{}: default {} outside {min}..={max}",
                        param.key,
                        param.default
                    );
                    assert!(step >= 0.0, "{}: negative step", param.key);
                }
                DevParamKind::Bool => assert!(
                    param.default == 0.0 || param.default == 1.0,
                    "{}: bool default {} is neither 0 nor 1",
                    param.key,
                    param.default
                ),
            }
        }
    }

    const GRID: DevParamKind = DevParamKind::Float {
        min: 0.5,
        max: 6.0,
        step: 0.1,
    };

    #[test]
    fn apply_passes_an_in_range_on_grid_value_through() {
        assert!((apply(&GRID, 2.0, 2.5) - 2.5).abs() < 1e-4);
    }

    #[test]
    fn apply_clamps_into_the_declared_range() {
        assert_eq!(apply(&GRID, 2.0, 99.0), 6.0);
        assert_eq!(apply(&GRID, 2.0, -3.0), 0.5);
    }

    #[test]
    fn apply_snaps_to_the_step_grid() {
        let kind = DevParamKind::Float {
            min: 0.0,
            max: 1.0,
            step: 0.02,
        };
        // 0.013 is between grid points 0.0 and 0.02.
        let applied = apply(&kind, 0.72, 0.013);
        assert!((applied - 0.02).abs() < 1e-4, "got {applied}");
    }

    /// A range that is not a whole number of steps must still land on the
    /// grid at its top end: snapping *then* clamping would return `max`
    /// itself, off the advertised grid.
    #[test]
    fn apply_keeps_an_uneven_ranges_top_end_on_grid() {
        let kind = DevParamKind::Float {
            min: 0.0,
            max: 0.05,
            step: 0.02,
        };
        assert!((apply(&kind, 0.0, 99.0) - 0.04).abs() < 1e-6);
    }

    /// Why [`reset`] stores the default's own bits instead of routing
    /// through the snap: the grid cannot represent every default exactly.
    #[test]
    fn the_snap_grid_does_not_round_trip_every_default() {
        let kind = DevParamKind::Float {
            min: 0.0,
            max: 1.0,
            step: 0.02,
        };
        assert_ne!(apply(&kind, 0.72, 0.72).to_bits(), 0.72f32.to_bits());
    }

    #[test]
    fn a_non_finite_value_is_refused() {
        assert_eq!(apply(&GRID, 2.0, f32::NAN), 2.0);
        assert_eq!(apply(&GRID, 2.0, f32::INFINITY), 2.0);
        assert_eq!(apply(&GRID, 2.0, f32::NEG_INFINITY), 2.0);
    }

    /// A bool normalizes to exactly 0.0/1.0, so no consumer can read a third
    /// state out of the registry (and `get_bool`'s `!= 0.0` can never see a
    /// value the panel would then format inconsistently).
    #[test]
    fn apply_normalizes_a_bool_to_zero_or_one() {
        assert_eq!(apply(&DevParamKind::Bool, 0.0, 1.0), 1.0);
        assert_eq!(apply(&DevParamKind::Bool, 1.0, 0.0), 0.0);
        assert_eq!(apply(&DevParamKind::Bool, 0.0, 0.5), 1.0);
        assert_eq!(apply(&DevParamKind::Bool, 0.0, -3.0), 1.0);
    }

    #[test]
    fn a_non_finite_bool_is_refused() {
        assert_eq!(apply(&DevParamKind::Bool, 1.0, f32::NAN), 1.0);
    }

    /// The free camera is a debug tool: it must be off until asked for.
    #[test]
    fn the_free_camera_gate_defaults_off() {
        assert!(!get_bool(FREE_CAMERA));
        assert!(!get_bool(FREE_CAMERA_CULL_FROM_CAMERA));
    }

    #[test]
    fn unknown_keys_do_not_resolve() {
        assert_eq!(find("no_such_param"), None);
        assert_eq!(find("panel_distance"), Some(FRONTEND_PANEL_DISTANCE));
    }
}
