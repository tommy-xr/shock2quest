pub mod entity_creator;
pub mod entity_populator;
pub mod mission_core;
mod spawn_location;
pub mod visibility_engine;

pub use mission_core::*;
pub use spawn_location::*;
pub use visibility_engine::*;

use cgmath::{Matrix4, Quaternion, Vector2, Vector3};

use engine::{
    assets::asset_cache::AssetCache,
    audio::AudioContext,
    scene::{light::SpotLight, SceneObject},
};

use shipyard::World;
use shipyard::*;

use crate::{
    game_scene::AmbientAudioState,
    input_context::{self, InputContext},
    mission::entity_populator::EntityPopulator,
    quest_info::QuestInfo,
    save_load::HeldItemSaveData,
    scripts::{Effect, GlobalEffect},
    time::Time,
    GameOptions,
};

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
