use cgmath::{Matrix4, Point3, Quaternion, Vector2, Vector3, Vector4};
use dark::{
    EnvSoundQuery,
    motion::{MotionQueryItem, MotionQuerySelectionStrategy},
    properties::{AIAlertLevel, AIMode, KeyCard, QuestBitValue},
};
use engine::audio::AudioHandle;
use shipyard::EntityId;

use crate::{
    gui::{GuiComponentRenderInfo, GuiHandle},
    mission::entity_creator::CreateEntityOptions,
    vr_config::Handedness,
};

use super::Message;

#[derive(Clone, Debug)]
pub enum GlobalEffect {
    // Save the game state to the given file_name
    Save {
        file_name: String,
    },

    // Load the game state from the given file_naem
    Load {
        file_name: String,
    },

    TransitionLevel {
        level_file: String,
        loc: Option<i32>,
        entities_to_trigger: Vec<String>,
    },

    // Test the reload functionality (as if saving + loading)
    TestReload,
}

#[derive(Clone, Debug)]
pub enum AIPropertyUpdate {
    Alertness {
        level: AIAlertLevel,
        peak: AIAlertLevel,
    },
    Mode {
        mode: AIMode,
    },
}

#[derive(Clone, Debug)]
pub enum Effect {
    NoEffect,

    AwardXP {
        amount: i32,
    },

    AdjustHitPoints {
        entity_id: EntityId,
        delta: i32,
    },

    ApplyForce {
        entity_id: EntityId,
        force: Vector3<f32>,
    },

    ChangeModel {
        entity_id: EntityId,
        model_name: String,
    },
    CreateEntityByTemplateName {
        template_name: String,
        position: Point3<f32>,
        orientation: Quaternion<f32>,
    },

    CreateEntity {
        template_id: i32,
        position: Point3<f32>,
        orientation: Quaternion<f32>,
        root_transform: Matrix4<f32>,
        options: CreateEntityOptions,
    },

    DrawDebugLines {
        lines: Vec<(Point3<f32>, Point3<f32>, Vector4<f32>)>,
    },

    DestroyEntity {
        entity_id: EntityId,
    },
    DropEntityInfo {
        parent_entity_id: EntityId,
        dropped_entity_id: EntityId,
    },
    GrabEntity {
        entity_id: EntityId,
        hand: Handedness,
        current_parent_id: Option<EntityId>,
    },
    SlayEntity {
        entity_id: EntityId,
    },

    QueueAnimationBySchema {
        // ActorType, MotActorTags get inferred from the entity id
        entity_id: EntityId,
        selection_strategy: MotionQuerySelectionStrategy,
        motion_query_items: Vec<MotionQueryItem>,
    },

    ReplaceEntity {
        entity_id: EntityId,
        template_id: i32,
    },

    Send {
        msg: Message,
    },
    PlayEmail {
        deck: u32,
        email: u32,
        force: bool,
    },
    PlaySound {
        handle: AudioHandle,
        name: String,
    },
    PlaySpeech {
        entity_id: EntityId,
        voice_index: usize,
        concept: String,
        tags: Vec<(String, String)>,
    },
    PlayEnvironmentalSound {
        audio_handle: AudioHandle,
        query: EnvSoundQuery,
        position: Vector3<f32>,
    },
    PositionInventory {
        position: Vector3<f32>,
        rotation: Quaternion<f32>,
    },
    StopSound {
        handle: AudioHandle,
    },
    SetPosition {
        entity_id: EntityId,
        position: Vector3<f32>,
    },
    SetRotation {
        entity_id: EntityId,
        rotation: Quaternion<f32>,
    },
    SetPositionRotation {
        entity_id: EntityId,
        position: Vector3<f32>,
        rotation: Quaternion<f32>,
    },
    SetJointTransform {
        entity_id: EntityId,
        joint_id: u32,
        transform: Matrix4<f32>,
    },
    SetPlayerPosition {
        position: Vector3<f32>,
        is_teleport: bool,
    },

    ResetGravity {
        entity_id: EntityId,
    },

    SetGravity {
        entity_id: EntityId,
        gravity_percent: f32,
    },

    SetQuestBit {
        quest_bit_name: String,
        quest_bit_value: QuestBitValue,
    },

    SetAIProperty {
        entity_id: EntityId,
        update: AIPropertyUpdate,
    },

    AcquireKeyCard {
        key_card: KeyCard,
    },

    TurnOffTweqs {
        entity_id: EntityId,
    },
    TurnOnTweqs {
        entity_id: EntityId,
    },

    SetUI {
        parent_entity: EntityId,
        handle: GuiHandle,
        world_offset: Vector3<f32>,
        world_size: Vector2<f32>,
        components: Vec<GuiComponentRenderInfo>,
    },

    Multiple(Vec<Effect>),
    // Deprecated:
    // Use Multiple instead
    Combined {
        effects: Vec<Effect>,
    },

    GlobalEffect(GlobalEffect),

    /// Interactive pathfinding test system
    PathfindingTest,
}

impl Effect {
    pub fn combine(effects: Vec<Effect>) -> Effect {
        Effect::Combined { effects }
    }
    pub fn flatten(effects: Vec<Effect>) -> Vec<Effect> {
        let mut ret = Vec::new();

        for v in effects {
            match v {
                Effect::NoEffect => (),
                Effect::Multiple(inner_effects) => {
                    ret.append(&mut Self::flatten(inner_effects).clone())
                }
                Effect::Combined {
                    effects: inner_effects,
                } => ret.append(&mut Self::flatten(inner_effects).clone()),
                _ => ret.push(v),
            }
        }

        ret
    }
}
