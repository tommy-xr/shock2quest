pub mod entity_creator;
pub mod entity_populator;
pub mod mission_core;
mod spawn_location;
pub mod visibility_engine;

pub use mission_core::*;
pub use spawn_location::*;
pub use visibility_engine::*;

use std::{
    collections::{HashMap, HashSet},
    fs::File,
    io::BufReader,
    rc::Rc,
    time::{Duration, SystemTime},
};

use cgmath::{
    num_traits::ToPrimitive, vec3, InnerSpace, Matrix4, Point3, Quaternion, Rotation, Rotation3,
    SquareMatrix, Transform, Vector2, Vector3,
};
use cgmath::{EuclideanSpace, Zero};

use dark::{
    audio::SongPlayer,
    gamesys::Gamesys,
    importers::{ANIMATION_CLIP_IMPORTER, AUDIO_IMPORTER, MODELS_IMPORTER, SONG_IMPORTER},
    mission::{room_database::RoomDatabase, SystemShock2Level},
    model::Model,
    motion::{AnimationEvent, AnimationPlayer, MotionQuery, MotionQueryItem},
    properties::{
        AmbientSoundFlags, Link, Links, PhysicsModelType, PropAmbientHacked, PropCreature,
        PropFrameAnimState, PropHasRefs, PropLocalPlayer, PropModelName, PropMotionActorTags,
        PropParticleGroup, PropParticleLaunchInfo, PropPhysDimensions, PropPhysInitialVelocity,
        PropPhysState, PropPhysType, PropPosition, PropRenderType, PropScripts, PropTeleported,
        PropTripFlags, RenderType, ToLink, TripFlags, WrappedEntityId,
    },
    ss2_entity_info::{self, SystemShock2EntityInfo},
    BitmapAnimation, SCALE_FACTOR,
};
use engine::{
    assets::asset_cache::AssetCache,
    audio::{AudioChannel, AudioContext, AudioHandle},
    game_log, profile,
    scene::{
        light::SpotLight, quad, BillboardMaterial, ParticleSystem, SceneObject, VertexPosition,
    },
    texture::TextureTrait,
};
use physics::PhysicsWorld;
use rapier3d::prelude::RigidBodyHandle;
use scripts::ScriptWorld;

use shipyard::*;
use shipyard::{self, View, World};
use tracing::{info, trace, warn};

use crate::{
    creature::{get_creature_definition, HitBoxManager},
    game_scene::AmbientAudioState,
    gui::GuiManager,
    hud::{draw_item_name, draw_item_outline},
    input_context::{self, InputContext},
    inventory::PlayerInventoryEntity,
    mission::entity_populator::EntityPopulator,
    physics::{self, PlayerHandle},
    quest_info::QuestInfo,
    runtime_props::{
        RuntimePropDoNotSerialize, RuntimePropJointTransforms, RuntimePropTransform,
        RuntimePropVhots,
    },
    save_load::HeldItemSaveData,
    scripts::{
        self,
        internal_fast_projectile::InternalFastProjectileScript,
        script_util::{get_all_links_with_template, get_environmental_sound_query},
        AIPropertyUpdate, Effect, GlobalEffect, Message, MessagePayload,
    },
    systems::{run_bitmap_animation, run_tweq, turn_off_tweqs, turn_on_tweqs},
    teleport::TeleportSystem,
    time::Time,
    util::{get_email_sound_file, has_refs, vec3_to_point3},
    virtual_hand::{VirtualHand, VirtualHandEffect},
    vr_config, GameOptions,
};

use self::{
    entity_creator::{CreateEntityOptions, EntityCreationInfo},
    visibility_engine::VisibilityEngine,
};
pub use crate::resource_path;

pub struct Mission {
    pub mission_core: MissionCore,
}

impl Mission {
    pub fn load(
        mission: String,
        asset_cache: &mut AssetCache,
        audio_context: &mut AudioContext<EntityId, String>,
        global_context: &GlobalContext,
        spawn_loc: SpawnLocation,
        quest_info: QuestInfo,
        entity_populator: Box<dyn EntityPopulator>,
        held_item_save_data: HeldItemSaveData,
        game_options: &GameOptions,
    ) -> Mission {
        let mission_core = MissionCore::load(
            mission,
            asset_cache,
            audio_context,
            global_context,
            spawn_loc,
            quest_info,
            entity_populator,
            held_item_save_data,
            game_options,
        );
        Mission { mission_core }
    }

    pub fn update(
        &mut self,
        time: &Time,
        asset_cache: &mut AssetCache,
        input_context: &input_context::InputContext,
        game_options: &GameOptions,
        command_effects: Vec<Effect>,
    ) -> Vec<Effect> {
        self.mission_core.update(
            time,
            asset_cache,
            input_context,
            game_options,
            command_effects,
        )
    }
}

// Implementation of GameScene trait for Mission
impl crate::game_scene::GameScene for Mission {
    fn update(
        &mut self,
        time: &Time,
        input_context: &InputContext,
        asset_cache: &mut AssetCache,
        game_options: &GameOptions,
        command_effects: Vec<Effect>,
    ) -> Vec<Effect> {
        self.update(
            time,
            asset_cache,
            input_context,
            game_options,
            command_effects,
        )
    }

    fn render(
        &mut self,
        asset_cache: &mut AssetCache,
        options: &GameOptions,
    ) -> (Vec<SceneObject>, Vector3<f32>, Quaternion<f32>) {
        self.mission_core.render(asset_cache, options)
    }

    fn render_per_eye(
        &mut self,
        asset_cache: &mut AssetCache,
        view: Matrix4<f32>,
        projection: Matrix4<f32>,
        screen_size: Vector2<f32>,
        options: &GameOptions,
    ) -> Vec<SceneObject> {
        self.mission_core
            .render_per_eye(asset_cache, view, projection, screen_size, options)
    }

    fn finish_render(
        &mut self,
        asset_cache: &mut AssetCache,
        view: Matrix4<f32>,
        projection: Matrix4<f32>,
        screen_size: Vector2<f32>,
    ) {
        self.mission_core
            .finish_render(asset_cache, view, projection, screen_size)
    }

    fn handle_effects(
        &mut self,
        effects: Vec<Effect>,
        global_context: &GlobalContext,
        game_options: &GameOptions,
        asset_cache: &mut AssetCache,
        audio_context: &mut AudioContext<EntityId, String>,
    ) -> Vec<GlobalEffect> {
        self.mission_core.handle_effects(
            effects,
            global_context,
            game_options,
            asset_cache,
            audio_context,
        )
    }

    fn get_hand_spotlights(&self, options: &GameOptions) -> Vec<SpotLight> {
        self.mission_core.get_hand_spotlights(options)
    }

    fn world(&self) -> &World {
        &self.mission_core.world
    }

    fn scene_name(&self) -> &str {
        &self.mission_core.level_name
    }

    fn ambient_audio_state(&self) -> Option<AmbientAudioState> {
        self.mission_core.ambient_audio_state()
    }

    fn queue_entity_trigger(&mut self, entity_name: String) {
        self.mission_core.queue_entity_trigger(entity_name);
    }
}
