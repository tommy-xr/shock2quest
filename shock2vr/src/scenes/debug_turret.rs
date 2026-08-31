use cgmath::{Deg, Matrix4, Point3, Quaternion, Rotation3, point3, vec3};
use dark::SCALE_FACTOR;
use dark::properties::PropHackDiff;
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

/// A live hostile creature (the one the ragdoll and melee scenes spawn), stood
/// beside the player inside the turret's cone. Hacking the turret moves it onto
/// the player's team, and this is what it then has to shoot at.
const HOSTILE_TEMPLATE_ID: i32 = -397;
const HOSTILE_START_POS: Point3<f32> = point3(0.8, 1.0, 0.0);

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

        // Debug scenes instantiate a template's scripts and model, not its
        // authored property set, so the turret's hack terms are given here -
        // the gamesys numbers for `Turrets`.
        scene.core_mut().world.add_component(
            turret_entity,
            PropHackDiff {
                success_chance: 20,
                critical_chance: 5,
                cost: 5.0,
            },
        );

        let hostile_entity = scene
            .core_mut()
            .create_entity_with_position(
                asset_cache,
                HOSTILE_TEMPLATE_ID,
                HOSTILE_START_POS,
                Quaternion::from_angle_y(Deg(0.0)),
                Matrix4::from_translation(vec3(0.0, 1.0, 10.0)),
                CreateEntityOptions::default(),
            )
            .entity_id;

        info!("Spawned debug turret entity {turret_entity:?}, hostile {hostile_entity:?}");

        Box::new(scene)
    }
}
