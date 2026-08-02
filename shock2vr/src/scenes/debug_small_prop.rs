//! Debug scene for exercising `/v1/player/move`'s bounded-hop step-up against
//! a small, climbable floor prop (crate/riser-sized obstacle) - see issue
//! #782. The player spawns a known distance from the near face of the prop,
//! facing it; automation can approach it via `/v1/player/move` in short hops
//! and confirm it climbs over instead of wedging (or via
//! `/v1/control/input` + `/v1/step` to compare against production
//! locomotion).

use cgmath::{Deg, Quaternion, Rotation3, vec3};
use engine::{assets::asset_cache::AssetCache, audio::AudioContext};
use rapier3d::prelude::{ColliderBuilder, Isometry, SharedShape};
use shipyard::EntityId;

use crate::{
    GameOptions,
    game_scene::GameScene,
    input_context::InputContext,
    mission::{GlobalContext, SpawnLocation, mission_core::MissionCore},
    physics::CollisionGroup,
    scenes::debug_common::{
        DebugSceneBuildOptions, DebugSceneBuilder, DebugSceneHooks, HookedDebugScene,
    },
    time::Time,
};

/// Height (world units) of the prop - a typical walkable stair riser/crate,
/// matching the height `validated_move_steps_up_stairs_but_not_walls` and
/// `validated_move_climbs_a_riser_via_short_requests_without_wedging`
/// (shock2vr/src/physics/mod.rs) already prove climbable with a long hop.
pub const PROP_HEIGHT: f32 = 0.6;

/// Distance (world units) from the player spawn to the near face of the
/// prop, straight ahead (-X, the default view forward).
pub const PROP_DISTANCE: f32 = 6.0;

#[derive(Default)]
struct SmallPropHooks;

impl SmallPropHooks {
    fn initialize(&mut self, core: &mut MissionCore) {
        // A separate static body (not baked into the level's compound
        // collider), matching how a real in-mission crate is its own
        // physics body rather than part of the level mesh.
        let center_x = -PROP_DISTANCE - 2.0;
        let isometry = Isometry::translation(center_x, PROP_HEIGHT / 2.0, 0.0);
        let handle = core.physics.create_static_body(isometry, None);
        let shape = SharedShape::cuboid(2.0, PROP_HEIGHT / 2.0, 2.0);
        core.physics
            .attach_collider(handle, shape, 1.0, CollisionGroup::entity());
    }
}

impl DebugSceneHooks for SmallPropHooks {
    fn before_update(
        &mut self,
        _core: &mut MissionCore,
        _time: &Time,
        _input_context: &InputContext,
        _asset_cache: &mut AssetCache,
        _game_options: &GameOptions,
    ) {
    }
}

pub fn create_debug_small_prop_scene(
    global_context: &GlobalContext,
    game_options: &GameOptions,
    asset_cache: &mut AssetCache,
    audio_context: &mut AudioContext<EntityId, String>,
) -> Box<dyn GameScene> {
    // Floor only - top at y=0. The prop is added separately below.
    let floor_collider = ColliderBuilder::new(SharedShape::cuboid(20.0, 0.5, 20.0))
        .position(Isometry::translation(0.0, -0.5, 0.0))
        .build();

    let builder = DebugSceneBuilder::new("debug_small_prop")
        .with_spawn_location(SpawnLocation::PositionRotation(
            vec3(0.0, 2.0, 0.0),
            Quaternion::from_angle_y(Deg(0.0)),
        ))
        .with_physics_geometry(floor_collider);

    let build_options = DebugSceneBuildOptions {
        global_context,
        game_options,
        asset_cache,
        audio_context,
    };

    let mut core = builder.build_core(build_options);
    let mut hooks = SmallPropHooks;
    hooks.initialize(&mut core);
    Box::new(HookedDebugScene::new(core, hooks))
}
