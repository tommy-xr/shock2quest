use cgmath::{Deg, Matrix4, Point3, Quaternion, Rotation3, point3, vec3};
use dark::SCALE_FACTOR;
use dark::properties::{Link, Links, PropEcology, ToLink, WrappedEntityId};
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
/// The gamesys security ecology (`TriggerEcology`) a camera raises. Levels
/// author their own copies; this is the base template.
const ECOLOGY_TEMPLATE_ID: i32 = -975;
/// The station security computer (`SecurityComputer`) - using it stands the
/// alarm down.
const SECURITY_COMPUTER_TEMPLATE_ID: i32 = -1250;
/// Off to the camera's left, on the same floor.
const CONSOLE_START_POS: Point3<f32> = point3(-3.0, 2.0 / SCALE_FACTOR, 5.0 / SCALE_FACTOR);

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

        // Give the camera the security ecology a real level links it to, so
        // the full alarm chain is exercisable here: identifying the player
        // raises the station alarm for the ecology's authored alert recovery,
        // and standing security down resets the ecology.
        let ecology_entity = core
            .create_entity_with_position(
                asset_cache,
                ECOLOGY_TEMPLATE_ID,
                CAMERA_START_POS,
                Quaternion::from_angle_y(Deg(0.0)),
                Matrix4::from_translation(vec3(0.0, 0.0, 0.0)),
                CreateEntityOptions::default(),
            )
            .entity_id;
        // The gamesys ecology template carries only the script; levels author
        // the population profile per instance. These are medsci1's numbers,
        // including its 120 s alert recovery - the alarm's duration.
        core.world.add_component(
            ecology_entity,
            PropEcology {
                period_seconds: 15.0,
                min_count: [0, 0, 2],
                max_count: [0, 0, 2],
                recovery_seconds: [0.0, 0.0, 120.0],
                random_chance: [0, 0, 0],
            },
        );
        // Levels author this pair of switch links in both directions: the
        // camera alarms the ecology, and the ecology's reset clears the
        // camera again.
        let switch_link = |to_entity_id, to_template_id| Links {
            to_links: vec![ToLink {
                to_template_id,
                to_entity_id: Some(WrappedEntityId(to_entity_id)),
                link: Link::SwitchLink,
            }],
        };
        core.world.add_component(
            camera_entity,
            switch_link(ecology_entity, ECOLOGY_TEMPLATE_ID),
        );
        core.world.add_component(
            ecology_entity,
            switch_link(camera_entity, CAMERA_TEMPLATE_ID),
        );
        info!("Spawned debug security ecology entity {ecology_entity:?}");

        // A security computer, so standing the alarm down early is testable
        // here too.
        let console_entity = core
            .create_entity_with_position(
                asset_cache,
                SECURITY_COMPUTER_TEMPLATE_ID,
                CONSOLE_START_POS,
                Quaternion::from_angle_y(Deg(180.0)),
                Matrix4::from_translation(vec3(0.0, 1.0, 10.0)),
                CreateEntityOptions::default(),
            )
            .entity_id;
        info!("Spawned debug security computer entity {console_entity:?}");

        // Equip the player with the psi amp on the first update (same
        // pattern as `debug_psi`), so camera-perception powers are castable
        // in this scene.
        Box::new(HookedDebugScene::new(
            core,
            AutoEquipHooks::new(PSI_AMP_TEMPLATE_ID),
        ))
    }
}
