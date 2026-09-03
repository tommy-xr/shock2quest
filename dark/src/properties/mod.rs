mod accumulator;
mod prop_ai;
mod prop_ai_alert_cap;
mod prop_ai_alertness;
mod prop_ai_aware_delay;
mod prop_ai_camera;
mod prop_ai_device;
mod prop_ai_hearing;
mod prop_ai_mode;
mod prop_ambient_hacked;
mod prop_anim_light;
mod prop_anim_tex;
mod prop_base_gun_desc;
mod prop_bitmap_animation;
mod prop_collision_type;
mod prop_creature_pose;
mod prop_ecology;
mod prop_frame_anim_config;
mod prop_frame_anim_state;
mod prop_frob_info;
mod prop_gun_state;
mod prop_hack_diff;
mod prop_hit_points;
mod prop_key;
mod prop_log;
mod prop_obj_state;
mod prop_particles;
mod prop_phys_attr;
mod prop_phys_initial_velocity;
mod prop_phys_type;
mod prop_player_gun;
mod prop_psi;
mod prop_quest_bit;
mod prop_render_type;
mod prop_replicator;
mod prop_research;
mod prop_room_gravity;
mod prop_schema_play_params;
mod prop_service;
mod prop_spawn;
mod prop_trip_flags;
mod prop_tweq;
mod prop_voice;

use num_derive::{FromPrimitive, ToPrimitive};
pub use prop_ai::*;
pub use prop_ai_alert_cap::*;
pub use prop_ai_alertness::*;
pub use prop_ai_aware_delay::*;
pub use prop_ai_camera::*;
pub use prop_ai_device::*;
pub use prop_ai_hearing::*;
pub use prop_ai_mode::*;
pub use prop_ambient_hacked::*;
pub use prop_anim_light::*;
pub use prop_anim_tex::*;
pub use prop_base_gun_desc::*;
pub use prop_bitmap_animation::*;
pub use prop_collision_type::*;
pub use prop_creature_pose::*;
pub use prop_ecology::*;
pub use prop_frame_anim_config::*;
pub use prop_frame_anim_state::*;
pub use prop_frob_info::*;
pub use prop_gun_state::*;
pub use prop_hack_diff::*;
pub use prop_hit_points::*;
pub use prop_key::*;
pub use prop_log::*;
pub use prop_obj_state::*;
pub use prop_particles::*;
pub use prop_phys_attr::*;
pub use prop_phys_initial_velocity::*;
pub use prop_phys_type::*;
pub use prop_player_gun::*;
pub use prop_psi::*;
pub use prop_quest_bit::*;
pub use prop_render_type::*;
pub use prop_replicator::*;
pub use prop_research::*;
pub use prop_room_gravity::*;
pub use prop_schema_play_params::*;
pub use prop_service::*;
pub use prop_spawn::*;
pub use prop_trip_flags::*;
pub use prop_tweq::*;
pub use prop_voice::*;

use num_traits::FromPrimitive;
use serde::{
    Deserialize, Serialize,
    de::{DeserializeOwned, Error},
};

use std::{
    collections::HashMap,
    convert::identity,
    fmt,
    io::{self, Cursor},
    time::Duration,
};

use crate::{SCALE_FACTOR, ss2_common::*};
use cgmath::{Deg, InnerSpace, Quaternion, Rotation3, Vector3, vec3};
use shipyard::{
    Component, EntityId, Get, IntoIter, IntoWithId, TupleAddComponent, View, ViewMut, World,
};

// Properties
#[derive(Debug, Component, Clone, Serialize, Deserialize)]
pub struct PropCreature(pub u32);

#[derive(Debug, Component, Clone, Serialize, Deserialize)]
pub struct PropDelayTime {
    pub delay: Duration,
}

#[derive(Debug, Component, Clone, Serialize, Deserialize)]
pub struct PropPosition {
    pub position: Vector3<f32>,
    pub cell: u16,
    pub rotation: Quaternion<f32>,
}

/// How an entity arrived at its current position when marked `PropTeleported`.
/// Distinguishes player-initiated movement (which fires tripwires on arrival,
/// like the original engine) from arrivals whose initial sensor overlaps must
/// be reconstructed without replaying tripwire ENTER.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum TeleportSource {
    /// Player-initiated movement teleport: VR teleport locomotion or a debug
    /// teleport. Tripwire ENTER fires on arrival.
    #[default]
    Locomotion,
    /// A scripted teleport trap (TrapTeleportPlayer) repositioned the player.
    /// Tripwire ENTER is suppressed for this arrival so a return teleport that
    /// lands inside another trap's tripwire box doesn't re-fire it (#515).
    ScriptedTrap,
    /// Save/load rebuilt the physics world at the serialized player position.
    /// Initial sensor overlaps are tracked, but their ENTER is not replayed;
    /// leaving and genuinely re-entering later still fires normally (#547).
    LoadRestore,
}

#[derive(Debug, Component, Serialize, Deserialize)]
/// Marks an entity as having just teleported (VR teleport locomotion, teleport
/// traps, debug teleport, or save restore). Tripwires fire on locomotion
/// teleport-entry like the original engine; the other sources reconstruct
/// their arrival without replaying ENTER.
pub struct PropTeleported {
    pub countdown_timer: f32, // Remaining time to be considered 'recently teleported'
    #[serde(default)]
    pub source: TeleportSource,
}

#[derive(Debug, Component, Clone, Serialize, Deserialize)]
pub struct PropKeypadCode(pub u32);

impl PropTeleported {
    pub fn new() -> PropTeleported {
        PropTeleported::with_source(TeleportSource::Locomotion)
    }

    pub fn with_source(source: TeleportSource) -> PropTeleported {
        PropTeleported {
            countdown_timer: 1.0,
            source,
        }
    }
}

#[derive(Debug, Component, Clone, Serialize, Deserialize)]
pub struct PropClassTag {
    raw: String,
    pub tag_values: Vec<(String, String)>,
}

impl PropClassTag {
    pub fn from_string(str: &str) -> PropClassTag {
        let raw = str.to_ascii_lowercase();

        let individual_values: Vec<&str> = raw.split(' ').collect();

        let mut tag_values = Vec::new();

        let mut i = 0;
        let len = individual_values.len();
        while i < len - 1 {
            let tag = individual_values[i].to_owned();
            let val = individual_values[i + 1].to_owned();
            tag_values.push((tag, val));
            i += 2;
        }

        PropClassTag { raw, tag_values }
    }

    pub fn class_tags(&self) -> Vec<(&str, &str)> {
        let ret: Vec<(&str, &str)> = self
            .tag_values
            .iter()
            .map(|(s1, s2)| (s1.as_str(), s2.as_str()))
            .collect();

        ret
    }
}

#[derive(Debug, Component, Clone, Serialize, Deserialize)]
pub struct PropConsumeType(pub String);

#[derive(Debug, Component, Clone, Serialize, Deserialize)]
pub struct PropDestLevel(pub String);

#[derive(Debug, Component, Clone, Serialize, Deserialize)]
pub struct PropDestLoc(pub i32);

#[derive(Debug, Component, Clone, Serialize, Deserialize)]
pub struct PropExp(pub i32);

/// The count of a stackable object (e.g. how many cyber modules an EXP-cookie
/// pile is worth - the retail engine stores an EXP cookie's module value as its
/// stack count, `P$StackCoun`). A 4-byte signed int.
#[derive(Debug, Component, Clone, Serialize, Deserialize)]
pub struct PropStackCount(pub i32);

/// The version of a piece of software (`P$SoftLevel`, 1..=3). Authored on the
/// `Softs` base archetype as 1 and overridden by the V2/V3 archetypes, so a
/// V1 soft inherits the base value. Read by the `AutoInstallSoft` script.
#[derive(Debug, Component, Clone, Serialize, Deserialize)]
pub struct PropSoftLevel(pub i32);

/// Which software slot a soft installs into (`P$SoftType`). Authored on the
/// four soft class archetypes: 1 = Hack, 2 = Modify, 3 = Repair, 4 = Research
/// (the `PDA Soft` archetype carries 0, i.e. no slot). The numbering matches
/// the `SoftUpgrade0..3` message order in `res/strings/MISC.STR`, offset by
/// one.
#[derive(Debug, Component, Clone, Serialize, Deserialize)]
pub struct PropSoftType(pub i32);

#[derive(Debug, Component, Clone, Serialize, Deserialize)]
pub struct PropLimbModel(pub String);

#[derive(Debug, Component, Clone, Serialize, Deserialize)]
pub struct PropMapLoc(pub i32);

#[derive(Debug, Component, Clone, Serialize, Deserialize)]
pub struct PropAutomap {
    pub page: i32,
    pub location: i32,
}

impl PropAutomap {
    pub fn read<T: io::Read + io::Seek>(reader: &mut T, _len: u32) -> PropAutomap {
        let page = read_i32(reader);
        let location = read_i32(reader);
        PropAutomap { page, location }
    }
}

#[derive(Debug, Component, Clone, Serialize, Deserialize)]
pub struct PropMapRef {
    pub x: i32,
    pub y: i32,
    pub frame: i32,
}

impl PropMapRef {
    pub fn read<T: io::Read + io::Seek>(reader: &mut T, _len: u32) -> PropMapRef {
        let x = read_i32(reader);
        let y = read_i32(reader);
        let frame = read_i32(reader);
        PropMapRef { x, y, frame }
    }
}

#[derive(Debug, Component, Clone, Serialize, Deserialize)]
pub struct PropMaterial(pub String);

#[derive(Debug, Component, Clone, Serialize, Deserialize)]
pub struct PropMapText(pub String);

#[derive(Debug, Component, Clone, Serialize, Deserialize)]
pub struct PropHackText(pub String);

#[derive(Debug, Component, Clone, Serialize, Deserialize)]
pub struct PropMapObjIcon(pub String);

#[derive(Debug, Component, Clone, Serialize, Deserialize)]
pub struct PropMapObjRotate(pub bool);

impl PropMapObjRotate {
    pub fn read<T: io::Read + io::Seek>(reader: &mut T, _len: u32) -> PropMapObjRotate {
        let rotate = read_bool(reader);
        PropMapObjRotate(rotate)
    }
}

#[derive(Debug, Component, Clone, Serialize, Deserialize)]
pub struct PropSymName(pub String);

#[derive(Debug, Component, Clone, Serialize, Deserialize)]
pub struct PropMotionActorTags {
    pub tags: Vec<String>,
}

impl PropMotionActorTags {
    pub fn read<T: io::Read + io::Seek>(reader: &mut T, len: u32) -> PropMotionActorTags {
        let str = read_prop_string(reader, len);
        let tags = str.split(',').map(|s| s.trim().to_owned()).collect();
        PropMotionActorTags { tags }
    }
}

#[derive(Debug, Component, Clone, Serialize, Deserialize)]
pub struct PropSelfIllumination(pub f32);

/// How quickly the player's stored radiation level is reduced after leaving
/// an irradiated environment. Authored on `The Player` as `P$RadRecove`.
#[derive(Debug, Component, Clone, Serialize, Deserialize)]
pub struct PropRadiationRecovery(pub f32);

