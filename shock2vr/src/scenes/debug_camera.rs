use std::collections::HashMap;

use cgmath::{
    point3, vec3, Deg, Matrix4, Point3, Quaternion, Rotation3, SquareMatrix, Vector2, Vector3,
};
use dark::{
    mission::{room_database::RoomDatabase, SongParams},
    ss2_entity_info::SystemShock2EntityInfo,
    SCALE_FACTOR,
};
use engine::{
    assets::asset_cache::AssetCache,
    audio::AudioContext,
    scene::{color_material, light::SpotLight, SceneObject},
};
use rapier3d::prelude::{Collider, ColliderBuilder};
use shipyard::EntityId;
use tracing::info;

use crate::{
    game_scene::GameScene,
    input_context::InputContext,
    mission::{
        entity_creator::CreateEntityOptions,
        entity_populator::empty_entity_populator::EmptyEntityPopulator, mission_core::MissionCore,
        AbstractMission, AlwaysVisible, GlobalContext, SpawnLocation,
    },
    quest_info::QuestInfo,
    save_load::HeldItemSaveData,
    scripts::{Effect, GlobalEffect},
    time::Time,
    GameOptions,
};

const FLOOR_COLOR: Vector3<f32> = Vector3::new(0.15, 0.15, 0.20);
const FLOOR_SIZE: Vector3<f32> = Vector3::new(120.0, 0.5, 120.0);
const CAMERA_START_POS: Point3<f32> = point3(0.0, 4.0 / SCALE_FACTOR, 5.0 / SCALE_FACTOR);
const CAMERA_TEMPLATE_ID: i32 = -367;

/// Debug scene that spawns a single camera entity so speech/awareness behaviour
/// can be exercised without loading a full level.
pub struct DebugCameraScene {
    core: MissionCore,
    #[allow(dead_code)]
    camera_entity: Option<EntityId>,
}

impl DebugCameraScene {
    pub fn new(
        global_context: &GlobalContext,
        game_options: &GameOptions,
        asset_cache: &mut AssetCache,
        audio_context: &mut AudioContext<EntityId, String>,
    ) -> Self {
        let abstract_mission = Self::create_debug_mission();

        let mut core = MissionCore::load(
            "debug_camera".to_string(),
            abstract_mission,
            asset_cache,
            audio_context,
            global_context,
            SpawnLocation::PositionRotation(
                vec3(0.0, 5.0 / SCALE_FACTOR, -5.0 / SCALE_FACTOR),
                Quaternion::from_angle_y(Deg(0.0)),
            ),
            QuestInfo::new(),
            Box::new(EmptyEntityPopulator {}),
            HeldItemSaveData::empty(),
            game_options,
        );

        let camera_entity = Some(
            core.create_entity_with_position(
                asset_cache,
                CAMERA_TEMPLATE_ID,
                CAMERA_START_POS,
                Quaternion::from_angle_y(Deg(180.0)),
                Matrix4::identity(),
                CreateEntityOptions::default(),
            )
            .entity_id,
        );

        match camera_entity {
            Some(id) => info!(
                "Spawned debug camera entity {id:?} at ({:.2}, {:.2}, {:.2})",
                CAMERA_START_POS.x, CAMERA_START_POS.y, CAMERA_START_POS.z
            ),
            None => info!("Failed to spawn debug camera entity from template 'vcamera'"),
        }

        Self {
            core,
            camera_entity,
        }
    }

    fn create_debug_mission() -> AbstractMission {
        let scene_objects = Self::create_floor_scene_objects();
        let physics_geometry = Self::create_floor_physics();
        let entity_info = SystemShock2EntityInfo::empty();
        let obj_map = HashMap::new();

        AbstractMission {
            scene_objects,
            song_params: SongParams {
                song: String::new(),
            },
            room_db: RoomDatabase { rooms: Vec::new() },
            physics_geometry: Some(physics_geometry),
            spatial_data: None,
            entity_info,
            obj_map,
            visibility_engine: Box::new(AlwaysVisible),
        }
    }

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

    fn create_floor_physics() -> Collider {
        let floor_size_scaled = vec3(
            FLOOR_SIZE.x / SCALE_FACTOR / 2.0,
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
}

impl GameScene for DebugCameraScene {
    fn update(
        &mut self,
        time: &Time,
        input_context: &InputContext,
        asset_cache: &mut AssetCache,
        game_options: &GameOptions,
        command_effects: Vec<Effect>,
    ) -> Vec<Effect> {
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
        self.core.handle_effects(
            effects,
            global_context,
            game_options,
            asset_cache,
            audio_context,
        )
    }

    fn get_hand_spotlights(&self, options: &GameOptions) -> Vec<SpotLight> {
        self.core.get_hand_spotlights(options)
    }

    fn world(&self) -> &shipyard::World {
        self.core.world()
    }

    fn scene_name(&self) -> &str {
        self.core.scene_name()
    }

    fn queue_entity_trigger(&mut self, entity_name: String) {
        self.core.queue_entity_trigger(entity_name)
    }
}
