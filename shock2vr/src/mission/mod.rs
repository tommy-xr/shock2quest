pub mod entity_creator;
use std::{fs::File, io::BufReader};

use tracing::info;
pub mod entity_populator;
pub mod mission_core;
pub mod pathfinding_debug;
pub mod pathfinding_test;
pub mod spatial_query;
mod spawn_location;
pub mod stim_response;
pub mod visibility_engine;

pub use mission_core::*;
pub use spatial_query::*;
pub use spawn_location::*;
pub use visibility_engine::*;

use cgmath::{Matrix4, Quaternion, Vector2, Vector3};
use rapier3d::prelude::{Collider, ColliderBuilder};

use engine::{
    assets::{asset_cache::AssetCache, asset_paths::AbstractAssetPath},
    audio::AudioContext,
    scene::{SceneObject, light::SpotLight},
};

use shipyard::World;
use shipyard::*;

use crate::{
    GameOptions,
    game_scene::AmbientAudioState,
    input_context::{self, InputContext},
    mission::entity_populator::EntityPopulator,
    quest_info::QuestInfo,
    save_load::HeldItemSaveData,
    scripts::{Effect, GlobalEffect},
    time::Time,
};

pub struct Mission {
    pub mission_core: MissionCore,
}

impl Mission {
    /// The GL-free, CPU-only first phase of a level load: read + parse the `.mis` into a
    /// `Send` `SystemShock2Level`. Takes only the `Send + Sync` asset-path layer and the
    /// gamesys/definitions (borrowed), so a worker thread can run it by owning `Arc`s of
    /// those and passing references in (projects/loading-screen.md). No GL, no AssetCache.
    pub fn parse(
        asset_paths: &dyn AbstractAssetPath,
        base_path: &str,
        mission: &str,
        global_context: &GlobalContext,
    ) -> dark::mission::SystemShock2Level {
        info!("starting level load");
        let f = File::open(resource_path(mission)).unwrap();
        let mut reader = BufReader::new(f);
        dark::mission::read(
            asset_paths,
            base_path,
            &mut reader,
            &global_context.gamesys,
            &global_context.links,
            &global_context.links_with_data,
            &global_context.properties,
        )
    }

    /// The main-thread second phase: GPU upload (`to_scene`) + physics/spatial build +
    /// entity instantiation. Consumes the parsed level.
    #[allow(clippy::too_many_arguments)]
    pub fn build(
        level: dark::mission::SystemShock2Level,
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
        let scene_objects = dark::mission::to_scene(&level, asset_cache);
        let song_params = level.song_params.clone();
        let room_db = level.room_database.clone();
        let physics_geometry = create_physics_collider(&level);
        let spatial_data = LevelSpatialData::from_level(&level);
        let obj_map = level.obj_map.clone();

        let abstract_mission = AbstractMission {
            scene_objects,
            song_params,
            room_db,
            physics_geometry,
            spatial_data: Some(Box::new(spatial_data)),
            entity_info: level.entity_info,
            obj_map,
            visibility_engine: Box::new(PortalVisibilityEngine::new()),
            path_database: level.path_database,
        };

        let mission_core = MissionCore::load(
            mission,
            abstract_mission,
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

    /// Synchronous load: `parse` then `build` on the calling thread (unchanged behavior).
    #[allow(clippy::too_many_arguments)]
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
        let base_path = asset_cache.base_path().to_string();
        let level = Self::parse(
            asset_cache.asset_paths(),
            &base_path,
            &mission,
            global_context,
        );
        Self::build(
            level,
            mission,
            asset_cache,
            audio_context,
            global_context,
            spawn_loc,
            quest_info,
            entity_populator,
            held_item_save_data,
            game_options,
        )
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

    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }

    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        Some(self)
    }

    fn as_debuggable(&self) -> Option<&dyn crate::game_scene::DebuggableScene> {
        Some(self)
    }

    fn as_debuggable_mut(&mut self) -> Option<&mut dyn crate::game_scene::DebuggableScene> {
        Some(self)
    }
}

