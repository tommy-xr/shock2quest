//! Debug scene for first-person weapon work (viewmodel framing + aim).
//!
//! A clean, controlled environment: the player stands on a floor facing a flat
//! wall a known distance straight ahead. Cycle weapons with `CycleWeapon`
//! (`B` on desktop / `POST /v1/input/action {"action":"CycleWeapon"}`) and fire;
//! shots land on the wall as hit-spangs, so it's easy to see whether a weapon's
//! barrel and its projectiles track the crosshair - without a real mission's
//! cluttered geometry hiding the viewmodel or swallowing the shot.

use cgmath::{Deg, Matrix4, Quaternion, Rotation3, vec3};
use engine::{
    assets::asset_cache::AssetCache,
    audio::AudioContext,
    scene::{SceneObject, color_material, cube},
};
use rapier3d::prelude::{ColliderBuilder, Isometry, SharedShape};
use shipyard::EntityId;

use crate::{
    GameOptions,
    mission::{GlobalContext, SpawnLocation},
};

use super::debug_common::{DebugScene, DebugSceneBuildOptions, DebugSceneBuilder};

/// Distance (world units) from the player to the test wall, straight ahead (-X,
/// the default-view forward).
const WALL_DISTANCE: f32 = 12.0;

fn cube_object(
    color: cgmath::Vector3<f32>,
    translation: cgmath::Vector3<f32>,
    scale: cgmath::Vector3<f32>,
) -> SceneObject {
    let mut object = SceneObject::new(color_material::create(color), Box::new(cube::create()));
    object.set_transform(
        Matrix4::from_translation(translation)
            * Matrix4::from_nonuniform_scale(scale.x, scale.y, scale.z),
    );
    object
}

pub fn create_debug_weapons_scene(
    global_context: &GlobalContext,
    game_options: &GameOptions,
    asset_cache: &mut AssetCache,
    audio_context: &mut AudioContext<EntityId, String>,
) -> DebugScene {
    // Visual floor (under the player) + a vertical wall straight ahead. The unit
    // cube spans [-0.5, 0.5], so a `cuboid` collider half-extent matches a
    // nonuniform scale of 2x the half-extent.
    let floor = cube_object(
        vec3(0.18, 0.18, 0.22),
        vec3(0.0, -0.5, 0.0),
        vec3(40.0, 1.0, 40.0),
    );
    let wall = cube_object(
        vec3(0.45, 0.45, 0.5),
        vec3(-WALL_DISTANCE, 5.0, 0.0),
        vec3(1.0, 30.0, 30.0),
    );

    // One static compound collider (floor + wall) in the ALL_COLLIDABLE group,
    // so the wall stops shots (hitscan checks WORLD) and shows hit-spangs.
    let collider = ColliderBuilder::compound(vec![
        (
            Isometry::translation(0.0, -0.5, 0.0),
            SharedShape::cuboid(20.0, 0.5, 20.0),
        ),
        (
            Isometry::translation(-WALL_DISTANCE, 5.0, 0.0),
            SharedShape::cuboid(0.5, 15.0, 15.0),
        ),
    ])
    .build();

    // Spawn at the floor with identity yaw: the default view forward is -X, so
    // the wall sits dead ahead.
    let builder = DebugSceneBuilder::new("debug_weapons")
        .with_spawn_location(SpawnLocation::PositionRotation(
            vec3(0.0, 2.0, 0.0),
            Quaternion::from_angle_y(Deg(0.0)),
        ))
        .with_physics_geometry(collider)
        .add_scene_object(floor)
        .add_scene_object(wall);

    builder.build(DebugSceneBuildOptions {
        global_context,
        game_options,
        asset_cache,
        audio_context,
    })
}
