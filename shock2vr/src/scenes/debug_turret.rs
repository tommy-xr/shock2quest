use cgmath::{Deg, Matrix4, Point3, Quaternion, Rotation3, point3, vec3};
use dark::SCALE_FACTOR;
use engine::{assets::asset_cache::AssetCache, audio::AudioContext};
use shipyard::EntityId;
use tracing::info;

use crate::{
    GameOptions,
    game_scene::GameScene,
    mission::{GlobalContext, entity_creator::CreateEntityOptions},
    scenes::debug_common::{DebugSceneBuildOptions, DebugSceneBuilder},
};

const TURRET_START_POS: Point3<f32> = point3(0.0, 2.0 / SCALE_FACTOR, 5.0 / SCALE_FACTOR);
const LASER_TURRET_TEMPLATE_ID: i32 = -168;

/// Namespace for constructing debug turret scenes.
pub struct DebugTurretScene;

impl DebugTurretScene {
    pub fn new(
        global_context: &GlobalContext,
        game_options: &GameOptions,
        asset_cache: &mut AssetCache,
        audio_context: &mut AudioContext<EntityId, String>,
    ) -> Box<dyn GameScene> {
        let builder = DebugSceneBuilder::new("debug_turret").with_default_floor();

        let build_options = DebugSceneBuildOptions {
            global_context,
            game_options,
            asset_cache,
            audio_context,
        };

        let mut scene = builder.build(build_options);

        let turret_entity = scene
            .core_mut()
            .create_entity_with_position(
                asset_cache,
                LASER_TURRET_TEMPLATE_ID,
                TURRET_START_POS,
                Quaternion::from_angle_y(Deg(180.0)),
                Matrix4::from_translation(vec3(0.0, 1.0, 10.0)),
                CreateEntityOptions::default(),
            )
            .entity_id;

        info!("Spawned debug turret entity {turret_entity:?}");

        Box::new(scene)
    }
}
