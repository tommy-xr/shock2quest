pub mod entity_creator;
use tracing::info;
mod ammo_pouch;
mod body_gear_feedback;
mod body_inventory;
pub mod entity_populator;
pub mod flat_ui_host;
mod holsters;
pub mod mission_core;
pub mod pathfinding_debug;
pub mod pathfinding_test;
pub mod player_footsteps;
pub(crate) mod reload;
mod shoulder_backpack;
pub mod spatial_query;
mod spawn_location;
pub mod stim_response;
pub(crate) mod turn_clip;
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
        // Through the asset paths, not `File::open`: on a 25AE install the
        // missions live inside `sshock2.kpf`.
        let reader = asset_paths
            .get_reader(base_path.to_owned(), mission.to_ascii_lowercase())
            .unwrap_or_else(|| panic!("mission {mission} not found in the mounted data"));
        dark::mission::read(
            asset_paths,
            base_path,
            &mut *reader.borrow_mut(),
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
        engine::platform::service_events();
        let mission_scene = dark::mission::to_scene(&level, asset_cache);
        engine::platform::service_events();
        let song_params = level.song_params.clone();
        let room_db = level.room_database.clone();
        let map_params = level.map_params;
        let physics_geometry = create_physics_collider(&level);
        engine::platform::service_events();
        let spatial_data = LevelSpatialData::from_level(&level);
        engine::platform::service_events();
        let obj_map = level.obj_map.clone();

        let abstract_mission = AbstractMission {
            scene_objects: mission_scene.objects,
            animated_lightmaps: Some(mission_scene.animated_lightmaps),
            song_params,
            room_db,
            map_params,
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
        engine::platform::service_events();
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
        engine::platform::service_events();
        let base_path = asset_cache.base_path().to_string();
        let level = Self::parse(
            asset_cache.asset_paths(),
            &base_path,
            &mission,
            global_context,
        );
        engine::platform::service_events();
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
    fn is_pausable(&self) -> bool {
        true
    }

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

    fn wants_pointer(&self) -> bool {
        self.mission_core.wants_pointer()
    }

    fn player_is_crouched(&self) -> bool {
        self.mission_core.player_is_crouched()
    }

    fn player_tracking_is_crouched(&self) -> bool {
        self.mission_core.player_tracking_is_crouched()
    }

    fn player_is_gripping(&self) -> bool {
        self.mission_core.player_is_gripping()
    }

    fn fov_pull_deg(&self, game_options: &GameOptions) -> f32 {
        self.mission_core.fov_pull_deg(game_options)
    }

    fn use_mode_vignette_intensity(&self) -> f32 {
        self.mission_core.use_mode_vignette_intensity()
    }

    fn death_camera(&self) -> Option<crate::death_camera::DeathCameraSample> {
        self.mission_core.death_camera()
    }

    fn player_save_position(&self) -> Result<Vector3<f32>, crate::game_scene::PlayerSavePoseError> {
        self.mission_core.player_save_position()
    }

    fn world(&self) -> &World {
        &self.mission_core.world
    }

    fn script_world(&self) -> Option<&crate::scripts::ScriptWorld> {
        Some(&self.mission_core.script_world)
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

    fn resolve_entity_id(&self, id: i32) -> Option<EntityId> {
        self.mission_core.resolve_entity_id(id)
    }

    fn entity_detail(&self, id: EntityId) -> Option<crate::game_scene::DebugEntityDetail> {
        self.mission_core.entity_detail(id)
    }

    fn animation_state(&self, id: EntityId) -> Option<crate::game_scene::DebugAnimationState> {
        self.mission_core.animation_state(id)
    }

    fn raycast(
        &self,
        start: cgmath::Point3<f32>,
        end: cgmath::Point3<f32>,
        mask: crate::game_scene::RaycastMask,
    ) -> crate::game_scene::DebugRayHit {
        self.mission_core.raycast(start, end, mask)
    }

    fn climb_grip(
        &self,
        point: Vector3<f32>,
        radius: Option<f32>,
        feet_y: Option<f32>,
    ) -> Option<crate::game_scene::DebugClimbGrip> {
        self.mission_core.climb_grip(point, radius, feet_y)
    }

    fn hand_feedback(&self) -> serde_json::Value {
        self.mission_core.hand_feedback()
    }

    fn hand_grips(&self) -> serde_json::Value {
        self.mission_core.hand_grips()
    }

    fn player_climb(&self) -> Option<crate::game_scene::DebugClimbState> {
        self.mission_core.player_climb()
    }

    fn teleport_player(&mut self, position: Vector3<f32>) -> Result<(), String> {
        self.mission_core.teleport_player(position)
    }

    fn move_player(&mut self, target: Vector3<f32>) -> crate::physics::MoveResult {
        self.mission_core.move_player(target)
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

    fn apply_body_impulse(&mut self, body_id: u32, impulse: [f32; 3]) -> bool {
        self.mission_core.apply_body_impulse(body_id, impulse)
    }

    fn audit_colliders(&self) -> Vec<crate::game_scene::DebugColliderIssue> {
        self.mission_core.audit_colliders()
    }

    fn quest_bits(&self) -> Vec<crate::game_scene::DebugQuestBit> {
        self.mission_core.quest_bits()
    }

    fn ui_state(&self) -> crate::game_scene::DebugUiState {
        self.mission_core.ui_state()
    }

    fn set_quest_bit(&mut self, name: &str, value: &str) -> Result<(), String> {
        self.mission_core.set_quest_bit(name, value)
    }

    fn player_inventory(&self) -> Vec<crate::game_scene::DebugInventoryItem> {
        self.mission_core.player_inventory()
    }

    fn give_item(&mut self, entity_id: shipyard::EntityId) -> Result<(), String> {
        self.mission_core.give_item(entity_id)
    }

    fn spawn_item_for_player(
        &mut self,
        asset_cache: &mut AssetCache,
        template: &crate::game_scene::DebugItemTemplate,
    ) -> Result<crate::game_scene::DebugSpawnedItem, String> {
        self.mission_core
            .spawn_item_for_player(asset_cache, template)
    }

    fn set_player_stats(
        &mut self,
        request: &crate::game_scene::DebugPlayerStatsRequest,
    ) -> Result<crate::player_stats::PlayerStats, String> {
        self.mission_core.set_player_stats(request)
    }

    fn list_transitions(&self) -> Vec<crate::game_scene::DebugTransition> {
        self.mission_core.list_transitions()
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

    fn ai_paths(&self) -> Vec<crate::game_scene::DebugAiPathEntry> {
        self.mission_core.ai_paths()
    }

    fn pathfinding_route(
        &self,
        from: [f32; 3],
        to: [f32; 3],
    ) -> Option<crate::game_scene::DebugPathRoute> {
        self.mission_core.pathfinding_route(from, to)
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
