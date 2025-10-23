mod accumulator;
mod prop_ai;
mod prop_ambient_hacked;
mod prop_anim_tex;
mod prop_bitmap_animation;
mod prop_collision_type;
mod prop_creature_pose;
mod prop_frame_anim_config;
mod prop_frame_anim_state;
mod prop_frob_info;
mod prop_hit_points;
mod prop_key;
mod prop_log;
mod prop_particles;
mod prop_phys_attr;
mod prop_phys_initial_velocity;
mod prop_phys_type;
mod prop_player_gun;
mod prop_quest_bit;
mod prop_render_type;
mod prop_replicator;
mod prop_room_gravity;
mod prop_trip_flags;
mod prop_tweq;

use num_derive::{FromPrimitive, ToPrimitive};
pub use prop_ai::*;
pub use prop_ambient_hacked::*;
pub use prop_anim_tex::*;
pub use prop_bitmap_animation::*;
pub use prop_collision_type::*;
pub use prop_creature_pose::*;
pub use prop_frame_anim_config::*;
pub use prop_frame_anim_state::*;
pub use prop_frob_info::*;
pub use prop_hit_points::*;
pub use prop_key::*;
pub use prop_log::*;
pub use prop_particles::*;
pub use prop_phys_attr::*;
pub use prop_phys_initial_velocity::*;
pub use prop_phys_type::*;
pub use prop_player_gun::*;
pub use prop_quest_bit::*;
pub use prop_render_type::*;
pub use prop_replicator::*;
pub use prop_room_gravity::*;
pub use prop_trip_flags::*;
pub use prop_tweq::*;

use num_traits::FromPrimitive;
use serde::{
    de::{DeserializeOwned, Error},
    Deserialize, Serialize,
};

use std::{
    collections::HashMap,
    convert::identity,
    fmt,
    io::{self, Cursor},
    time::Duration,
};

use crate::{ss2_common::*, SCALE_FACTOR};
use cgmath::{vec3, Deg, Quaternion, Rotation3, Vector3};
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

#[derive(Debug, Component, Serialize, Deserialize)]
pub struct PropTeleported {
    pub countdown_timer: f32, // Remaining time to be considered 'recently teleported'
}

#[derive(Debug, Component, Clone, Serialize, Deserialize)]
pub struct PropKeypadCode(pub u32);

impl PropTeleported {
    pub fn new() -> PropTeleported {
        PropTeleported {
            countdown_timer: 1.0,
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

#[derive(Debug, Component, Clone, Serialize, Deserialize)]
pub struct PropLimbModel(pub String);

#[derive(Debug, Component, Clone, Serialize, Deserialize)]
pub struct PropMaterial(pub String);

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

#[derive(Debug, Component, Clone, Serialize, Deserialize)]
pub struct PropObjName(pub String);

#[derive(Debug, Component, Clone, Serialize, Deserialize)]
pub struct PropObjShortName(pub String);

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

// TODO: Is there a player prop
#[derive(Debug, Component, Clone, Serialize, Deserialize)]
pub struct PropLocalPlayer {}

#[derive(Debug, Component, Clone, Serialize, Deserialize)]
pub struct PropHUDSelect(pub bool);

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
    Corpse(CorpseOptions),
    Flinderize(FlinderizeOptions),
    GunFlash(GunFlashOptions),
    LandingPoint,
    Projectile(ProjectileOptions),
    Replicator,
    SwitchLink,
    MissSpang,
    TPathInit,
    TPath(TPathData),
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

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct FlinderizeOptions {
    count: u32,
    impulse: f32,
    scatter: bool,
    offset: Vector3<f32>,
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
        let prev_links: Links = serde_json::from_value(json).unwrap();

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

#[derive(Debug, Component, Clone, Serialize, Deserialize)]
pub struct PropTranslatingDoor {
    pub door_type: i32,
    pub closed: f32,
    pub open: f32,
    pub speed: f32,
    pub axis: i32,
    pub base_closed_location: Vector3<f32>,
    pub base_open_location: Vector3<f32>,
    pub base_location: Vector3<f32>,
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
        define_link("L$SwitchLin", |_| Link::SwitchLink),
        define_link("L$TPathInit", |_| Link::TPathInit),
        define_link("L$Miss Span", |_| Link::MissSpang),
        //define_link("L$TPath", |_| Link::TPath),
        //define_link("L$TPathNext", |_| Link::TPath),
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
    ];

    // Properties
    let props = vec![
        define_prop(
            "P$AI",
            read_prop_string,
            |str| PropAI(str),
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
            "P$ObjShort",
            read_variable_length_string,
            PropObjShortName,
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
            "P$Scale",
            |reader, _len| read_vec3(reader),
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
            "P$StartLoc",
            |reader, _len| read_i32(reader),
            PropStartLoc,
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
    let _state = read_i32(reader);
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

pub trait Property: fmt::Debug {
    fn initialize(&self, world: &mut World, entity: EntityId);
}

pub trait PropertyDefinition<R: io::Read + io::Seek> {
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

pub trait LinkDefinition {
    fn name(&self) -> String;

    fn convert(&self, link: ToTemplateLinkInfo) -> ToTemplateLink;
}

struct LinkDefinitionStruct {
    name: String,
    converter: Converter<ToTemplateLinkInfo, Link>,
}

pub trait LinkDefinitionWithData {
    fn link_chunk_name(&self) -> String;
    fn link_data_chunk_name(&self) -> String;

    fn convert(&self, data: Vec<u8>, prop_len: u32, link: ToTemplateLinkInfo) -> ToTemplateLink;
}

struct LinkDefinitionWithDataStruct<TData> {
    link_name: String,
    link_data_name: String,
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
                let prop: ROutput = serde_json::from_value(json.clone()).unwrap();
                world.add_component(*new_ent_id, prop);
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
        reader,
        converter,
    })
}
