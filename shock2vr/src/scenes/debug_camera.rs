use cgmath::{Deg, Matrix4, Point3, Quaternion, Rotation3, Vector2, Vector3, point3, vec3};
use dark::SCALE_FACTOR;
use engine::{
    assets::asset_cache::AssetCache,
    audio::AudioContext,
    scene::{SceneObject, light::SpotLight},
};
use shipyard::EntityId;
use tracing::info;

use crate::{
    GameOptions,
    game_scene::GameScene,
    input_context::InputContext,
    mission::{GlobalContext, SpawnLocation, entity_creator::CreateEntityOptions},
    scenes::debug_common::{
        DebugScene, DebugSceneBuildOptions, DebugSceneBuilder, DebugSceneFloor,
    },
    scripts::{Effect, GlobalEffect},
    time::Time,
};

const FLOOR_COLOR: Vector3<f32> = Vector3::new(0.15, 0.15, 0.20);
const FLOOR_SIZE: Vector3<f32> = Vector3::new(120.0, 0.5, 120.0);
const CAMERA_START_POS: Point3<f32> = point3(0.0, 4.0 / SCALE_FACTOR, 5.0 / SCALE_FACTOR);
const CAMERA_TEMPLATE_ID: i32 = -367;

/// Debug scene that spawns a single camera entity so speech/awareness behaviour
/// can be exercised without loading a full level.
pub struct DebugCameraScene {
    scene: DebugScene,
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
        let builder = DebugSceneBuilder::new("debug_camera")
            .with_floor(DebugSceneFloor::ss2_units(FLOOR_SIZE, FLOOR_COLOR))
            .with_spawn_location(SpawnLocation::PositionRotation(
                vec3(0.0, 5.0 / SCALE_FACTOR, 0.0 / SCALE_FACTOR),
                Quaternion::from_angle_y(Deg(90.0)),
            ));

        let build_options = DebugSceneBuildOptions {
            global_context,
            game_options,
            asset_cache,
            audio_context,
        };

        let mut scene = builder.build(build_options);

        let camera_entity = Some(
            scene
                .core_mut()
                .create_entity_with_position(
                    asset_cache,
                    CAMERA_TEMPLATE_ID,
                    CAMERA_START_POS,
                    Quaternion::from_angle_y(Deg(180.0)),
                    Matrix4::from_translation(vec3(0.0, 1.0, 10.0)),
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
            scene,
            camera_entity,
        }
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
        self.scene.update(
            time,
            input_context,
            asset_cache,
            game_options,
            command_effects,
        )
    }

    fn render(
        &mut self,
        asset_cache: &mut AssetCache,
        options: &GameOptions,
    ) -> (Vec<SceneObject>, Vector3<f32>, Quaternion<f32>) {
        self.scene.render(asset_cache, options)
    }

    fn render_per_eye(
        &mut self,
        asset_cache: &mut AssetCache,
        view: Matrix4<f32>,
        projection: Matrix4<f32>,
        screen_size: Vector2<f32>,
        options: &GameOptions,
    ) -> Vec<SceneObject> {
        self.scene
            .render_per_eye(asset_cache, view, projection, screen_size, options)
    }

    fn finish_render(
        &mut self,
        asset_cache: &mut AssetCache,
        view: Matrix4<f32>,
        projection: Matrix4<f32>,
        screen_size: Vector2<f32>,
    ) {
        self.scene
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
        self.scene.handle_effects(
            effects,
            global_context,
            game_options,
            asset_cache,
            audio_context,
        )
    }

    fn get_hand_spotlights(&self, options: &GameOptions) -> Vec<SpotLight> {
        self.scene.get_hand_spotlights(options)
    }

    fn world(&self) -> &shipyard::World {
        self.scene.world()
    }

    fn scene_name(&self) -> &str {
        self.scene.scene_name()
    }

    fn queue_entity_trigger(&mut self, entity_name: String) {
        self.scene.queue_entity_trigger(entity_name)
    }
}
