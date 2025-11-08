use std::collections::HashMap;

use cgmath::{Matrix4, Quaternion, Vector2, Vector3, vec3};
use dark::{
    SCALE_FACTOR,
    mission::{SongParams, room_database::RoomDatabase},
    ss2_entity_info::SystemShock2EntityInfo,
};
use engine::{
    assets::asset_cache::AssetCache,
    audio::AudioContext,
    scene::{SceneObject, color_material, light::SpotLight},
};
use rapier3d::prelude::{Collider, ColliderBuilder};
use shipyard::EntityId;

use crate::{
    GameOptions,
    game_scene::GameScene,
    input_context::InputContext,
    mission::{
        AbstractMission, AlwaysVisible, GlobalContext, SpawnLocation,
        entity_populator::empty_entity_populator::EmptyEntityPopulator, mission_core::MissionCore,
    },
    quest_info::QuestInfo,
    save_load::HeldItemSaveData,
    scripts::{Effect, GlobalEffect},
    time::Time,
};

const FLOOR_COLOR: Vector3<f32> = Vector3::new(0.15, 0.15, 0.20);
const FLOOR_SIZE: Vector3<f32> = Vector3::new(120.0, 0.5, 120.0);

/// Debug scene that demonstrates MissionCore working with custom debug data
/// Creates a simple floor with colored cubes for testing physics and rendering
pub struct DebugEntityPlaygroundScene {
    core: MissionCore,
}

impl DebugEntityPlaygroundScene {
    pub fn new(
        global_context: &GlobalContext,
        game_options: &GameOptions,
        asset_cache: &mut AssetCache,
        audio_context: &mut AudioContext<EntityId, String>,
    ) -> Self {
        // Create debug AbstractMission with minimal data
        let abstract_mission = Self::create_debug_mission();

        // Create MissionCore using our new load method
        let core = MissionCore::load(
            "debug_entity_playground".to_string(),
            abstract_mission,
            asset_cache,
            audio_context,
            global_context,
            SpawnLocation::PositionRotation(
                vec3(0.0, 5.0 / SCALE_FACTOR, 0.0), // Start well above the floor
                Quaternion::new(1.0, 0.0, 0.0, 0.0),
            ),
            QuestInfo::new(),
            Box::new(EmptyEntityPopulator {}),
            HeldItemSaveData::empty(),
            game_options,
        );

        Self { core }
    }

    /// Create a debug AbstractMission with minimal required data
    fn create_debug_mission() -> AbstractMission {
        // Create visual scene objects (floor)
        let scene_objects = Self::create_floor_scene_objects();

        // Create custom physics geometry (floor collision)
        let physics_geometry = Self::create_floor_physics();

        // Create minimal entity info and obj map
        let entity_info = Self::create_empty_entity_info();
        let obj_map = HashMap::new();

        AbstractMission {
            scene_objects,
            song_params: SongParams {
                song: String::new(),
            },
            room_db: RoomDatabase { rooms: Vec::new() },
            physics_geometry: Some(physics_geometry),
            spatial_data: None, // No spatial queries needed for simple debug scene
            entity_info,
            obj_map,
            visibility_engine: Box::new(AlwaysVisible),
        }
    }

    /// Create floor scene objects for rendering
    fn create_floor_scene_objects() -> Vec<SceneObject> {
        let floor_size_scaled = vec3(
            FLOOR_SIZE.x / SCALE_FACTOR,
            FLOOR_SIZE.y / SCALE_FACTOR,
            FLOOR_SIZE.z / SCALE_FACTOR,
        );

        let floor_transform = Matrix4::from_translation(vec3(0.0, 0.0, 0.0))
            * Matrix4::from_nonuniform_scale(
                floor_size_scaled.x,
                floor_size_scaled.y,
                floor_size_scaled.z,
            );

        let floor_material = color_material::create(FLOOR_COLOR);
        let mut floor_object =
            SceneObject::new(floor_material, Box::new(engine::scene::cube::create()));
        floor_object.set_transform(floor_transform);

        vec![floor_object]
    }

    /// Create floor physics collider
    fn create_floor_physics() -> Collider {
        let floor_size_scaled = vec3(
            FLOOR_SIZE.x / SCALE_FACTOR / 2.0, // Half extents for box collider
            FLOOR_SIZE.y / SCALE_FACTOR / 2.0,
            FLOOR_SIZE.z / SCALE_FACTOR / 2.0,
        );

        ColliderBuilder::cuboid(
            floor_size_scaled.x,
            floor_size_scaled.y,
            floor_size_scaled.z,
        )
        .build()
    }

    /// Create empty entity info for debug scene
    /// We'll create a basic one and rely on the merge_with_gamesys call in MissionCore::load
    fn create_empty_entity_info() -> SystemShock2EntityInfo {
        // Use the new empty() constructor from SystemShock2EntityInfo
        SystemShock2EntityInfo::empty()
    }
}

impl Default for DebugEntityPlaygroundScene {
    fn default() -> Self {
        // This won't work without required parameters, but satisfies the trait
        panic!(
            "DebugEntityPlaygroundScene requires GlobalContext, AssetCache, and AudioContext - use new() instead"
        )
    }
}

impl GameScene for DebugEntityPlaygroundScene {
    fn update(
        &mut self,
        time: &Time,
        input_context: &InputContext,
        asset_cache: &mut AssetCache,
        game_options: &GameOptions,
        command_effects: Vec<Effect>,
    ) -> Vec<Effect> {
        // Delegate to core
        self.core.update(
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
        // Delegate to core
        self.core.render(asset_cache, options)
    }

    fn render_per_eye(
        &mut self,
        asset_cache: &mut AssetCache,
        view: Matrix4<f32>,
        projection: Matrix4<f32>,
        screen_size: Vector2<f32>,
        options: &GameOptions,
    ) -> Vec<SceneObject> {
        // Delegate to core
        self.core
            .render_per_eye(asset_cache, view, projection, screen_size, options)
    }

    fn finish_render(
        &mut self,
        asset_cache: &mut AssetCache,
        view: Matrix4<f32>,
        projection: Matrix4<f32>,
        screen_size: Vector2<f32>,
    ) {
        // Delegate to core
        self.core
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
        // Delegate to core
        self.core.handle_effects(
            effects,
            global_context,
            game_options,
            asset_cache,
            audio_context,
        )
    }

    fn get_hand_spotlights(&self, options: &GameOptions) -> Vec<SpotLight> {
        // Delegate to core
        self.core.get_hand_spotlights(options)
    }

    fn world(&self) -> &shipyard::World {
        // Delegate to core
        self.core.world()
    }

    fn scene_name(&self) -> &str {
        // Delegate to core
        self.core.scene_name()
    }

    fn queue_entity_trigger(&mut self, entity_name: String) {
        // Delegate to core
        self.core.queue_entity_trigger(entity_name)
    }
}