// ============================================================================
// DebuggableScene Implementation for Mission
// ============================================================================

impl crate::game_scene::DebuggableScene for Mission {
    fn list_entities(
        &self,
        limit: Option<usize>,
        filter: Option<&str>,
    ) -> Vec<crate::game_scene::DebugEntitySummary> {
        self.mission_core.list_entities(limit, filter)
    }

    fn entity_detail(&self, id: EntityId) -> Option<crate::game_scene::DebugEntityDetail> {
        self.mission_core.entity_detail(id)
    }

    fn raycast(
        &self,
        start: cgmath::Point3<f32>,
        end: cgmath::Point3<f32>,
        mask: crate::game_scene::RaycastMask,
    ) -> crate::game_scene::DebugRayHit {
        self.mission_core.raycast(start, end, mask)
    }

    fn teleport_player(&mut self, position: Vector3<f32>) -> Result<(), String> {
        self.mission_core.teleport_player(position)
    }

    fn player_position(&self) -> Vector3<f32> {
        self.mission_core.player_position()
    }

    fn list_physics_bodies(
        &self,
        limit: Option<usize>,
    ) -> Vec<crate::game_scene::DebugPhysicsBodySummary> {
        self.mission_core.list_physics_bodies(limit)
    }

    fn physics_body_detail(
        &self,
        body_id: u32,
    ) -> Option<crate::game_scene::DebugPhysicsBodyDetail> {
        self.mission_core.physics_body_detail(body_id)
    }

    fn ragdoll_metrics(&self) -> Vec<crate::game_scene::DebugRagdollMetrics> {
        self.mission_core.ragdoll_metrics()
    }

    fn list_physics_joints(&self) -> Vec<crate::game_scene::DebugPhysicsJoint> {
        self.mission_core.list_physics_joints()
    }

    fn audit_colliders(&self) -> Vec<crate::game_scene::DebugColliderIssue> {
        self.mission_core.audit_colliders()
    }

    fn get_input_state(&self) -> crate::input_context::InputContext {
        self.mission_core.get_input_state()
    }

    fn set_input(&mut self, channel: &str, value: serde_json::Value) -> bool {
        self.mission_core.set_input(channel, value)
    }

    fn pathfinding_test_status(&self) -> crate::game_scene::DebugPathfindingTestStatus {
        self.mission_core.pathfinding_test_status()
    }

    fn pathfinding_stats(&self) -> Option<crate::game_scene::DebugPathfindingStats> {
        self.mission_core.pathfinding_stats()
    }

    fn send_entity_message(
        &mut self,
        id: EntityId,
        message: crate::game_scene::DebugEntityMessage,
    ) -> bool {
        self.mission_core.send_entity_message(id, message)
    }
}

/// Creates a physics collider from level geometry
/// This allows mission loading code to create physics geometry independently of the physics system
pub fn create_physics_collider(level: &dark::mission::SystemShock2Level) -> Option<Collider> {
    if level.all_geometry.is_empty() {
        return None;
    }

    let mut vertices = Vec::new();
    let mut indices = Vec::new();

    for geo in &level.all_geometry {
        let verts = &geo.verts;

        let mut idx = 0;
        let len = verts.len();

        while idx < len {
            let dest_idx = vertices.len() as u32;

            // Convert vertex positions to rapier3d format
            vertices.push(rapier3d::prelude::Point::new(
                verts[idx].position.x,
                verts[idx].position.y,
                verts[idx].position.z,
            ));
            vertices.push(rapier3d::prelude::Point::new(
                verts[idx + 1].position.x,
                verts[idx + 1].position.y,
                verts[idx + 1].position.z,
            ));
            vertices.push(rapier3d::prelude::Point::new(
                verts[idx + 2].position.x,
                verts[idx + 2].position.y,
                verts[idx + 2].position.z,
            ));

            indices.push([dest_idx, dest_idx + 1, dest_idx + 2]);

            idx += 3;
        }
    }

    ColliderBuilder::trimesh(vertices, indices)
        .ok()
        .map(|builder| builder.build())
}
