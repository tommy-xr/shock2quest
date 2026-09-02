//! Debug scene for climbing: ladders and climbable blocks in the shapes the
//! shipped missions use, in one place, for flat and VR alike.
//!
//! Climbing tests borrow mission geometry (medsci1's cryo ladder, rick1's
//! opening deck, hydro2's Sector-C rung stack), so each case costs a mission
//! load and a re-staged pose whenever the geometry it leans on changes. This
//! scene is the controlled equivalent: one station per climbing shape, at
//! known coordinates, built from the same ladder templates the missions place
//! (so their `PropPhysAttr.climbable` flag and model-bounds colliders are the
//! production ones, not stand-ins).
//!
//! Stations sit at `x ≈ -7` (ahead of the spawn) on lanes along `z`:
//! - `z = 0`   ledge: a 16' ladder up a walkable block whose top is just
//!   below the ladder's cap - the rick1 top-out shape.
//! - `z = 8`   arch: a freestanding block with a 16' ladder on both faces -
//!   climb up, cross, climb down.
//! - `z = -8`  stack: eleven single `Rick Ladder` rungs at 0.8-unit spacing
//!   up a tall wall - the hydro2 Sector-C shape (separate entities, so a
//!   climb must carry across rung boundaries).
//! - `z = 16`  short: a freestanding 4' ladder, reachable on either face,
//!   with an exposed top and bottom.
//! - `z = 24`  mantle: a low block with no ladder, within jump-and-mantle
//!   reach - the earth.mis training ledge shape. In VR its lip is what a
//!   hand grabs.
//! - `z = -16` wall: a plain block, same size as the ledge's, with no ladder.
//!   The negative case.

use cgmath::{Deg, Point3, Quaternion, Rotation3, Vector3, vec3};
use engine::{assets::asset_cache::AssetCache, audio::AudioContext};
use rapier3d::prelude::{ColliderBuilder, Isometry, SharedShape};
use shipyard::EntityId;

use crate::{
    GameOptions,
    game_scene::GameScene,
    mission::{GlobalContext, SpawnLocation, mission_core::MissionCore},
    scenes::debug_common::{
        DebugSceneBuildOptions, DebugSceneBuilder, DebugSceneHooks, HookedDebugScene, cube_object,
        spawn_at_oriented,
    },
    scripts::Effect,
};

/// `Rick Ladder 16` / `Rick Ladder 4`: one-piece ladders, 16 and 4 SS2 feet
/// tall. `Rick Ladder` is the single rung the missions stack.
const LADDER_16: i32 = -2558;
const LADDER_4: i32 = -2556;
const LADDER_RUNG: i32 = -178;

/// Ladder heights in world units (SS2 feet / 2.5).
const LADDER_16_HEIGHT: f32 = 6.4;
const LADDER_4_HEIGHT: f32 = 1.6;
/// hydro2 stacks its rungs 0.8 units apart.
const RUNG_SPACING: f32 = 0.8;
const RUNG_COUNT: usize = 11;

/// Near face of every station block, ahead of the spawn along -X.
const STATION_FACE_X: f32 = -7.0;
/// Blocks are 4 units wide across their lane.
const BLOCK_WIDTH: f32 = 4.0;

const LEDGE_Z: f32 = 0.0;
const LEDGE_HEIGHT: f32 = 6.0;
const LEDGE_DEPTH: f32 = 6.0;

const ARCH_Z: f32 = 8.0;
const ARCH_DEPTH: f32 = 2.0;

const STACK_Z: f32 = -8.0;
const STACK_WALL_HEIGHT: f32 = 9.0;

const SHORT_Z: f32 = 16.0;

const MANTLE_Z: f32 = 24.0;
/// 7.5 ft: above step height, below the jump-plus-mantle reach.
const MANTLE_HEIGHT: f32 = 3.0;

const WALL_Z: f32 = -16.0;

/// The ladder models are authored with their rungs facing ±Z. Every station
/// here is approached along X, so the ladders turn a quarter to face the
/// player.
fn ladder_yaw() -> Quaternion<f32> {
    Quaternion::from_angle_y(Deg(90.0))
}

fn spawn_ladder(template_id: i32, position: Point3<f32>) -> Effect {
    spawn_at_oriented(template_id, position, ladder_yaw())
}