/// How quickly ambient radiation is absorbed into the player's stored level.
/// Authored on `The Player` as `P$RadAbsorb`.
#[derive(Debug, Component, Clone, Serialize, Deserialize)]
pub struct PropRadiationAbsorb(pub f32);

/// Retail radiation-drain tuning authored on `The Player` as `P$RadDrain`.
#[derive(Debug, Component, Clone, Serialize, Deserialize)]
pub struct PropRadiationDrain(pub f32);

/// A container's inventory grid, in cells. The `Contains` link's ordinal is
/// `y * width + x` against *this* width, so it is what makes a stored cell
/// mean anything.
///
/// NOTE: the editor calls this `ContainDims`, but chunk names are stored in a
/// 12-byte field, so on disk it is `P$ContainDi` - see the registration below.
#[derive(Debug, Component, Clone, Serialize, Deserialize)]
pub struct PropContainDimensions {
    pub width: u32,
    pub height: u32,
}

impl PropContainDimensions {
    pub fn read<T: io::Read + io::Seek>(reader: &mut T, _len: u32) -> PropContainDimensions {
        let width = read_u32(reader);
        let height = read_u32(reader);
        PropContainDimensions { width, height }
    }
}

#[derive(Debug, Component, Clone, Serialize, Deserialize)]
pub struct PropInventoryDimensions {
    pub width: u32,
    pub height: u32,
}

impl PropInventoryDimensions {
    pub fn read<T: io::Read + io::Seek>(reader: &mut T, _len: u32) -> PropInventoryDimensions {
        let width = read_u32(reader);
        let height = read_u32(reader);
        PropInventoryDimensions { width, height }
    }
}

#[derive(Debug, Component, Clone, Serialize, Deserialize)]
pub struct PropObjIcon(pub String);

/// `P$Sett1` / `P$Sett2` - the description text for a gun's first / second fire
/// setting, and `P$SHead1` / `P$SHead2` - the short header shown beside it
/// (e.g. "NORM" / "BURST"). Each holds an object string (`key: "fallback"`)
/// resolved against the matching `SETT1`/`SETT2`/`SHEAD1`/`SHEAD2` string
/// table; see `dark::importers::resolve_gun_setting_string`.
#[derive(Debug, Component, Clone, Serialize, Deserialize)]
pub struct PropGunSettingText1(pub String);

#[derive(Debug, Component, Clone, Serialize, Deserialize)]
pub struct PropGunSettingText2(pub String);

#[derive(Debug, Component, Clone, Serialize, Deserialize)]
pub struct PropGunSettingHeader1(pub String);

#[derive(Debug, Component, Clone, Serialize, Deserialize)]
pub struct PropGunSettingHeader2(pub String);

#[derive(Debug, Component, Clone, Serialize, Deserialize)]
pub struct PropObjName(pub String);

/// How the original Shock UI substitutes placeholders in an object's localized
/// long/short name (`P$NameType` / `ObjNameType`).
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub enum ObjectNameType {
    #[default]
    Normal,
    StackCount,
    LogTitle,
    Weapon,
    /// Preserve unfamiliar retail/mod values rather than silently treating
    /// them as one of the known formatting strategies.
    Unknown(i32),
}

impl ObjectNameType {
    pub fn from_raw(value: i32) -> Self {
        match value {
            0 => Self::Normal,
            1 => Self::StackCount,
            2 => Self::LogTitle,
            3 => Self::Weapon,
            value => Self::Unknown(value),
        }
    }

