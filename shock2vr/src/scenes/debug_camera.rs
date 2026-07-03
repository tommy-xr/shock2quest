use cgmath::{Deg, Matrix4, Point3, Quaternion, Rotation3, point3, vec3};
use dark::SCALE_FACTOR;
use engine::{assets::asset_cache::AssetCache, audio::AudioContext};
use shipyard::EntityId;
use tracing::info;

use crate::{
    GameOptions,
    game_scene::GameScene,
    mission::{GlobalContext, entity_creator::CreateEntityOptions},
    scenes::debug_common::{
        AutoEquipHooks, DebugSceneBuildOptions, DebugSceneBuilder, HookedDebugScene,
    },
};

const CAMERA_START_POS: Point3<f32> = point3(0.0, 4.0 / SCALE_FACTOR, 5.0 / SCALE_FACTOR);
const CAMERA_TEMPLATE_ID: i32 = -367;

/// The Psi Amp player weapon - equipped so psi powers that affect AI
/// perception (e.g. Photonic Redirection) can be cast against the camera.
const PSI_AMP_TEMPLATE_ID: i32 = -247;

/// Namespace for constructing debug camera scenes.
pub struct DebugCameraScene;

impl DebugCameraScene {
    pub fn new(
        global_context: &GlobalContext,
        game_options: &GameOptions,
        asset_cache: &mut AssetCache,
        audio_context: &mut AudioContext<EntityId, String>,
    ) -> Box<dyn GameScene> {
        let builder = DebugSceneBuilder::new("debug_camera").with_default_floor();

        let build_options = DebugSceneBuildOptions {
            global_context,
            game_options,
            asset_cache,
            audio_context,
        };

        let mut core = builder.build_core(build_options);

        let camera_entity = core
            .create_entity_with_position(
                asset_cache,
                CAMERA_TEMPLATE_ID,
                CAMERA_START_POS,
                Quaternion::from_angle_y(Deg(180.0)),
                Matrix4::from_translation(vec3(0.0, 1.0, 10.0)),
                CreateEntityOptions::default(),
            )
            .entity_id;

        info!("Spawned debug camera entity {camera_entity:?}");

        // Equip the player with the psi amp on the first update (same
        // pattern as `debug_psi`), so camera-perception powers are castable
        // in this scene.
        Box::new(HookedDebugScene::new(
            core,
            AutoEquipHooks::new(PSI_AMP_TEMPLATE_ID),
        ))
    }
}
