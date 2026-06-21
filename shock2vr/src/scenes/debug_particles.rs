//! Debug scene for inspecting particle-group colors.
//!
//! Builds a row of particle emitters with authored test params, each using a
//! different **master-palette color index**, so the palette -> RGB -> `inColor`
//! color path can be eyeballed. (Bare gamesys *templates* carry zeroed runtime
//! state, so we set the params directly rather than spawning a template; placed
//! `.mis` instances do carry authored params and render in real missions.)

use cgmath::{Matrix4, Point3, Vector3, point3, vec3};
use dark::{
    SCALE_FACTOR,
    properties::{PropParticleGroup, PropParticleLaunchInfo},
};
use engine::{assets::asset_cache::AssetCache, audio::AudioContext};
use shipyard::EntityId;

use crate::{
    GameOptions,
    game_scene::GameScene,
    mission::{GlobalContext, SpawnLocation, mission_core::MissionCore},
    runtime_props::RuntimePropTransform,
    scenes::debug_common::{
        DebugSceneBuildOptions, DebugSceneBuilder, DebugSceneHooks, HookedDebugScene,
    },
};

/// Showcased emitters: `(label, master-palette color index)`. Indices chosen as
/// vivid, visually distinct colors in SHOCKPAL.PCX.
const SHOWCASE: &[(&str, u8)] = &[
    ("red", 20),
    ("orange", 30),
    ("yellow", 35),
    ("green", 67),
    ("cyan", 91),
    ("blue", 200),
];

/// Spacing between adjacent emitters along Z (left-right across the default view).
const SPACING: f32 = 1.3;
/// How far ahead of the camera (which looks toward -X) the emitters sit.
const DISTANCE_X: f32 = -6.0;

pub struct DebugParticlesScene;

impl DebugParticlesScene {
    pub fn new(
        global_context: &GlobalContext,
        game_options: &GameOptions,
        asset_cache: &mut AssetCache,
        audio_context: &mut AudioContext<EntityId, String>,
    ) -> Box<dyn GameScene> {
        let builder = DebugSceneBuilder::new("debug_particles")
            .with_default_floor()
            .with_spawn_location(SpawnLocation::PositionRotation(
                vec3(0.0, 5.0 / SCALE_FACTOR, 0.0),
                cgmath::Quaternion::new(1.0, 0.0, 0.0, 0.0),
            ));

        let build_options = DebugSceneBuildOptions {
            global_context,
            game_options,
            asset_cache,
            audio_context,
        };

        let core = builder.build_core(build_options);
        Box::new(HookedDebugScene::new(core, ParticleHooks::new()))
    }
}

/// World position for the `i`th emitter, centered around Z = 0.
fn showcase_position(i: usize, count: usize) -> Point3<f32> {
    let centered = i as f32 - (count as f32 - 1.0) / 2.0;
    point3(DISTANCE_X, 5.0 / SCALE_FACTOR, centered * SPACING)
}

/// A small upward-drifting plume emitter tinted by `color_index`.
fn make_particle_group(color_index: u8) -> PropParticleGroup {
    PropParticleGroup {
        render_type: 4, // PRT_SINGLE_COLOR_DISK
        motion_type: 0,
        animation_type: 0,
        num: 40,
        velocity: Vector3::new(0.0, 0.0, 0.0),
        // gravity is applied as acceleration (Dark units, /SCALE_FACTOR at use).
        gravity: vec3(0.0, -1.0, 0.0),
        r: color_index, // primary color = palette index
        g: 0,
        b: 0,
        a: 200, // alpha (of 255)
        spin: Vector3::new(0.0, 0.0, 0.0),
        is_active: true,
        is_worldspace: false,
        size: 0.6,
        scale_vel: 0.0,
        prev_loc: Vector3::new(0.0, 0.0, 0.0),
        bbox_min: Vector3::new(0.0, 0.0, 0.0),
        bbox_max: Vector3::new(0.0, 0.0, 0.0),
        radius: 1.0,
        launch_time: 0.05,
        fade_time: 0.4,
        model_name: String::new(),
    }
}

/// A small launch volume with a gentle upward velocity spread (Dark units).
fn make_launch_info() -> PropParticleLaunchInfo {
    PropParticleLaunchInfo {
        launch_type: 0,
        loc_min: vec3(-0.3, -0.1, -0.3),
        loc_max: vec3(0.3, 0.1, 0.3),
        vel_min: vec3(-1.0, 3.0, -1.0),
        vel_max: vec3(1.0, 4.5, 1.0),
        min_radius: 0.0,
        max_radius: 0.0,
        min_time: 1.5,
        max_time: 2.5,
    }
}

struct ParticleHooks {
    spawned: bool,
}

impl ParticleHooks {
    fn new() -> Self {
        let layout = SHOWCASE
            .iter()
            .enumerate()
            .map(|(i, (label, idx))| format!("  [{i}] {label} (palette index {idx})"))
            .collect::<Vec<_>>()
            .join("\n");
        println!("[debug_particles] palette-colored emitters, left-to-right:\n{layout}");
        Self { spawned: false }
    }

    fn spawn_emitters(&mut self, core: &mut MissionCore) {
        if self.spawned {
            return;
        }
        for (i, (_, color_index)) in SHOWCASE.iter().enumerate() {
            let position = showcase_position(i, SHOWCASE.len());
            let transform = Matrix4::from_translation(vec3(position.x, position.y, position.z));
            core.world.add_entity((
                RuntimePropTransform(transform),
                make_particle_group(*color_index),
                make_launch_info(),
            ));
        }
        self.spawned = true;
    }
}

impl DebugSceneHooks for ParticleHooks {
    fn before_update(
        &mut self,
        core: &mut MissionCore,
        _time: &crate::time::Time,
        _input_context: &crate::input_context::InputContext,
        _asset_cache: &mut AssetCache,
        _game_options: &GameOptions,
    ) {
        self.spawn_emitters(core);
    }
}