    pub fn raw(self) -> i32 {
        match self {
            Self::Normal => 0,
            Self::StackCount => 1,
            Self::LogTitle => 2,
            Self::Weapon => 3,
            Self::Unknown(value) => value,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize, Component)]
pub struct PropObjectNameType(pub ObjectNameType);

impl PropObjectNameType {
    pub fn read<T: io::Read + io::Seek>(reader: &mut T, len: u32) -> Self {
        assert_eq!(len, 4, "P$NameType must contain one signed 32-bit value");
        Self(ObjectNameType::from_raw(read_i32(reader)))
    }
}

#[derive(Debug, Component, Clone, Serialize, Deserialize)]
pub struct PropObjShortName(pub String);

/// `P$UseMsg`: the status-line message an object shows the player - either a
/// key into USEMSG.STR or, when the value carries its own quoted text, that
/// text (see `resolve_localized_property_string`).
#[derive(Debug, Component, Clone, Serialize, Deserialize)]
pub struct PropUseMsg(pub String);

#[derive(Debug, Component, Clone, Serialize, Deserialize)]
pub struct PropPhysState {
    pub position: Vector3<f32>,
    pub rotation: Quaternion<f32>,
    pub velocity: Vector3<f32>,
    pub rot_velocity: Vector3<f32>,
}

#[derive(Debug, Component, Clone, Serialize, Deserialize)]
pub struct PropLocked(pub bool);

impl PropLocked {
    pub fn read<T: io::Read + io::Seek>(reader: &mut T, _len: u32) -> PropLocked {
        let is_locked = read_bool(reader);
        PropLocked(is_locked)
    }
}

#[derive(Debug, Component, Clone, Serialize, Deserialize)]
pub struct PropModelName(pub String);

#[derive(Debug, Component, Clone, Serialize, Deserialize)]
pub struct InternalPropOriginalModelName(pub String);

#[derive(Debug, Component, Clone, Serialize, Deserialize)]
pub struct PropScale(pub Vector3<f32>);

#[derive(Debug, Component, Clone, Serialize, Deserialize)]
pub struct PropSignalType(pub String);

#[derive(Debug, Component, Clone, Serialize, Deserialize)]
pub struct PropStartLoc(pub i32);

/// `P$CharGenRo` - the character-generation "row"/tour index (4-byte int) on
/// the station recruit deck's training-tour markers. `station.mis` objs
/// 125/126/127 carry 0/1/2, selecting which of a training year's three tours
/// the player completed; `ChooseMission` uses it (with the current career +
/// year) to look up the tour reward. See `shock2vr::player_stats`.
#[derive(Debug, Component, Clone, Serialize, Deserialize)]
pub struct PropCharGenRo(pub i32);

#[derive(Debug, Component, Clone, Serialize, Deserialize)]
pub struct PropHasRefs(pub bool);

#[derive(Debug, Component, Clone, Serialize, Deserialize)]
pub struct PropImmobile(pub bool);

#[derive(Debug, Component, Clone, Serialize, Deserialize)]
pub struct PropPhysDimensions {
    pub radius0: f32,
    pub radius1: f32,
    pub offset0: Vector3<f32>,
    pub offset1: Vector3<f32>,
    pub size: Vector3<f32>,
    pub unk1: u32,
    pub unk2: u32,
}

/// `P$MovingTer` - marks a physical object as authored moving terrain.
/// Dark stores both the current active state and its previous state.
#[derive(Debug, Component, Clone, Serialize, Deserialize)]
pub struct PropMovingTerrain {
    pub active: bool,
    pub previous_active: bool,
}

impl PropMovingTerrain {
    pub fn read<T: io::Read + io::Seek>(reader: &mut T, _len: u32) -> PropMovingTerrain {
        PropMovingTerrain {
            active: read_bool(reader),
            previous_active: read_bool(reader),
        }
    }
}

// TODO: Is there a player prop
#[derive(Debug, Component, Clone, Serialize, Deserialize)]
pub struct PropLocalPlayer {}

#[derive(Debug, Component, Clone, Serialize, Deserialize)]
pub struct PropHUDSelect(pub bool);

/// "ShowHP" ("Show HP?"): opt-in for the hit-point bar drawn above the
/// selection brackets. Separate from [`PropHUDSelect`], which only gates the
/// brackets themselves - the shipped data sets this on the creature families
/// so loot and set dressing stay bar-less.
#[derive(Debug, Component, Clone, Serialize, Deserialize)]
pub struct PropShowHP(pub bool);

/// "AI_Patrol": when true, the AI patrols a route of patrol-point objects
/// chained by `Link::AIPatrol`, walking point to point while idle.
#[derive(Debug, Component, Clone, Serialize, Deserialize)]
pub struct PropAIPatrol(pub bool);

/// "AI_PtrlRnd": when true, the patrol ability may pick any other point in
/// the connected patrol graph instead of following one outgoing branch.
#[derive(Debug, Component, Clone, Serialize, Deserialize)]
pub struct PropAIPatrolRandom(pub bool);

//  This is a backlink to the template ID from the ss2 map file
#[derive(Debug, Component, Clone, Serialize, Deserialize)]
pub struct PropTemplateId {
    pub template_id: i32,
}

// TODO
#[derive(Debug, Component, Clone, Serialize, Deserialize)]
pub struct ToTemplateLinkInfo {
    pub id: i32,
    pub dest_template_id: i32,
    pub flavor: u16,
}

#[derive(Debug, Component, Clone, PartialEq, Serialize, Deserialize)]
pub enum Link {
    AIProjectile(AIProjectileOptions),
    AIRangedWeapon,
    AIWatchObj(AIWatchOptions),
    Contains(u32),
    /// From a projectile archetype to the compatible inventory-ammo archetype
    /// (e.g. Standard Bullet -> Standard Clip).
    Clip,
    Corpse(CorpseOptions),
    Flinderize(FlinderizeOptions),
    GunFlash(GunFlashOptions),
    LandingPoint,
    Projectile(ProjectileOptions),
    Replicator,
    /// Authored candidate location for a `TrapSpawn` generator.
    SpawnPoint,
    /// Runtime ownership edge from a spawn marker to its live child.
    Spawned,
    SwitchLink,
    MissSpang,
    /// From a melee AI to the weapon archetype it strikes with (its pipe,
    /// claw, spike). The weapon carries the `StimSource` links that decide
    /// what a connecting swing does to the victim.
    Weapon,
    /// From a projectile archetype to a victim archetype class; the payload is
    /// the template id of the spang (impact effect, e.g. a blood or sparks
    /// particle group) to spawn when that projectile hits a descendant of the
    /// class (e.g. pistol bullets -> Hybrids spawns the blood spang).
    HitSpang(i32),
    /// Script target activated when a tech hack succeeds.
    Hacking,
    /// From a particle-group archetype to the object it rides
    /// ("ParticleAttachement"). On an archetype host, the engine instantiates
    /// the particle group when a concrete host is created (projectile trails,
    /// psi bolt visuals); on a concrete (mission-placed) particle entity it
    /// names the object to follow.
    ParticleAttachement(ParticleAttachOptions),
    /// Rigid physics attachment from the source object to the destination
    /// object. Dark drives the source from the destination's motion plus the
    /// authored world-space offset (tram wall/floor assemblies, lifts).
    PhysAttach(PhysAttachOptions),
    TPathInit,
    /// Mutable moving-terrain state: from an elevator/platform to the waypoint
    /// it is currently travelling toward. The original engine replaces this
    /// bare relation when an elevator is rerouted.
    TPathNext,
    TPath(TPathData),
    /// Script-authored object parameter. `RerouteElevatorButton` uses this
    /// bare relation to name the waypoint requested by each call button.
    ScriptParams,
    /// From a patrol-point object to the next patrol point on its route. An AI
    /// with `PropAIPatrol(true)` walks this chain of points while idle. The
    /// link carries no data (a bare "go to the next point" edge).
    AIPatrol,
    /// Runtime-only current patrol destination, from the AI to the marker it
    /// must resume after an interruption or save/load. Dark names this local
    /// relation `AICurrentPatrol`; it is not authored in mission data.
    AICurrentPatrol,
    /// Names a destination object for scripted teleports (CS9 eggs/rumblers,
    /// TrapTeleport family, TrapDestroyTeleport's destroy target).
    Teleport,
    /// Act/react stim source ("arSrcDesc"): the linked-to template is the stim
    /// archetype this object emits (e.g. HE Explosion -> Standard Impact at
    /// intensity 10 over a radius of 10).
    StimSource(StimSourceOptions),
    /// Act/react receptron ("Receptron"): the linked-to template is the stim
    /// archetype this object responds to (e.g. Human Vulnerability ->
    /// High Explosive: Damage x4).
    Receptron(ReceptronOptions),
}

#[derive(
    FromPrimitive,
    ToPrimitive,
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Serialize,
    Deserialize,
)]
pub enum AITargetMethod {
    StraightLine = 0,
    Arcing = 1,
    Reflecting = 2,
    Overhead = 3,
    Radius = 4,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct AIProjectileOptions {
    pub targeting_method: AITargetMethod,
    pub delay: f32,
    pub should_lead_target: bool,
    pub ammo: u32,
    pub accuracy: u32, // How is this represented?
    pub select_time: f32,
    pub joint: u32, // joint to use for projectile
    pub vhot: u32,  // vhot to use for projectile
}

impl AIProjectileOptions {
    pub fn read(reader: &mut Box<dyn ReadAndSeek>, _len: u32) -> AIProjectileOptions {
        let _unk = read_u32(reader);
        let _unk = read_u32(reader);

        let targeting_method_u32 = read_u32(reader);
        let targeting_method = AITargetMethod::from_u32(targeting_method_u32).unwrap();

        let _unk = read_u32(reader);
        let delay = read_single(reader);
        let should_lead_target = read_bool(reader);
        let ammo = read_u32(reader);

        let _unk = read_u32(reader);
        let accuracy = read_u32(reader);

        let joint = read_u32(reader);
        let vhot = read_u32(reader);

        let select_time = read_single(reader);
        let _unk = read_u32(reader);

        // let speed = read_single(reader) / SCALE_FACTOR;
        // let time = read_single(reader);
        // let limit = read_bool(reader);
        // let paused = read_u32(reader);
        AIProjectileOptions {
            targeting_method,
            delay,
            should_lead_target,
            ammo,
            accuracy,
            select_time,
            joint,
            vhot,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct TPathData {
    pub speed: f32,
}

impl TPathData {
    pub fn read(reader: &mut Box<dyn ReadAndSeek>, _len: u32) -> TPathData {
        let speed = read_single(reader) / SCALE_FACTOR;
        let _time = read_single(reader);
        let _limit = read_bool(reader);
        let _paused = read_u32(reader);
        TPathData { speed }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProjectileOptions {
    pub order: i32,
    pub setting: i32,
}

impl ProjectileOptions {
    pub fn read(reader: &mut Box<dyn ReadAndSeek>, _len: u32) -> ProjectileOptions {
        let order = read_i32(reader);
        let setting = read_i32(reader);
        ProjectileOptions { order, setting }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct CorpseOptions {
    propagate_scale: bool,
}

impl CorpseOptions {
    pub fn read(reader: &mut Box<dyn ReadAndSeek>, _len: u32) -> CorpseOptions {
        let propagate_scale = read_bool(reader);
        CorpseOptions { propagate_scale }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct ParticleAttachOptions {
    /// 0 = object, 1 = vhot, 2 = joint, 3 = submodel.
    pub attach_type: u32,
    pub vhot: i32,
    pub joint: i32,
    pub submodel: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct PhysAttachOptions {
    pub offset: Vector3<f32>,
}

impl PhysAttachOptions {
    pub fn read(reader: &mut Box<dyn ReadAndSeek>, _len: u32) -> PhysAttachOptions {
        PhysAttachOptions {
            offset: read_vec3(reader) / SCALE_FACTOR,
        }
    }
}

impl ParticleAttachOptions {
    pub fn read(reader: &mut Box<dyn ReadAndSeek>, _len: u32) -> ParticleAttachOptions {
        let attach_type = read_u32(reader);
        let vhot = read_i32(reader);
        let joint = read_i32(reader);
        let submodel = read_i32(reader);
        ParticleAttachOptions {
            attach_type,
            vhot,
            joint,
            submodel,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct GunFlashOptions {
    pub vhot: u32,
    pub flags: u32,
}

impl GunFlashOptions {
    pub fn read(reader: &mut Box<dyn ReadAndSeek>, _len: u32) -> GunFlashOptions {
        let vhot = read_u32(reader);
        let flags = read_u32(reader);
        GunFlashOptions { vhot, flags }
    }
}

/// How a stim source spreads from its object (the "Propagator" in DromEd's
/// act/react sources editor).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum StimPropagator {
    Null,
    /// Stimulates objects touching the source (melee weapons, hazards).
    Contact,
    /// Stimulates everything within `radius` of the source (explosions).
    Radius {
        radius: f32,
    },
    Unknown(u32),
}

/// Act/react stim source (L$arSrcDesc): the source object emits the stim
/// archetype the link points to, at `intensity`, spread by `propagator`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct StimSourceOptions {
    pub intensity: f32,
    pub propagator: StimPropagator,
}

impl StimSourceOptions {
    pub fn read(reader: &mut Box<dyn ReadAndSeek>, _len: u32) -> StimSourceOptions {
        // sStimSourceDesc (108 bytes): propagator id, intensity, then
        // propagator-specific params (a redundant propagator name string sits
        // at +76). Only the fields the game consumes are parsed.
        let propagator_id = read_u32(reader);
        let intensity = read_single(reader);
        let _unknown = read_u32(reader);
        let propagator = match propagator_id {
            0 => StimPropagator::Null,
            1 => StimPropagator::Contact,
            2 => StimPropagator::Radius {
                radius: read_single(reader) / SCALE_FACTOR,
            },
            other => StimPropagator::Unknown(other),
        };
        StimSourceOptions {
            intensity,
            propagator,
        }
    }
}

/// What a receptron does when its stim arrives (DromEd act/react "effect").
/// Only the effects the game consumes are modeled; the rest keep their name.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum ReceptronEffect {
    /// Deal `stim intensity * multiplier` damage (negative multipliers heal).
    /// `use_intensity: false` (rare - one record in shock2.gam) means a flat
    /// `multiplier` points instead.
    Damage {
        multiplier: f32,
        use_intensity: bool,
    },
    /// Scale the incoming stim's intensity by `factor` before other
    /// receptrons see it (shields, armor, Low Grav).
    Amplify { factor: f32 },
    /// Swallow the stim entirely (Invulnerable).
    Abort,
    /// Add radiation exposure scaled by `multiplier`. The player authors this
    /// response to the Radiation stimulus with a multiplier of 1.
    Radiate { multiplier: f32 },
    /// An effect the game does not implement yet (EnvSound, add_metaprop,
    /// Freeze, Stun, toxin, ...).
    Unhandled(String),
}

/// Act/react receptron (L$Receptron): the source template is the receiving
/// archetype (e.g. Human Vulnerability), the linked-to template is the stim
/// archetype it responds to (e.g. High Explosive), and the data says how.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ReceptronOptions {
    /// Evaluation order within the receiver's receptron list (lower first);
    /// matters when an Amplify and a Damage both match the same stim.
    pub order: i32,
    pub effect: ReceptronEffect,
}

impl ReceptronOptions {
    pub fn read(reader: &mut Box<dyn ReadAndSeek>, len: u32) -> ReceptronOptions {
        // sReceptron (88 bytes): order, min/max intensity (unused by SS2 data),
        // flags, a 32-byte effect-name buffer, target/agent object sentinels,
        // then an effect-specific parameter block.
        if len < 72 {
            // A truncated record (corrupt/fan-mission data) would panic the
            // fixed-offset reads below; degrade to an inert receptron instead.
            return ReceptronOptions {
                order: 0,
                effect: ReceptronEffect::Unhandled("<truncated record>".to_string()),
            };
        }
        let order = read_i32(reader);
        let _min_intensity = read_i32(reader);
        let _max_intensity = read_i32(reader);
        let _flags = read_i32(reader);
        let name_bytes = read_bytes(reader, 32);
        let name_end = name_bytes.iter().position(|b| *b == 0).unwrap_or(32);
        let name = String::from_utf8_lossy(&name_bytes[..name_end]).into_owned();
        let _target = read_i32(reader);
        let _agent = read_i32(reader);
        let param_56 = read_single(reader);
        let _param_60 = read_i32(reader);
        let param_64 = read_single(reader);
        let param_68 = read_i32(reader);

        let effect = match name.as_str() {
            "damage" => ReceptronEffect::Damage {
                multiplier: param_64,
                use_intensity: param_68 != 0,
            },
            "Amplify" => ReceptronEffect::Amplify { factor: param_56 },
            "Abort" => ReceptronEffect::Abort,
            "radiate" => ReceptronEffect::Radiate {
                multiplier: param_56,
            },
            _ => ReceptronEffect::Unhandled(name),
        };

        ReceptronOptions { order, effect }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct FlinderizeOptions {
    pub count: u32,
    pub impulse: f32,
    pub scatter: bool,
    pub offset: Vector3<f32>,
}

impl FlinderizeOptions {
    pub fn read(reader: &mut Box<dyn ReadAndSeek>, _len: u32) -> FlinderizeOptions {
        let count = read_u32(reader);
        let impulse = read_single(reader);
        let scatter = read_bool(reader);
        let offset = read_vec3(reader) / SCALE_FACTOR;
        FlinderizeOptions {
            count,
            impulse,
            scatter,
            offset,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AIWatchOptions {
    pub radius: f32,
    pub height: f32,
    pub scripted_actions: Vec<AIScriptedAction>,
}

impl AIWatchOptions {
    pub fn read(reader: &mut Box<dyn ReadAndSeek>, _len: u32) -> AIWatchOptions {
        let _unknown = read_bytes(reader, 60);

        let _trigger = read_u32(reader);
        let _awareness = read_u32(reader);
        let _ai_watch_visibility = read_u32(reader);
        let _unknown2 = read_i32(reader);
        let _ai_watch_kill_condition = read_u32(reader);
        let _kill_like_links = read_bool(reader);
        let _once_only = read_bool(reader);
        let _reuse_time = read_i32(reader);
        let _reset_time = read_i32(reader);
        let _min_alertness = read_u32(reader);
        let _max_alertness = read_u32(reader);
        let _ai_priority = read_u32(reader);
        let radius = read_i32(reader) as f32 / SCALE_FACTOR;
        let height = read_i32(reader) as f32 / SCALE_FACTOR;

        let mut scripted_actions = Vec::new();
        for _ in 0..8 {
            let action = AIScriptedAction::read(reader);
            scripted_actions.push(action);
        }

        AIWatchOptions {
            radius,
            height,
            scripted_actions,
        }
    }
}

#[derive(Clone, Debug)]
pub struct ToTemplateLink {
    pub to_template_id: i32,
    pub link: Link,
}

#[derive(Clone, Copy, Debug)]
pub struct WrappedEntityId(pub EntityId);

impl serde::Serialize for WrappedEntityId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let entity_id = self.0;
        let entity_id_inner = entity_id.inner();
        serializer.serialize_u64(entity_id_inner)
    }
}

impl<'a> serde::Deserialize<'a> for WrappedEntityId {
    fn deserialize<D>(deserializer: D) -> Result<WrappedEntityId, D::Error>
    where
        D: serde::Deserializer<'a>,
    {
        let entity_id_inner = u64::deserialize(deserializer)?;
        let entity_id = EntityId::from_inner(entity_id_inner)
            .ok_or_else(|| D::Error::custom("Failed to deserialize entity_id"))?; // TODO: Better error
        Ok(WrappedEntityId(entity_id))
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToLink {
    pub to_template_id: i32, // For debugging purposes - keep track of template id...
    pub to_entity_id: Option<WrappedEntityId>,
    pub link: Link,
}

#[derive(Clone, Debug)]
pub struct TemplateLinks {
    pub to_links: Vec<ToTemplateLink>,
}

impl TemplateLinks {
    pub fn empty() -> TemplateLinks {
        TemplateLinks { to_links: vec![] }
    }
    pub fn merge(a: &TemplateLinks, b: &TemplateLinks) -> TemplateLinks {
        let mut to_links = a.to_links.clone();
        to_links.extend(b.to_links.clone());
        TemplateLinks { to_links }
    }
}

#[derive(Clone, Component, Debug, Serialize, Deserialize)]
pub struct Links {
    pub to_links: Vec<ToLink>,
}

impl Links {
    pub fn empty() -> Links {
        Links { to_links: vec![] }
    }

    pub fn deserialize(
        json: serde_json::Value,
        entity_id_mapper: &HashMap<EntityId, EntityId>,
    ) -> Links {
        // Link data carries f32s, so a snapshot can hold values JSON cannot
        // round-trip (serde_json stores non-finite floats as null, #431).
        // Degrade to no links rather than panic the game loop.
        let prev_links: Links = match serde_json::from_value(json.clone()) {
            Ok(links) => links,
            Err(err) => {
                tracing::warn!("skipping links: cannot deserialize {}: {}", json, err);
                return Links::empty();
            }
        };

        let new_to_links = prev_links.to_links.iter().map(|link| {
            let new_to_link = link.clone();

            let to_entity_id = &new_to_link
                .to_entity_id
                .map(|id| entity_id_mapper.get(&id.0).map(|id| WrappedEntityId(*id)))
                .flatten();

            ToLink {
                to_entity_id: *to_entity_id,
                ..new_to_link
            }
        });

        Links {
            to_links: new_to_links.collect(),
        }
    }

    pub fn from_template_links(
        template_links: &TemplateLinks,
        template_to_id: &HashMap<i32, WrappedEntityId>,
    ) -> Links {
        let to_links = template_links
            .to_links
            .iter()
            .map(|t| ToLink {
                to_entity_id: template_to_id.get(&t.to_template_id).copied(),
                to_template_id: t.to_template_id,
                link: t.link.clone(),
            })
            .collect::<Vec<ToLink>>();

        Links { to_links }
    }

    pub fn merge(first: &Links, other: &Links) -> Links {
        let mut to_links = vec![];
        to_links.extend(first.to_links.clone());
        to_links.extend(other.to_links.clone());
        Links { to_links }
    }
}

#[derive(Debug, Component, Clone, Serialize, Deserialize)]
pub struct PropPickBias(pub f32);

/// Renderer\Transparency (alpha): 0.0 = invisible, 1.0 = opaque. Authored on
/// holo/ghost entities (e.g. the CS9 cutscene exhibits) and animated by the
/// Transluce script family.
#[derive(Debug, Component, Clone, Serialize, Deserialize)]
pub struct PropRenderAlpha(pub f32);

#[derive(Debug, Component, Clone, Serialize, Deserialize)]
pub struct PropTranslatingDoor {
    pub door_type: i32,
    pub closed: f32,
    pub open: f32,
    pub speed: f32,
    pub axis: i32,
    /// Authored door state (Dark's DOOR_STATE): 0=closed, 1=open, 2=closing,
    /// 3=opening, 4=halted. Doors must initialize to this pose - snapping
    /// everything to closed shuts doors that were authored open.
    pub state: i32,
    pub base_closed_location: Vector3<f32>,
    pub base_open_location: Vector3<f32>,
    pub base_location: Vector3<f32>,
}

impl PropTranslatingDoor {
    /// The world position this door should occupy at load time, per its
    /// authored state. In-motion/halted states resume from the authored
    /// location.
    pub fn initial_location(&self) -> Vector3<f32> {
        match self.state {
            0 => self.base_closed_location,
            1 => self.base_open_location,
            // closing / opening / halted / unknown: as authored
            _ => self.base_location,
        }
    }

    /// Whether the two endpoints differ, i.e. the door has somewhere to go.
    /// A door authored with `closed == open` has none: it can never move, so
    /// its open/closed state can't be read back from its position (speed is
    /// irrelevant - there is nowhere to travel to).
    pub fn has_travel(&self) -> bool {
        let travel = self.base_open_location - self.base_closed_location;
        travel.magnitude2() > 1e-6
    }

    /// A doorway the level authors left permanently open: no travel, and an
    /// authored state of open. The retail data has 10 of these (hydro2's
    /// survey-lab doors and the hydro airlock pairs); nothing can ever move
    /// them, so treating them as closed seals the rooms behind them (#602).
    pub fn is_permanently_open(&self) -> bool {
        !self.has_travel() && self.state == 1
    }

    /// A doorway that can never open: no travel, and not authored open.
    /// These doors remain physical barriers regardless of their lock state
    /// (e.g. medsci1's space-shield membranes).
    pub fn is_permanently_closed(&self) -> bool {
        !self.has_travel() && self.state != 1
    }
}

#[derive(Debug, Component, Clone, Serialize, Deserialize)]
pub struct PropObjectSound {
    pub name: String,
}

#[derive(Debug, Component, Clone, Serialize, Deserialize)]
pub struct PropScripts {
    pub scripts: Vec<String>,
    pub inherits: bool,
}

/// Read a P$Scale vector, replacing non-finite components with 1.0. Retail
/// data contains infinite scales (earth.mis obj 189, rec2.mis obj 474); JSON
/// stores non-finite floats as null, which broke the save round-trip when
/// re-entering the level (#431).
fn read_scale_vec3<T: io::Read>(reader: &mut T) -> Vector3<f32> {
    let mut scale = read_vec3(reader);
    if !scale.x.is_finite() || !scale.y.is_finite() || !scale.z.is_finite() {
        tracing::warn!("P$Scale has a non-finite component, using 1.0: {:?}", scale);
        for component in [&mut scale.x, &mut scale.y, &mut scale.z] {
            if !component.is_finite() {
                *component = 1.0;
            }
        }
    }
    scale
}

pub fn get<R: io::Read + io::Seek + 'static>() -> (
    Vec<Box<dyn PropertyDefinition<R>>>,
    Vec<Box<dyn LinkDefinition>>,
    Vec<Box<dyn LinkDefinitionWithData>>,
) {
    // Links
    let links = vec![
        define_link("L$AIRangedW", |_| Link::AIRangedWeapon),
        // TODO: Why is the data not available for some of these links?
        define_link("L$Corpse", |_| {
            Link::Corpse(CorpseOptions {
                propagate_scale: false,
            })
        }),
        define_link("L$LandingPo", |_| Link::LandingPoint),
        define_link("L$Replicato", |_| Link::Replicator),
        define_link("L$SpawnPoin", |_| Link::SpawnPoint),
        define_link("L$Spawned", |_| Link::Spawned),
        define_link("L$HackingLi", |_| Link::Hacking),
        define_link("L$SwitchLin", |_| Link::SwitchLink),
        define_link("L$Teleport", |_| Link::Teleport),
        define_link("L$TPathInit", |_| Link::TPathInit),
        define_link("L$TPathNext", |_| Link::TPathNext),
        define_link("L$ScriptPar", |_| Link::ScriptParams),
        define_link("L$AIPatrol", |_| Link::AIPatrol),
        define_link("L$Clip", |_| Link::Clip),
        define_link("L$Miss Span", |_| Link::MissSpang),
        define_link("L$Weapon", |_| Link::Weapon),
        //define_link("L$TPath", |_| Link::TPath),
    ];

    // Links with data
    let links_with_data = vec![
        // define_link_with_data("L$Corpse", "LD$Corpse", CorpseOptions::read, Link::Corpse),
        define_link_with_data(
            "L$AIWatchOb",
            "LD$AIWatchO",
            AIWatchOptions::read,
            Link::AIWatchObj,
        ),
        define_link_with_data(
            "L$Contains",
            "LD$Contains",
            |reader, _len| read_u32(reader),
            Link::Contains,
        ),
        define_link_with_data(
            "L$Hit Spang",
            "LD$Hit Span",
            |reader, _len| read_i32(reader),
            Link::HitSpang,
        ),
        define_link_with_data(
            "L$ParticleA",
            "LD$Particle",
            ParticleAttachOptions::read,
            Link::ParticleAttachement,
        ),
        define_link_with_data(
            "L$PhysAttac",
            "LD$PhysAtta",
            PhysAttachOptions::read,
            Link::PhysAttach,
        ),
        define_link_with_data(
            "L$AIProject",
            "LD$AIProjec",
            AIProjectileOptions::read,
            Link::AIProjectile,
        ),
        define_link_with_data("L$TPath", "LD$TPath", TPathData::read, Link::TPath),
        define_link_with_data(
            "L$Flinderiz",
            "LD$Flinderi",
            FlinderizeOptions::read,
            Link::Flinderize,
        ),
        define_link_with_data(
            "L$GunFlash",
            "LD$GunFlash",
            GunFlashOptions::read,
            Link::GunFlash,
        ),
        define_link_with_data(
            "L$Projectil",
            "LD$Projecti",
            ProjectileOptions::read,
            Link::Projectile,
        ),
        define_link_with_versioned_data(
            "L$arSrcDesc",
            "LD$arSrcDes",
            StimSourceOptions::read,
            Link::StimSource,
        ),
        define_link_with_versioned_data(
            "L$Receptron",
            "LD$Receptro",
            ReceptronOptions::read,
            Link::Receptron,
        ),
    ];

    // Properties
    let props = vec![
        define_prop(
            "P$AI",
            read_prop_string,
            |str| PropAI(str),
            accumulator::latest,
        ),
        define_prop("P$AI_Team", PropAITeam::read, identity, accumulator::latest),
        define_prop(
            // Dark truncates property chunk names to 11 characters, so the
            // chunk in the game files is "P$AI_AlertC", not "P$AI_AlertCap".
            "P$AI_AlertC",
            PropAIAlertCap::read,
            identity,
            accumulator::latest,
        ),
        define_prop(
            "P$AI_Alertn",
            PropAIAlertness::read,
            identity,
            accumulator::latest,
        ),
        define_prop(
            // The editor property is AI_AwrDel2, but Dark's 11-character
            // chunk-name field truncates its on-disk key to AI_AwrDel.
            "P$AI_AwrDel",
            PropAIAwareDelay::read,
            identity,
            accumulator::latest,
        ),
        define_prop(
            "P$AI_Hearin",
            PropAIHearing::read,
            identity,
            accumulator::latest,
        ),
        define_prop(
            "P$AI_Camera",
            PropAICamera::read,
            identity,
            accumulator::latest,
        ),
        define_prop(
            "P$AI_Device",
            PropAIDevice::read,
            identity,
            accumulator::latest,
        ),
        define_prop("P$AI_Mode", PropAIMode::read, identity, accumulator::latest),
        define_prop(
            "P$AI_Patrol",
            |reader, _len| read_bool(reader),
            PropAIPatrol,
            accumulator::latest,
        ),
        // Dark chunk keys retain at most 11 characters, so the property
        // `AI_PtrlRnd` is stored as `P$AI_PtrlRn` in retail missions.
        define_prop(
            "P$AI_PtrlRn",
            |reader, _len| read_bool(reader),
            PropAIPatrolRandom,
            accumulator::latest,
        ),
        define_prop(
            "P$AI_SigRsp",
            PropAISignalResponse::read,
            identity,
            accumulator::latest,
        ),
        define_prop(
            "P$AmbientHa",
            PropAmbientHacked::read,
            identity,
            accumulator::latest,
        ),
        define_prop(
            "P$AnimLight",
            PropAnimLight::read,
            identity,
            accumulator::latest,
        ),
        define_prop(
            "P$AnimTex",
            PropAnimTex::read,
            identity,
            accumulator::latest,
        ),
        define_prop(
            "P$BitmapAni",
            PropBitmapAnimation::read,
            identity,
            accumulator::latest,
        ),
        define_prop(
            "P$Class Tag",
            read_variable_length_string,
            |str| PropClassTag::from_string(&str),
            accumulator::latest,
        ),
        define_prop(
            "P$Collision",
            PropCollisionType::read,
            identity,
            accumulator::latest,
        ),
        define_prop(
            "P$ConsumeTy",
            read_variable_length_string,
            PropConsumeType,
            accumulator::latest,
        ),
        define_prop(
            "P$ChemNeede",
            PropChemicalNeeded::read,
            identity,
            accumulator::latest,
        ),
        define_prop(
            "P$Creature",
            |reader, _len| read_u32(reader),
            PropCreature,
            accumulator::latest,
        ),
        define_prop(
            "P$CretPose",
            PropCreaturePose::read,
            identity,
            accumulator::latest,
        ),
        define_prop(
            "P$DelayTime",
            |reader, _len| read_duration(reader),
            |delay| PropDelayTime { delay },
            accumulator::latest,
        ),
        define_prop(
            "P$Ecology",
            PropEcology::read,
            identity,
            accumulator::latest,
        ),
        define_prop(
            "P$EcoType",
            |reader, _len| read_i32(reader),
            PropEcoType,
            accumulator::latest,
        ),
        define_prop(
            "P$EcoState",
            |reader, _len| read_i32(reader),
            PropEcoState,
            accumulator::latest,
        ),
        define_prop(
            "P$DestLevel",
            read_prop_string,
            PropDestLevel,
            accumulator::latest,
        ),
        define_prop(
            "P$DestLoc",
            |reader, _len| read_i32(reader),
            PropDestLoc,
            accumulator::latest,
        ),
        define_prop(
            "P$ExP",
            |reader, _len| read_i32(reader),
            PropExp,
            accumulator::latest,
        ),
        define_prop(
            "P$StackCoun",
            |reader, _len| read_i32(reader),
            PropStackCount,
            accumulator::latest,
        ),
        define_prop(
            "P$SoftLevel",
            |reader, _len| read_i32(reader),
            PropSoftLevel,
            accumulator::latest,
        ),
        define_prop(
            "P$SoftType",
            |reader, _len| read_i32(reader),
            PropSoftType,
            accumulator::latest,
        ),
        define_prop("P$Spawn", PropSpawn::read, identity, accumulator::latest),
        define_prop(
            "P$FrameAniC",
            PropFrameAnimConfig::read,
            identity,
            accumulator::latest,
        ),
        define_prop(
            "P$FrameAniS",
            PropFrameAnimState::read,
            identity,
            accumulator::latest,
        ),
        define_prop(
            "P$FrobInfo",
            PropFrobInfo::read,
            identity,
            accumulator::latest,
        ),
        define_prop("P$KeyDst", KeyCard::read, PropKeyDst, accumulator::latest),
        define_prop("P$KeySrc", KeyCard::read, PropKeySrc, accumulator::latest),
        define_prop(
            "P$BaseGunDe",
            PropBaseGunDesc::read,
            identity,
            accumulator::latest,
        ),
        define_prop(
            "P$Sett1",
            read_variable_length_string,
            PropGunSettingText1,
            accumulator::latest,
        ),
        define_prop(
            "P$Sett2",
            read_variable_length_string,
            PropGunSettingText2,
            accumulator::latest,
        ),
        define_prop(
            "P$SHead1",
            read_variable_length_string,
            PropGunSettingHeader1,
            accumulator::latest,
        ),
        define_prop(
            "P$SHead2",
            read_variable_length_string,
            PropGunSettingHeader2,
            accumulator::latest,
        ),
        define_prop(
            "P$BaseTechD",
            PropBaseTechDesc::read,
            identity,
            accumulator::latest,
        ),
        define_prop(
            "P$GunState",
            PropGunState::read,
            identity,
            accumulator::latest,
        ),
        define_prop(
            "P$HackDiff",
            PropHackDiff::read,
            identity,
            accumulator::latest,
        ),
        define_prop(
            "P$HackText",
            read_variable_length_string,
            PropHackText,
            accumulator::latest,
        ),
        define_prop(
            "P$HitPoints",
            PropHitPoints::read,
            identity,
            accumulator::latest,
        ),
        define_prop(
            "P$HUDSelect",
            |reader, _len| read_bool(reader),
            PropHUDSelect,
            accumulator::latest,
        ),
        define_prop(
            "P$ShowHP",
            |reader, _len| read_bool(reader),
            PropShowHP,
            accumulator::latest,
        ),
        define_prop(
            // Truncated on disk from `P$ContainDims`: chunk names live in a
            // 12-byte field (same trap as `P$AI_AlertC`).
            "P$ContainDi",
            PropContainDimensions::read,
            identity,
            accumulator::latest,
        ),
        define_prop(
            "P$InvDims",
            PropInventoryDimensions::read,
            identity,
            accumulator::latest,
        ),
        define_prop(
            "P$InvLimbMo",
            read_prop_string,
            PropLimbModel,
            accumulator::latest,
        ),
        define_prop(
            "P$KeypadCod",
            |reader, _len| read_u32(reader),
            PropKeypadCode,
            accumulator::latest,
        ),
        define_prop("P$Locked", PropLocked::read, identity, accumulator::latest),
        define_prop(
            "P$Logs1",
            PropLog::read_deck1,
            identity,
            accumulator::latest,
        ),
        define_prop(
            "P$Logs2",
            PropLog::read_deck2,
            identity,
            accumulator::latest,
        ),
        define_prop(
            "P$Logs3",
            PropLog::read_deck3,
            identity,
            accumulator::latest,
        ),
        define_prop(
            "P$Logs4",
            PropLog::read_deck4,
            identity,
            accumulator::latest,
        ),
        define_prop(
            "P$Logs5",
            PropLog::read_deck5,
            identity,
            accumulator::latest,
        ),
        define_prop(
            "P$Logs6",
            PropLog::read_deck6,
            identity,
            accumulator::latest,
        ),
        define_prop(
            "P$Logs7",
            PropLog::read_deck7,
            identity,
            accumulator::latest,
        ),
        define_prop(
            "P$Logs8",
            PropLog::read_deck8,
            identity,
            accumulator::latest,
        ),
        define_prop(
            "P$Logs9",
            PropLog::read_deck9,
            identity,
            accumulator::latest,
        ),
        define_prop(
            "P$MapLoc",
            |reader, _len| read_i32(reader),
            PropMapLoc,
            accumulator::latest,
        ),
        define_prop(
            "P$Automap",
            PropAutomap::read,
            identity,
            accumulator::latest,
        ),
        define_prop("P$MapRef", PropMapRef::read, identity, accumulator::latest),
        define_prop(
            "P$MapText",
            read_variable_length_string,
            PropMapText,
            accumulator::latest,
        ),
        define_prop(
            "P$MapObjIco",
            read_variable_length_string,
            PropMapObjIcon,
            accumulator::latest,
        ),
        define_prop(
            "P$MapObjRot",
            PropMapObjRotate::read,
            identity,
            accumulator::latest,
        ),
        define_prop(
            "P$Material ", // NOTE: The trailing space is not a typo - the chunk name includes a space for this property (Material Tags)
            read_variable_length_string,
            PropMaterial,
            accumulator::latest,
        ),
        define_prop(
            "P$MAX_HP",
            PropMaxHitPoints::read,
            identity,
            accumulator::latest,
        ),
        define_prop(
            "P$ModelName",
            read_prop_string,
            PropModelName,
            accumulator::latest,
        ),
        define_prop(
            "P$MotActorT",
            PropMotionActorTags::read,
            identity,
            accumulator::latest,
        ),
        define_prop(
            "P$ObjIcon",
            read_prop_string,
            PropObjIcon,
            accumulator::latest,
        ),
        define_prop(
            "P$ObjName",
            read_variable_length_string,
            PropObjName,
            accumulator::latest,
        ),
        define_prop(
            "P$ObjLookS",
            PropObjLookString::read,
            identity,
            accumulator::latest,
        ),
        define_prop(
            "P$NameType",
            PropObjectNameType::read,
            identity,
            accumulator::latest,
        ),
        define_prop(
            "P$ObjState",
            PropObjState::read,
            identity,
            accumulator::latest,
        ),
        define_prop(
            "P$ObjShort",
            read_variable_length_string,
            PropObjShortName,
            accumulator::latest,
        ),
        define_prop(
            "P$UseMsg",
            read_variable_length_string,
            PropUseMsg,
            accumulator::latest,
        ),
        define_prop(
            "P$PsiPower",
            PropPsiPower::read,
            identity,
            accumulator::latest,
        ),
        define_prop(
            "P$PsiPowerD",
            PropPsiPowerLearned::read,
            identity,
            accumulator::latest,
        ),
        define_prop(
            "P$PsiPower2",
            PropPsiPowerLearned2::read,
            identity,
            accumulator::latest,
        ),
        define_prop(
            "P$PsiShield",
            PropPsiShield::read,
            identity,
            accumulator::latest,
        ),
        define_prop(
            "P$PsiState",
            PropPsiState::read,
            identity,
            accumulator::latest,
        ),
        define_prop(
            "P$VoiceIdx",
            PropVoiceIndex::read,
            identity,
            accumulator::latest,
        ),
        define_prop(
            "P$SpchVoice",
            read_prop_string,
            PropSpeechVoice,
            accumulator::latest,
        ),
        define_prop(
            "P$ObjSoundN",
            read_prop_string,
            |name| PropObjectSound { name },
            accumulator::latest,
        ),
        define_prop(
            "P$ParticleG",
            PropParticleGroup::read,
            identity,
            accumulator::latest,
        ),
        define_prop(
            "P$PickBias",
            |reader, _len| read_single(reader),
            PropPickBias,
            accumulator::latest,
        ),
        define_prop(
            "P$RenderAlp",
            |reader, _len| read_single(reader),
            PropRenderAlpha,
            accumulator::latest,
        ),
        define_prop(
            "P$RadAbsorb",
            |reader, _len| read_single(reader),
            PropRadiationAbsorb,
            accumulator::latest,
        ),
        define_prop(
            "P$RadDrain",
            |reader, _len| read_single(reader),
            PropRadiationDrain,
            accumulator::latest,
        ),
        define_prop(
            "P$RadRecove",
            |reader, _len| read_single(reader),
            PropRadiationRecovery,
            accumulator::latest,
        ),
        define_prop(
            "P$Position",
            read_prop_position,
            identity,
            accumulator::latest,
        ),
        define_prop(
            "P$PhysAttr",
            PropPhysAttr::read,
            identity,
            accumulator::latest,
        ),
        define_prop(
            "P$PhysDims",
            read_prop_phys_dimensions,
            identity,
            accumulator::latest,
        ),
        define_prop(
            "P$PhysInitV",
            PropPhysInitialVelocity::read,
            identity,
            accumulator::latest,
        ),
        define_prop(
            "P$PhysState",
            read_prop_phys_state,
            identity,
            accumulator::latest,
        ),
        define_prop(
            "P$PhysType",
            PropPhysType::read,
            identity,
            accumulator::latest,
        ),
        define_prop(
            "P$MovingTer",
            PropMovingTerrain::read,
            identity,
            accumulator::latest,
        ),
        define_prop(
            "P$PlayerGun",
            PropPlayerGun::read,
            identity,
            accumulator::latest,
        ),
        define_prop(
            "P$PGLaunchI",
            PropParticleLaunchInfo::read,
            identity,
            accumulator::latest,
        ),
        define_prop(
            "P$RenderTyp",
            |reader, _len| {
                let val = read_u32(reader);
                num_traits::FromPrimitive::from_u32(val).unwrap()
            },
            PropRenderType,
            accumulator::latest,
        ),
        define_prop(
            "P$RoomGrav",
            PropRoomGravity::read,
            identity,
            accumulator::latest,
        ),
        define_prop(
            "P$SchPlayPa",
            PropSchemaPlayParams::read,
            identity,
            accumulator::latest,
        ),
        define_prop(
            "P$Scale",
            |reader, _len| read_scale_vec3(reader),
            PropScale,
            accumulator::latest,
        ),
        define_prop(
            "P$Scripts",
            read_prop_scripts,
            identity,
            merge_ancestor_scripts,
        ),
        define_prop(
            "P$SelfIllum",
            |reader, _len| read_single(reader),
            PropSelfIllumination,
            accumulator::latest,
        ),
        define_prop(
            "P$Service",
            |reader, _len| read_u32(reader),
            PropService,
            accumulator::latest,
        ),
        define_prop(
            "P$SymName",
            read_variable_length_string,
            PropSymName,
            accumulator::latest,
        ),
        define_prop(
            "P$HasRefs",
            |reader, _len| read_bool(reader),
            PropHasRefs,
            accumulator::latest,
        ),
        define_prop(
            "P$Immobile",
            |reader, _len| read_bool(reader),
            PropImmobile,
            accumulator::latest,
        ),
        define_prop(
            "P$QBName",
            read_variable_length_string,
            PropQuestBitName,
            accumulator::latest,
        ),
        define_prop(
            "P$QBVal",
            PropQuestBitValue::read,
            identity,
            accumulator::latest,
        ),
        define_prop(
            "P$RepConten",
            PropReplicatorContents::read,
            identity,
            accumulator::latest,
        ),
        define_prop(
            "P$RepHacked",
            PropReplicatorHackedContents::read,
            identity,
            accumulator::latest,
        ),
        define_prop(
            "P$ReqTechDe",
            PropRequiredTechDesc::read,
            identity,
            accumulator::latest,
        ),
        define_prop(
            "P$RsrchRep",
            PropResearchReport::read,
            identity,
            accumulator::latest,
        ),
        define_prop(
            "P$RsrchTime",
            PropResearchTime::read,
            identity,
            accumulator::latest,
        ),
        define_prop(
            "P$RsrchTxt",
            PropResearchText::read,
            identity,
            accumulator::latest,
        ),
        define_prop(
            "P$StartLoc",
            |reader, _len| read_i32(reader),
            PropStartLoc,
            accumulator::latest,
        ),
        define_prop(
            "P$CharGenRo",
            |reader, _len| read_i32(reader),
            PropCharGenRo,
            accumulator::latest,
        ),
        define_prop(
            "P$TransDoor",
            read_prop_translating_door,
            identity,
            accumulator::latest,
        ),
        define_prop(
            "P$TripFlags",
            PropTripFlags::read,
            identity,
            accumulator::latest,
        ),
        // Tweq props
        define_prop(
            "P$CfgTweqDe",
            PropTweqDeleteConfig::read,
            identity,
            accumulator::latest,
        ),
        define_prop(
            "P$CfgTweqEm",
            PropTweqEmitterConfig::read,
            identity,
            accumulator::latest,
        ),
        define_prop(
            "P$CfgTweqMo",
            PropTweqModelConfig::read,
            identity,
            accumulator::latest,
        ),
        define_prop(
            "P$SignalTyp",
            read_variable_length_string,
            PropSignalType,
            accumulator::latest,
        ),
        define_prop(
            "P$StTweqDel",
            PropTweqDeleteState::read,
            identity,
            accumulator::latest,
        ),
        define_prop(
            "P$StTweqEmi",
            PropTweqEmitterState::read,
            identity,
            accumulator::latest,
        ),
        define_prop(
            "P$StTweqRot",
            PropTweqRotateState::read,
            identity,
            accumulator::latest,
        ),
        define_prop(
            "P$StTweqMod",
            PropTweqModelState::read,
            identity,
            accumulator::latest,
        ),
        // Internal properties
        // These are not properties that are provided by shock2 game,
        // but are used internally for save/restore.
        define_prop(
            "__P$InternalTemplateId",
            |reader, _len| read_i32(reader),
            |id| PropTemplateId { template_id: id },
            accumulator::latest,
        ),
        define_prop(
            "__P$OriginalModelName",
            read_prop_string,
            InternalPropOriginalModelName,
            accumulator::latest,
        ),
    ];
    (props, links, links_with_data)
}

fn merge_ancestor_scripts(ancestor_scripts: PropScripts, new_scripts: PropScripts) -> PropScripts {
    let ret;
    if !new_scripts.inherits {
        ret = new_scripts
    } else {
        let mut cloned_new_scripts = new_scripts.scripts;
        let mut cloned_ancestor_scripts = ancestor_scripts.scripts;
        cloned_new_scripts.append(&mut cloned_ancestor_scripts);
        ret = PropScripts {
            scripts: cloned_new_scripts,
            inherits: true,
        }
    }
    ret
}

fn read_prop_scripts<T: io::Read + io::Seek>(reader: &mut T, _len: u32) -> PropScripts {
    let script1 = read_string_with_size(reader, 32);
    let script2 = read_string_with_size(reader, 32);
    let script3 = read_string_with_size(reader, 32);
    let script4 = read_string_with_size(reader, 32);

    // The prop is actually `dont_inherits` - so if it is false, that means to inherit.
    // Just removing the double negative
    let inherits = read_u32(reader) == 0;

    let ret = PropScripts {
        scripts: vec![script1, script2, script3, script4]
            .iter()
            .filter(|e| !e.is_empty())
            .map(|str| str.to_owned())
            .collect::<Vec<String>>(),
        inherits,
    };

    ret
}

fn read_prop_translating_door<T: io::Read + io::Seek>(
    reader: &mut T,
    _len: u32,
) -> PropTranslatingDoor {
    let door_type = read_i32(reader);
    let closed = read_single(reader);
    let open = read_single(reader);
    let speed = read_single(reader) / SCALE_FACTOR;
    let axis = read_i32(reader);
    let state = read_i32(reader);
    let _hard_limits = read_bool(reader);
    let _sound_blocking = read_single(reader);
    let _vision_blocking = read_single(reader);
    let _push_mass = read_single(reader);
    let base_closed_location = read_vec3(reader) / SCALE_FACTOR;
    let base_open_location = read_vec3(reader) / SCALE_FACTOR;
    let base_location = read_vec3(reader) / SCALE_FACTOR;
    let _base_angle = read_u16_vec3(reader);
    let _base = read_single(reader);
    let _room1 = read_i32(reader);
    let _room2 = read_i32(reader);

    let delta = _len - 94;
    if delta > 0 {
        let _unk = read_bytes(reader, delta as usize);
    }

    PropTranslatingDoor {
        door_type,
        closed,
        open,
        state,
        base_closed_location,
        base_open_location,
        base_location,
        axis,
        speed,
    }
}

fn read_prop_phys_dimensions<T: io::Read + io::Seek>(
    reader: &mut T,
    _len: u32,
) -> PropPhysDimensions {
    let radius0 = read_single(reader) / SCALE_FACTOR / 2.0;
    let radius1 = read_single(reader) / SCALE_FACTOR / 2.0;
    let offset0 = read_vec3(reader) / SCALE_FACTOR;
    let offset1 = read_vec3(reader) / SCALE_FACTOR;
    let size = read_vec3(reader) / SCALE_FACTOR;
    let unk1 = read_u32(reader);
    let unk2 = read_u32(reader);

    PropPhysDimensions {
        radius0,
        radius1,
        offset0,
        offset1,
        size,
        unk1,
        unk2,
    }
}

fn read_prop_phys_state<T: io::Read + io::Seek>(reader: &mut T, _len: u32) -> PropPhysState {
    let position = read_vec3(reader) / SCALE_FACTOR;
    let facing = read_vec3(reader);
    let velocity = read_vec3(reader);
    let rot_velocity = read_vec3(reader);

    let rotation = quat_from_facing_vector(vec3(Deg(facing.x), Deg(facing.y), Deg(facing.z)));

    PropPhysState {
        position,
        rotation,
        velocity,
        rot_velocity,
    }
}

fn read_prop_position<T: io::Read + io::Seek>(reader: &mut T, _len: u32) -> PropPosition {
    let position = read_vec3(reader) / SCALE_FACTOR;
    let cell = read_u16(reader);
    let _unknown = read_i16(reader);
    let facing = read_u16_vec3(reader);
    let rotation = quat_from_facing_vector(facing);

    PropPosition {
        position,
        cell,
        rotation,
    }
}

/// Returns a quaternion from a vector based on euler angles
fn quat_from_facing_vector(facing: Vector3<Deg<f32>>) -> Quaternion<f32> {
    Quaternion::from_angle_y(facing.y)
        * Quaternion::from_angle_z(facing.z)
        * Quaternion::from_angle_x(facing.x)
}

fn read_prop_string<T: io::Read + io::Seek>(reader: &mut T, prop_len: u32) -> String {
    read_string_with_size(reader, prop_len as usize)
}

fn read_variable_length_string<T: io::Read + io::Seek>(reader: &mut T, prop_len: u32) -> String {
    let _ignored = read_u32(reader);
    read_string_with_size(reader, prop_len as usize - 4usize)
}

// Implement shipyard component for all properties
impl<C> Property for C
where
    C: Component
        + TupleAddComponent
        + fmt::Debug
        + std::marker::Sync
        + Clone
        + std::marker::Send
        + Serialize,
{
    fn initialize(&self, world: &mut World, entity: EntityId) {
        world.add_component(entity, self.clone());
    }
}

#[derive(Debug)]
pub struct WrappedProperty<C> {
    inner_property: C,
    accumulator: fn(C /* ancestor */, C /* newest */) -> C,
}

impl<C> Property for WrappedProperty<C>
where
    C: Component
        + TupleAddComponent
        + fmt::Debug
        + std::marker::Sync
        + Clone
        + std::marker::Send
        + Serialize
        + serde::Deserialize<'static>,
{
    fn initialize(&self, world: &mut World, entity: EntityId) {
        let view: ViewMut<C> = world.borrow().unwrap();
        let maybe_previous_value = &view.get(entity);
        let mut value_to_set = self.inner_property.clone();

        // If there was a previous value, defer to the accumulator.
        // Should the previous and new value be reconciled in some way (ie, inheriting scripts?),
        // or should the value just be simply overwritten?
        if let Ok(previous_value) = maybe_previous_value {
            value_to_set =
                (self.accumulator)((*previous_value).clone(), self.inner_property.clone());
        }

        drop(view);
        world.add_component(entity, value_to_set);
    }
}

// `Send + Sync` is required so the level parse (which builds `Vec<Arc<Box<dyn Property>>>`)
// can run on a background thread (projects/loading-screen.md, PR S2). This is satisfied
// for free: both blanket impls below already require `C: Send + Sync`.
pub trait Property: fmt::Debug + Send + Sync {
    fn initialize(&self, world: &mut World, entity: EntityId);
}

// `Send + Sync` so the shared `GlobalContext` (which holds these definition objects)
// can be borrowed by the background level-parse thread (projects/loading-screen.md, PR S3).
pub trait PropertyDefinition<R: io::Read + io::Seek>: Send + Sync {
    fn name(&self) -> String;

    fn read(&self, reader: &mut R, prop_len: u32) -> Box<dyn Property>;

    fn serialize(&self, world: &World) -> HashMap<u64, serde_json::Value>;

    fn deserialize(
        &self,
        val: &HashMap<u64, serde_json::Value>,
        world: &mut World,
        entity_id_map: &HashMap<EntityId, EntityId>,
    );
}

pub trait LinkDefinition: Send + Sync {
    fn name(&self) -> String;

    fn convert(&self, link: ToTemplateLinkInfo) -> ToTemplateLink;
}

struct LinkDefinitionStruct {
    name: String,
    converter: Converter<ToTemplateLinkInfo, Link>,
}

/// How an LD$ chunk frames its per-link records.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LinkDataFraming {
    /// The chunk's leading u32 is the per-record data size (most chunks).
    HeaderDeclared,
    /// The leading u32 is a database version, not a size (the act/react
    /// chunks LD$arSrcDes/LD$Receptro declare "2"); the record size must be
    /// derived from the chunk length and the link count.
    VersionHeader,
}

pub trait LinkDefinitionWithData: Send + Sync {
    fn link_chunk_name(&self) -> String;
    fn link_data_chunk_name(&self) -> String;
    fn link_data_framing(&self) -> LinkDataFraming;

    fn convert(&self, data: Vec<u8>, prop_len: u32, link: ToTemplateLinkInfo) -> ToTemplateLink;
}

struct LinkDefinitionWithDataStruct<TData> {
    link_name: String,
    link_data_name: String,
    framing: LinkDataFraming,
    converter: Converter<TData, Link>,
    reader: Reader<Box<dyn ReadAndSeek>, TData>,
}

pub trait ReadAndSeek: io::Seek + io::Read {}
impl ReadAndSeek for Cursor<Vec<u8>> {}

impl<TData> LinkDefinitionWithData for LinkDefinitionWithDataStruct<TData> {
    fn link_chunk_name(&self) -> String {
        self.link_name.to_owned()
    }

    fn link_data_chunk_name(&self) -> String {
        self.link_data_name.to_owned()
    }

    fn link_data_framing(&self) -> LinkDataFraming {
        self.framing
    }

    fn convert(
        &self,
        data: Vec<u8>,
        prop_len: u32,
        link_info: ToTemplateLinkInfo,
    ) -> ToTemplateLink {
        let mut cursor: Box<dyn ReadAndSeek> = Box::new(Cursor::new(data));
        let data = (self.reader)(&mut cursor, prop_len);
        let link = (self.converter)(data);
        ToTemplateLink {
            to_template_id: link_info.dest_template_id,
            link,
        }
    }
}

impl LinkDefinition for LinkDefinitionStruct {
    fn name(&self) -> String {
        self.name.to_owned()
    }

    fn convert(&self, link_info: ToTemplateLinkInfo) -> ToTemplateLink {
        ToTemplateLink {
            to_template_id: link_info.dest_template_id,
            link: (self.converter)(link_info),
        }
    }
}

type Reader<R, ROutput> = fn(&mut R, u32) -> ROutput;

type Converter<RIntermediate, ROutput> = fn(RIntermediate) -> ROutput;

type Accumulator<T> = fn(T, T) -> T;

struct PropertyDefinitionStruct<R: io::Read + io::Seek, RIntermediate, ROutput: Component> {
    name: String,
    reader: Reader<R, RIntermediate>,
    converter: Converter<RIntermediate, ROutput>,
    accumulator: Accumulator<ROutput>,
}

impl<R, RIntermediate, ROutput> PropertyDefinition<R>
    for PropertyDefinitionStruct<R, RIntermediate, ROutput>
where
    R: io::Read + io::Seek,
    ROutput: Component
        + TupleAddComponent
        + fmt::Debug
        + std::marker::Sync
        + Clone
        + std::marker::Send
        + Serialize
        + DeserializeOwned,
{
    fn name(&self) -> String {
        self.name.to_owned()
    }

    fn read(&self, reader: &mut R, prop_len: u32) -> Box<dyn Property> {
        let intermediate = (self.reader)(reader, prop_len);
        let output = (self.converter)(intermediate);
        Box::new(WrappedProperty {
            inner_property: output,
            accumulator: self.accumulator,
        })
    }

    fn serialize(&self, world: &World) -> HashMap<u64, serde_json::Value> {
        let view: View<ROutput> = world.borrow::<View<ROutput>>().unwrap();
        let mut result = HashMap::new();
        for (entity, prop) in view.iter().with_id() {
            let serialized = serde_json::to_value(prop).unwrap();
            result.insert(entity.inner(), serialized);
        }
        result
    }

    fn deserialize(
        &self,
        map: &HashMap<u64, serde_json::Value>,
        world: &mut World,
        entity_id_map: &HashMap<EntityId, EntityId>,
    ) {
        for (old_ent_id, json) in map {
            if let Some(new_ent_id) = entity_id_map.get(&EntityId::from_inner(*old_ent_id).unwrap())
            {
                // A snapshot can hold values JSON cannot round-trip (serde_json
                // stores non-finite floats as null, #431). Drop the component
                // rather than panic the game loop.
                match serde_json::from_value::<ROutput>(json.clone()) {
                    Ok(prop) => world.add_component(*new_ent_id, prop),
                    Err(err) => tracing::warn!(
                        "skipping {} for entity {}: cannot deserialize {}: {}",
                        self.name,
                        old_ent_id,
                        json,
                        err
                    ),
                }
            }
        }
    }
}

pub fn define_prop<
    R: io::Read + io::Seek + 'static,
    RIntermediate: 'static,
    ROutput: 'static + fmt::Debug + Send + Sync + Clone + Component + Serialize + DeserializeOwned,
>(
    name: &str,
    reader: Reader<R, RIntermediate>,
    converter: Converter<RIntermediate, ROutput>,
    accumulator: Accumulator<ROutput>,
) -> Box<dyn PropertyDefinition<R>> {
    Box::new(PropertyDefinitionStruct {
        name: name.to_string(),
        reader,
        converter,
        accumulator,
    })
}

pub fn define_link(
    name: &str,
    converter: Converter<ToTemplateLinkInfo, Link>,
) -> Box<dyn LinkDefinition> {
    Box::new(LinkDefinitionStruct {
        name: name.to_string(),
        converter,
    })
}

pub fn define_link_with_data<TData: 'static + fmt::Debug + Send + Sync + Clone>(
    link_name: &str,
    link_data_name: &str,
    reader: Reader<Box<dyn ReadAndSeek>, TData>,
    converter: Converter<TData, Link>,
) -> Box<dyn LinkDefinitionWithData> {
    Box::new(LinkDefinitionWithDataStruct {
        link_name: link_name.to_string(),
        link_data_name: link_data_name.to_string(),
        framing: LinkDataFraming::HeaderDeclared,
        reader,
        converter,
    })
}

/// Like `define_link_with_data`, but for the act/react chunks whose LD$ header
/// is a version number rather than the record size (see `LinkDataFraming`).
pub fn define_link_with_versioned_data<TData: 'static + fmt::Debug + Send + Sync + Clone>(
    link_name: &str,
    link_data_name: &str,
    reader: Reader<Box<dyn ReadAndSeek>, TData>,
    converter: Converter<TData, Link>,
) -> Box<dyn LinkDefinitionWithData> {
    Box::new(LinkDefinitionWithDataStruct {
        link_name: link_name.to_string(),
        link_data_name: link_data_name.to_string(),
        framing: LinkDataFraming::VersionHeader,
        reader,
        converter,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn object_name_type_reads_retail_variants_and_preserves_unknown_values() {
        for (raw, expected) in [
            (0_i32, ObjectNameType::Normal),
            (1, ObjectNameType::StackCount),
            (2, ObjectNameType::LogTitle),
            (3, ObjectNameType::Weapon),
            (17, ObjectNameType::Unknown(17)),
        ] {
            let parsed = PropObjectNameType::read(&mut Cursor::new(raw.to_le_bytes()), 4);
            assert_eq!(parsed, PropObjectNameType(expected));
            assert_eq!(parsed.0.raw(), raw);
        }
    }

    /// Build a 108-byte sStimSourceDesc payload the way shock2.gam lays it
    /// out: propagator id, intensity, unknown, then propagator params.
    fn stim_source_payload(propagator_id: u32, intensity: f32, radius: f32) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&propagator_id.to_le_bytes());
        bytes.extend_from_slice(&intensity.to_le_bytes());
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(&radius.to_le_bytes());
        bytes.resize(108, 0);
        bytes
    }

    fn read_stim_source(payload: Vec<u8>) -> StimSourceOptions {
        let mut cursor: Box<dyn ReadAndSeek> = Box::new(Cursor::new(payload));
        StimSourceOptions::read(&mut cursor, 108)
    }

    #[test]
    fn phys_attach_offset_uses_world_axes_and_scale() {
        // command1 Tram Front -> Tram stores the Dark-space vector
        // (-10, -0.1875, -0.5), which read_vec3 maps to world
        // (10, -0.5, -0.1875) before the global 2.5 scale.
        let mut bytes = Vec::new();
        for value in [-10.0f32, -0.1875, -0.5] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        let mut cursor: Box<dyn ReadAndSeek> = Box::new(Cursor::new(bytes));
        let options = PhysAttachOptions::read(&mut cursor, 12);

        assert_eq!(options.offset, vec3(4.0, -0.2, -0.075));
    }

    /// The eng2 "Message Trap" P$UseMsg chunk: a leading u32 the readers
    /// ignore, then the NUL-terminated key ("OutOfOrder", 15 bytes total).
    #[test]
    fn use_message_reads_its_key_from_a_variable_length_string_chunk() {
        let mut bytes = 11u32.to_le_bytes().to_vec();
        bytes.extend_from_slice(b"OutOfOrder\0");
        let mut cursor = Cursor::new(bytes);

        let property = PropUseMsg(read_variable_length_string(&mut cursor, 15));

        assert_eq!(property.0, "OutOfOrder");
    }

    #[test]
    fn moving_terrain_reads_active_and_previous_state() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        let mut cursor = Cursor::new(bytes);

        let property = PropMovingTerrain::read(&mut cursor, 8);

        assert!(property.active);
        assert!(!property.previous_active);
    }

    #[test]
    fn stim_source_radius_matches_incendiary_explosion_data() {
        // Incendiary Explosion -> Incendiary stim in shock2.gam: intensity 15,
        // radius 10 (dark units).
        let opts = read_stim_source(stim_source_payload(2, 15.0, 10.0));
        assert_eq!(opts.intensity, 15.0);
        assert_eq!(
            opts.propagator,
            StimPropagator::Radius {
                radius: 10.0 / SCALE_FACTOR
            }
        );
    }

    #[test]
    fn stim_source_contact_and_null_have_no_radius() {
        let contact = read_stim_source(stim_source_payload(1, 8.0, 0.0));
        assert_eq!(contact.intensity, 8.0);
        assert_eq!(contact.propagator, StimPropagator::Contact);

        let null = read_stim_source(stim_source_payload(0, 16.0, 0.0));
        assert_eq!(null.propagator, StimPropagator::Null);
    }

    /// Build an 88-byte sReceptron payload the way shock2.gam lays it out:
    /// order, min/max intensity, flags, 32-byte effect name, target/agent,
    /// then the effect parameter block ([56] f32, [60] i32, [64] f32, [68] i32).
    fn receptron_payload(order: i32, effect_name: &str, p56: f32, p64: f32, p68: i32) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&order.to_le_bytes());
        bytes.extend_from_slice(&[0u8; 8]); // min/max intensity (unused)
        bytes.extend_from_slice(&2i32.to_le_bytes()); // flags
        let mut name = effect_name.as_bytes().to_vec();
        name.resize(32, 0);
        bytes.extend_from_slice(&name);
        bytes.extend_from_slice(&(-2i32).to_le_bytes()); // target sentinel
        bytes.extend_from_slice(&(-257i32).to_le_bytes()); // agent sentinel
        bytes.extend_from_slice(&p56.to_le_bytes());
        bytes.extend_from_slice(&0i32.to_le_bytes());
        bytes.extend_from_slice(&p64.to_le_bytes());
        bytes.extend_from_slice(&p68.to_le_bytes());
        bytes.resize(88, 0);
        bytes
    }

    fn read_receptron(payload: Vec<u8>) -> ReceptronOptions {
        let mut cursor: Box<dyn ReadAndSeek> = Box::new(Cursor::new(payload));
        ReceptronOptions::read(&mut cursor, 88)
    }

    #[test]
    fn receptron_damage_matches_human_vs_high_explosive_data() {
        // Human Vulnerability -> High Explosive in shock2.gam: damage x4,
        // scaled by stim intensity.
        let opts = read_receptron(receptron_payload(33, "damage", 0.0, 4.0, 1));
        assert_eq!(opts.order, 33);
        assert_eq!(
            opts.effect,
            ReceptronEffect::Damage {
                multiplier: 4.0,
                use_intensity: true
            }
        );
    }

    #[test]
    fn receptron_amplify_abort_and_unknown_effects() {
        // PsiShield's Amplify x0.85 (factor lives at +56, not +64).
        let amplify = read_receptron(receptron_payload(95, "Amplify", 0.85, 0.0, 0));
        assert_eq!(amplify.effect, ReceptronEffect::Amplify { factor: 0.85 });

        let abort = read_receptron(receptron_payload(1, "Abort", 0.0, 0.0, 0));
        assert_eq!(abort.effect, ReceptronEffect::Abort);

        let other = read_receptron(receptron_payload(7, "EnvSound", 0.0, 0.0, 0));
        assert_eq!(
            other.effect,
            ReceptronEffect::Unhandled("EnvSound".to_string())
        );
    }

    #[test]
    fn receptron_radiate_keeps_the_authored_exposure_multiplier() {
        // The Player -> Radiation in shock2.gam: `radiate` x1. The multiplier
        // lives at +56, just like the original reaction implementation reads.
        let opts = read_receptron(receptron_payload(69, "radiate", 1.0, 0.0, 0));
        assert_eq!(opts.effect, ReceptronEffect::Radiate { multiplier: 1.0 });
    }

    fn scale_definition() -> Box<dyn PropertyDefinition<Box<dyn ReadAndSeek>>> {
        let (props, _, _) = get::<Box<dyn ReadAndSeek>>();
        props
            .into_iter()
            .find(|p| p.name() == "P$Scale")
            .expect("P$Scale definition should be registered")
    }

    #[test]
    fn scale_parse_replaces_non_finite_components() {
        // earth.mis obj 189 ("Grate 6x8") ships P$Scale = (1/6, 1/8, +inf) and
        // rec2.mis obj 474 has a similar infinity. A non-finite component must
        // not reach the world: JSON stores it as null, which broke the save
        // round-trip when re-entering the level (#431).
        let mut bytes = Vec::new();
        for v in [1.0f32 / 6.0, 0.125, f32::INFINITY] {
            bytes.extend_from_slice(&v.to_le_bytes());
        }
        let mut cursor: Box<dyn ReadAndSeek> = Box::new(Cursor::new(bytes));
        let prop = scale_definition().read(&mut cursor, 12);

        let mut world = World::new();
        let entity = world.add_entity(());
        prop.initialize(&mut world, entity);

        let v_scale = world.borrow::<View<PropScale>>().unwrap();
        let scale = v_scale.get(entity).unwrap().0;
        // read_vec3 maps file (x, z, y) -> world (-x, y, z), so the file's
        // infinite third float lands in world y.
        assert_eq!(scale.x, -1.0 / 6.0);
        assert_eq!(scale.y, 1.0, "non-finite component should fall back to 1.0");
        assert_eq!(scale.z, 0.125);
    }

    #[test]
    fn links_deserialize_degrades_to_empty_on_bad_json() {
        // Same failure class as #431 via the links path: snapshot JSON that
        // cannot deserialize (e.g. a nulled non-finite float in link data)
        // must degrade to no links, not panic the game loop.
        let id_map = HashMap::new();
        let links = Links::deserialize(serde_json::json!({ "to_links": null }), &id_map);
        assert!(links.to_links.is_empty());
    }

    /// Dark stores chunk names in a fixed 12-byte field (see
    /// `ss2_chunk_file_reader::read_table_of_contents`), so an editor property
    /// name longer than 11 characters is truncated on disk. Chunk lookup is an
    /// exact match, so a longer registered name silently never parses - which
    /// is exactly how `P$AI_AlertCap` went unparsed for every entity.
    #[test]
    fn registered_chunk_names_fit_the_11_char_chunk_field() {
        let (props, _, _) = get::<io::Cursor<Vec<u8>>>();
        let too_long: Vec<String> = props
            .iter()
            .map(|p| p.name())
            // `__`-prefixed names are internal runtime-only properties with no
            // chunk on disk, so the field width does not apply to them.
            .filter(|name| !name.starts_with("__"))
            .filter(|name| name.len() > 11)
            .collect();
        assert!(
            too_long.is_empty(),
            "these registered property names exceed the 11-character chunk-name \
             field and can never match a chunk: {too_long:?}"
        );
    }

    /// Regression guard for #887: the editor-facing property name is
    /// `AI_AwrDel2`, but Dark's 11-character on-disk chunk key is truncated to
    /// `P$AI_AwrDel`. Registering the editor name can never match retail data.
    #[test]
    fn ai_aware_delay_uses_the_retail_chunk_name() {
        let (props, _, _) = get::<io::Cursor<Vec<u8>>>();
        let names: Vec<String> = props.iter().map(|prop| prop.name()).collect();

        assert!(names.iter().any(|name| name == "P$AI_AwrDel"));
        assert!(!names.iter().any(|name| name == "P$AI_AwrDel2"));
    }

    #[test]
    fn deserialize_skips_values_json_cannot_round_trip() {
        // A live world can still hold non-finite floats (e.g. physics NaNs);
        // serde_json::to_value stores those as null. Rebuilding a level from
        // its snapshot must drop such a component with a warning instead of
        // panicking the game-loop thread (#431).
        let scale_def = scale_definition();

        let mut world = World::new();
        let entity = world.add_entity(PropScale(vec3(1.0, f32::INFINITY, 1.0)));
        let serialized = scale_def.serialize(&world);
        assert_eq!(
            serialized[&entity.inner()]["y"],
            serde_json::Value::Null,
            "serde_json should map a non-finite float to null"
        );

        let mut new_world = World::new();
        let new_entity = new_world.add_entity(());
        let id_map = HashMap::from([(entity, new_entity)]);
        scale_def.deserialize(&serialized, &mut new_world, &id_map);

        let v_scale = new_world.borrow::<View<PropScale>>().unwrap();
        assert!(
            v_scale.get(new_entity).is_err(),
            "component that cannot round-trip should be skipped, not panic"
        );
    }
}
