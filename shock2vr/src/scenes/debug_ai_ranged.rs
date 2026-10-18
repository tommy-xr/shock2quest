//! Ranged AI shooters: a shotgun hybrid, a midwife, a security droid and a
//! slug turret stand in a row ahead of the player and open fire on sight.
//!
//! Each fires through the creature launch path (`AIProjectile`), so this is
//! the controlled place to watch what an AI shot looks like - flash, exhaust,
//! projectile - without staging a mission. Turn on the `cheat` dev param to
//! watch for longer than the player survives.

use cgmath::{Deg, Matrix4, Point3, Quaternion, Rotation3, SquareMatrix, vec3};
use engine::{assets::asset_cache::AssetCache, audio::AudioContext};
use shipyard::EntityId;

use crate::{
    GameOptions,
    game_scene::GameScene,
    mission::{GlobalContext, SpawnLocation, entity_creator::CreateEntityOptions},
    scenes::debug_common::{DebugSceneBuildOptions, DebugSceneBuilder, boxes_to_geometry},
};

/// (template, lane z): OG-Shotgun, Midwife, Security.
const SHOOTERS: [(i32, f32); 3] = [(-175, -4.0), (-179, 0.0), (-172, 4.0)];

/// Slug Turret, beside the row. Turret models face their -X, so it turns
/// 180 degrees to face the player; its narrow sight cone must reach them.
const SLUG_TURRET: (i32, f32) = (-369, 7.0);

/// Distance ahead (along -X, the identity spawn's forward) of the shooters:
/// outside every one's minimum range, so each fires before closing in.
const SHOOTER_DISTANCE: f32 = 14.0;

pub fn create_debug_ai_ranged_scene(
    global_context: &GlobalContext,
    game_options: &GameOptions,
    asset_cache: &mut AssetCache,
    audio_context: &mut AudioContext<EntityId, String>,
) -> Box<dyn GameScene> {
    let (objects, collider) = boxes_to_geometry(&[
        // floor
        (
            vec3(0.18, 0.18, 0.22),
            vec3(-10.0, -0.5, 0.0),
            vec3(40.0, 1.0, 40.0),
        ),
        // back wall, behind the shooters
        (
            vec3(0.45, 0.45, 0.5),
            vec3(-SHOOTER_DISTANCE - 4.0, 2.0, 0.0),
            vec3(1.0, 4.0, 14.0),
        ),
    ]);
    let mut builder = DebugSceneBuilder::new("debug_ai_ranged")
        .with_spawn_location(SpawnLocation::PositionRotation(
            vec3(0.0, 2.0, 0.0),
            Quaternion::from_angle_y(Deg(0.0)),
        ))
        .with_physics_geometry(collider);
    for object in objects {
        builder = builder.add_scene_object(object);
    }
    let mut scene = builder.build(DebugSceneBuildOptions {
        global_context,
        game_options,
        asset_cache,
        audio_context,
    });
    for (template_id, z) in SHOOTERS {
        scene.core_mut().create_entity_with_position(
            asset_cache,
            template_id,
            Point3::new(-SHOOTER_DISTANCE, 1.0, z),
            // Face the player (+X).
            Quaternion::from_angle_y(Deg(90.0)),
            Matrix4::identity(),
            CreateEntityOptions::default(),
        );
    }
    let (template_id, z) = SLUG_TURRET;
    scene.core_mut().create_entity_with_position(
        asset_cache,
        template_id,
        Point3::new(-SHOOTER_DISTANCE, 1.0, z),
        Quaternion::from_angle_y(Deg(180.0)),
        Matrix4::identity(),
        CreateEntityOptions::default(),
    );
    println!(
        "[debug_ai_ranged] A shotgun hybrid, a midwife, a security droid and a slug turret stand \
         {SHOOTER_DISTANCE} units ahead and fire on sight. Set the `cheat` dev param to survive."
    );
    Box::new(scene)
}