pub fn create_debug_ladder_scene(
    global_context: &GlobalContext,
    game_options: &GameOptions,
    asset_cache: &mut AssetCache,
    audio_context: &mut AudioContext<EntityId, String>,
) -> Box<dyn GameScene> {
    // A block whose near face is at STATION_FACE_X, `depth` long along -X.
    let block = |color: Vector3<f32>, z: f32, height: f32, depth: f32| {
        (
            color,
            vec3(STATION_FACE_X - depth / 2.0, height / 2.0, z),
            vec3(depth, height, BLOCK_WIDTH),
        )
    };
    // Every piece is a (color, center, size) box used for both the visual and
    // the collider so the two cannot drift.
    let mut boxes: Vec<(Vector3<f32>, Vector3<f32>, Vector3<f32>)> = vec![
        // floor
        (
            vec3(0.18, 0.18, 0.22),
            vec3(0.0, -0.5, 0.0),
            vec3(60.0, 1.0, 60.0),
        ),
        block(vec3(0.30, 0.40, 0.30), LEDGE_Z, LEDGE_HEIGHT, LEDGE_DEPTH),
        block(vec3(0.40, 0.30, 0.30), ARCH_Z, LADDER_16_HEIGHT, ARCH_DEPTH),
        block(
            vec3(0.30, 0.30, 0.45),
            STACK_Z,
            STACK_WALL_HEIGHT,
            LEDGE_DEPTH,
        ),
        block(vec3(0.45, 0.40, 0.30), MANTLE_Z, MANTLE_HEIGHT, LEDGE_DEPTH),
        block(vec3(0.35, 0.35, 0.35), WALL_Z, LEDGE_HEIGHT, LEDGE_DEPTH),
    ];

    let scene_objects = boxes
        .iter()
        .map(|(color, translation, scale)| cube_object(*color, *translation, *scale))
        .collect::<Vec<_>>();
    let collider = ColliderBuilder::compound(
        boxes
            .drain(..)
            .map(|(_, translation, scale)| {
                (
                    Isometry::translation(translation.x, translation.y, translation.z),
                    SharedShape::cuboid(scale.x / 2.0, scale.y / 2.0, scale.z / 2.0),
                )
            })
            .collect(),
    )
    .build();

    let mut builder = DebugSceneBuilder::new("debug_ladder")
        .with_spawn_location(SpawnLocation::PositionRotation(
            vec3(0.0, 2.0, 0.0),
            Quaternion::from_angle_y(Deg(0.0)),
        ))
        .with_physics_geometry(collider);
    for scene_object in scene_objects {
        builder = builder.add_scene_object(scene_object);
    }

    let core = builder.build_core(DebugSceneBuildOptions {
        global_context,
        game_options,
        asset_cache,
        audio_context,
    });

    println!(
        "[debug_ladder] Climbing stations ahead (-X), one per lane along z:\n\
         z=0 ledge (16' ladder, top-out onto the block), z=8 arch (ladder both\n\
         faces), z=-8 stack (11 stacked rungs), z=16 short 4' ladder, z=24 low\n\
         mantle block (no ladder), z=-16 plain wall (not climbable). Ladders are the shipped templates, so their\n\
         climbable flag and colliders are the production ones."
    );

    Box::new(HookedDebugScene::new(core, LadderHooks::default()))
}

#[derive(Default)]
struct LadderHooks {
    populated: bool,
}

impl DebugSceneHooks for LadderHooks {
    fn before_handle_effects(
        &mut self,
        core: &mut MissionCore,
        _effects: &mut Vec<Effect>,
        global_context: &GlobalContext,
        game_options: &GameOptions,
        asset_cache: &mut AssetCache,
        audio_context: &mut AudioContext<EntityId, String>,
    ) {
        if self.populated {
            return;
        }
        self.populated = true;

        // Ladders stand flush against their block, centered on their height
        // (the templates' origin is the model center).
        let flush_x = STATION_FACE_X + 0.1;
        let mut effects = vec![
            spawn_ladder(
                LADDER_16,
                Point3::new(flush_x, LADDER_16_HEIGHT / 2.0, LEDGE_Z),
            ),
            spawn_ladder(
                LADDER_16,
                Point3::new(flush_x, LADDER_16_HEIGHT / 2.0, ARCH_Z),
            ),
            spawn_ladder(
                LADDER_16,
                Point3::new(
                    STATION_FACE_X - ARCH_DEPTH - 0.1,
                    LADDER_16_HEIGHT / 2.0,
                    ARCH_Z,
                ),
            ),
            spawn_ladder(
                LADDER_4,
                Point3::new(STATION_FACE_X - 1.0, LADDER_4_HEIGHT / 2.0, SHORT_Z),
            ),
        ];
        for index in 0..RUNG_COUNT {
            effects.push(spawn_ladder(
                LADDER_RUNG,
                Point3::new(flush_x, RUNG_SPACING * (index as f32 + 0.5), STACK_Z),
            ));
        }

        core.handle_effects(
            effects,
            global_context,
            game_options,
            asset_cache,
            audio_context,
        );
    }
}
