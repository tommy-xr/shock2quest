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

use std::f32::consts::FRAC_1_SQRT_2;

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

/// The repro lanes reach this far along +z; the floor covers them.
const FLOOR_MAX_Z: f32 = 110.0;

type SceneBox = (Vector3<f32>, Vector3<f32>, Vector3<f32>);

/// A box in a repro lane's frame: `d` is the distance in front of the
/// ladder wall (`x - STATION_FACE_X`, positive toward the climber), `w` the
/// offset across the lane (`z - lane`), `y` the height. Ranges are min..max.
fn lane_box(color: Vector3<f32>, lane: f32, d: [f32; 2], y: [f32; 2], w: [f32; 2]) -> SceneBox {
    let min = vec3(STATION_FACE_X + d[0], y[0], lane + w[0]);
    let max = vec3(STATION_FACE_X + d[1], y[1], lane + w[1]);
    (color, (min + max) / 2.0, max - min)
}

/// A ladder in a repro lane, centered `d` in front of the wall at height `y`.
fn lane_ladder(template_id: i32, lane: f32, d: f32, y: f32, w: f32) -> Effect {
    spawn_ladder(template_id, Point3::new(STATION_FACE_X + d, y, lane + w))
}

// Repro stations: each reproduces a ladder bug seen in a shipped mission,
// with the mission's heights measured by raycast and re-based so the lowest
// floor is y = 0. Mission "into the ladder" maps to -X here.

/// rick1 Ladder 530/532: stacked 16' ladders (mission 30.8 -> 43.6 over a
/// 32.0 shaft floor) run 1.6 above the 42.0 ceiling. Behind the climber is a
/// 38.8 corridor floor; behind the ladder wall is a 41.6 roof, open above.
pub const CAPPED_Z: f32 = 36.0;
/// Thickness of a slab standing in for a one-sided mission face.
const THIN: f32 = 0.02;
const CAPPED_CEILING: f32 = 10.0;
const CAPPED_UPPER_FLOOR: f32 = 6.8;
const CAPPED_PIT_DEPTH: f32 = 2.2;
const CAPPED_ROOF: f32 = 9.6;
/// Ladder 532 authors a 12.8-tall PhysDims box offset 6.4 down, so its
/// climbable column ends at 40.4 - 1.6 below the ceiling, not at its model top.
const CAPPED_COLUMN_TOP: f32 = 8.4;

const WALL_COLOR: Vector3<f32> = vec3(0.32, 0.36, 0.40);
const FLOOR_COLOR: Vector3<f32> = vec3(0.30, 0.42, 0.34);
const CEILING_COLOR: Vector3<f32> = vec3(0.42, 0.34, 0.30);

/// A walkable face whose top is at `y`.
fn floor_face(lane: f32, d: [f32; 2], y: f32, w: [f32; 2]) -> SceneBox {
    lane_box(FLOOR_COLOR, lane, d, [y - THIN, y], w)
}

/// A ceiling face whose underside is at `y`.
fn ceiling_face(lane: f32, d: [f32; 2], y: f32, w: [f32; 2]) -> SceneBox {
    lane_box(CEILING_COLOR, lane, d, [y, y + THIN], w)
}

/// A wall face across the lane whose climber-side surface is at `d`.
fn wall_face(lane: f32, d: f32, y: [f32; 2], w: [f32; 2]) -> SceneBox {
    lane_box(WALL_COLOR, lane, [d - THIN, d], y, w)
}

/// A wall face along the lane at `w`, spanning `d`.
fn side_face(lane: f32, d: [f32; 2], y: [f32; 2], w: f32) -> SceneBox {
    lane_box(WALL_COLOR, lane, d, y, [w - THIN / 2.0, w + THIN / 2.0])
}

fn repro_boxes() -> Vec<SceneBox> {
    let mut boxes = capped_boxes();
    boxes.extend(setback_boxes());
    boxes.extend(recess_boxes());
    boxes.extend(through_boxes());
    boxes.extend(midmount_boxes());
    boxes.extend(jump_grab_boxes());
    boxes
}

fn capped_boxes() -> Vec<SceneBox> {
    let c = CAPPED_Z;
    let wall = WALL_COLOR;
    let floor = FLOOR_COLOR;
    let ceiling = CEILING_COLOR;
    vec![
        // Corridor floor behind the climber, wrapping the 2-wide pit.
        lane_box(
            floor,
            c,
            [CAPPED_PIT_DEPTH, 7.8],
            [0.0, CAPPED_UPPER_FLOOR],
            [-4.0, 4.0],
        ),
        lane_box(
            floor,
            c,
            [0.0, CAPPED_PIT_DEPTH],
            [0.0, CAPPED_UPPER_FLOOR],
            [-4.0, -1.0],
        ),
        lane_box(
            floor,
            c,
            [0.0, CAPPED_PIT_DEPTH],
            [0.0, CAPPED_UPPER_FLOOR],
            [1.0, 4.0],
        ),
        // Ceiling over pit and corridor, with a 0.8 lintel at the wall. The
        // mission's ceiling is a single face with nothing above it, so the
        // slab is thin: a thick one would hide the top-out probe's path.
        lane_box(
            ceiling,
            c,
            [0.0, 7.8],
            [CAPPED_CEILING, CAPPED_CEILING + THIN],
            [-4.0, 4.0],
        ),
        lane_box(
            wall,
            c,
            [0.0, 0.2],
            [CAPPED_CEILING - 0.8, CAPPED_CEILING],
            [-4.0, 4.0],
        ),
        // The ladder wall, solid back to the roof.
        lane_box(wall, c, [-4.0, 0.0], [0.0, CAPPED_ROOF], [-4.0, 4.0]),
    ]
}

/// rick1 Ladder 488: a 16' ladder (mission 29.48 -> 35.88, floor 29.6) in
/// a pit under the y38 deck. The pit ceiling (37.2) is the deck slab's
/// underside; the deck (38.0) runs back over the climber to the wall the
/// ladder stands 0.51 in front of, beyond which is an open shaft. A pipe
/// entity over the deck leaves 1.4 headroom, so only a crouched climber fits.
pub const SETBACK_Z: f32 = 48.0;
const SETBACK_LADDER_D: f32 = 0.51;
const SETBACK_PIT_CEILING: f32 = 7.6;
const SETBACK_DECK: f32 = 8.4;
const SETBACK_DECK_CEILING: f32 = 11.6;
const SETBACK_PIT_DEPTH: f32 = 3.2;
const SETBACK_DECK_DEPTH: f32 = 4.3;
const SETBACK_PIT_HALF_WIDTH: f32 = 1.2;
/// `Pipe 24x4`, the entity over rick1's deck.
const PIPE_24X4: i32 = -1092;
/// `Rick Conduit`: 3.2-long conduit entities run along the wall behind
/// ladder 488 at 34.98..36.22, protruding 0.29 toward the climber.
const RICK_CONDUIT: i32 = -94;

fn setback_boxes() -> Vec<SceneBox> {
    let z = SETBACK_Z;
    let pit = [-SETBACK_PIT_HALF_WIDTH, SETBACK_PIT_HALF_WIDTH];
    let lane = [-4.0, 4.0];
    vec![
        // Wall behind the ladder: pit side and deck side, with the open slab
        // edge between them (the mission's slab has no face toward the shaft).
        wall_face(z, 0.0, [0.0, SETBACK_PIT_CEILING], lane),
        wall_face(z, 0.0, [SETBACK_DECK, SETBACK_DECK_CEILING], lane),
        // The 32.8..34.0 ledge between ladder and wall.
        lane_box(FLOOR_COLOR, z, [0.0, 0.4], [3.2, 4.4], pit),
        // Pit: back wall and sides.
        lane_box(
            WALL_COLOR,
            z,
            [SETBACK_PIT_DEPTH, SETBACK_PIT_DEPTH + THIN],
            [0.0, SETBACK_PIT_CEILING],
            pit,
        ),
        side_face(
            z,
            [0.0, SETBACK_PIT_DEPTH],
            [0.0, SETBACK_PIT_CEILING],
            pit[0],
        ),
        side_face(
            z,
            [0.0, SETBACK_PIT_DEPTH],
            [0.0, SETBACK_PIT_CEILING],
            pit[1],
        ),
        // Deck slab: underside over the pit, top over pit and climber.
        ceiling_face(z, [0.0, SETBACK_DECK_DEPTH], SETBACK_PIT_CEILING, lane),
        floor_face(z, [0.0, SETBACK_DECK_DEPTH], SETBACK_DECK, lane),
        ceiling_face(z, [0.0, SETBACK_DECK_DEPTH], SETBACK_DECK_CEILING, lane),
        lane_box(
            WALL_COLOR,
            z,
            [SETBACK_DECK_DEPTH, SETBACK_DECK_DEPTH + THIN],
            [SETBACK_DECK, SETBACK_DECK_CEILING],
            lane,
        ),
    ]
}

/// rick1 Ladder 499: a 16' ladder (mission 33.88 -> 40.28) at the far edge
/// of a 2.4 x 2.36 hole in the y38 deck, climbed from the y34 floor below.
/// Past the ladder is a 0.44 deck strip, then a wall over an open shaft; the
/// deck wraps the hole on both sides. Ceiling 41.2, sloping down 45 degrees
/// behind the climber.
pub const RECESS_Z: f32 = 60.0;
const RECESS_LADDER_D: f32 = 0.49;
const RECESS_DECK_UNDERSIDE: f32 = 3.2;
const RECESS_DECK: f32 = 4.0;
const RECESS_CEILING: f32 = 7.2;
/// The hole in the deck: from the strip edge to its far edge, and across.
const RECESS_HOLE_D: [f32; 2] = [0.44, 2.8];
const RECESS_HOLE_W: [f32; 2] = [-1.21, 1.19];
/// `Curved Pipe 1' Diameter`, one sits on the strip beside ladder 499.
const CURVED_PIPE_1FT: i32 = -1148;

fn recess_boxes() -> Vec<SceneBox> {
    let z = RECESS_Z;
    let lane = [-4.0, 4.0];
    let hole_d = RECESS_HOLE_D;
    let hole_w = RECESS_HOLE_W;
    let mut boxes = vec![
        // Shaft wall past the ladder, open at the deck slab's edge.
        wall_face(z, 0.0, [0.0, RECESS_DECK_UNDERSIDE], lane),
        wall_face(z, 0.0, [RECESS_DECK, RECESS_CEILING], lane),
        // Far wall of the y34 room under the deck.
        lane_box(
            WALL_COLOR,
            z,
            [3.2, 3.2 + THIN],
            [0.0, RECESS_DECK_UNDERSIDE],
            lane,
        ),
        // Deck faces around the hole: the strip, the far side, both flanks.
        floor_face(z, [0.0, hole_d[0]], RECESS_DECK, lane),
        floor_face(z, [hole_d[1], 6.0], RECESS_DECK, lane),
        floor_face(z, hole_d, RECESS_DECK, [lane[0], hole_w[0]]),
        floor_face(z, hole_d, RECESS_DECK, [hole_w[1], lane[1]]),
        ceiling_face(z, [0.0, hole_d[0]], RECESS_DECK_UNDERSIDE, lane),
        ceiling_face(z, [hole_d[1], 6.0], RECESS_DECK_UNDERSIDE, lane),
        ceiling_face(z, hole_d, RECESS_DECK_UNDERSIDE, [lane[0], hole_w[0]]),
        ceiling_face(z, hole_d, RECESS_DECK_UNDERSIDE, [hole_w[1], lane[1]]),
        // The slab's edges facing into the hole.
        lane_box(
            WALL_COLOR,
            z,
            [hole_d[0] - THIN, hole_d[0]],
            [RECESS_DECK_UNDERSIDE, RECESS_DECK],
            hole_w,
        ),
        lane_box(
            WALL_COLOR,
            z,
            [hole_d[1], hole_d[1] + THIN],
            [RECESS_DECK_UNDERSIDE, RECESS_DECK],
            hole_w,
        ),
        side_face(z, hole_d, [RECESS_DECK_UNDERSIDE, RECESS_DECK], hole_w[0]),
        side_face(z, hole_d, [RECESS_DECK_UNDERSIDE, RECESS_DECK], hole_w[1]),
        ceiling_face(z, [0.0, 1.6], RECESS_CEILING, lane),
    ];
    // The ceiling behind the climber slopes down 45 degrees to the deck.
    for step in 0..8 {
        let d = 1.6 + 0.4 * step as f32;
        boxes.push(ceiling_face(
            z,
            [d, d + 0.4],
            RECESS_CEILING - 0.4 * (step + 1) as f32,
            lane,
        ));
    }
    boxes
}

/// eng1 Ladder 317: an eng1 `Ladder 16'` from the -19.8 corridor floor to
/// its -13.4 ceiling, on a 0.4-thick wall. The -16.6 floor of the room
/// behind that wall (3.2 above the corridor, ceiling -9.8) is the only exit.
pub const THROUGH_Z: f32 = 72.0;
const THROUGH_CEILING: f32 = 6.4;
const THROUGH_WALL: f32 = 0.4;
const THROUGH_UPPER_FLOOR: f32 = 3.2;
const THROUGH_UPPER_CEILING: f32 = 10.0;
/// Corridor width across the lane (mission x 1.6..3.0), and the 3.2 shelf
/// beside it (x 3.0..4.6, slab 2.8..3.2).
const THROUGH_CORRIDOR_W: [f32; 2] = [-0.77, 0.63];
const THROUGH_SHELF_W: f32 = 2.23;
/// eng1's `Ladder 16'` (model `ladder`, 1.65 wide).
const ENG_LADDER_16: i32 = -784;

fn through_boxes() -> Vec<SceneBox> {
    let z = THROUGH_Z;
    let lane = [-4.0, 4.0];
    let corridor = [THROUGH_CORRIDOR_W[0], THROUGH_SHELF_W];
    let back = -THROUGH_WALL;
    vec![
        // The wall: its two faces, as in the mission.
        wall_face(z, 0.0, [0.0, THROUGH_UPPER_CEILING], lane),
        lane_box(
            WALL_COLOR,
            z,
            [back, back + THIN],
            [THROUGH_UPPER_FLOOR, THROUGH_UPPER_CEILING],
            lane,
        ),
        // Corridor: ceiling (taller past 1.67), sides, shelf, far end.
        ceiling_face(z, [0.0, 1.67], THROUGH_CEILING, corridor),
        ceiling_face(z, [1.67, 5.6], 9.6, corridor),
        side_face(z, [0.0, 5.6], [0.0, 9.6], THROUGH_CORRIDOR_W[0]),
        side_face(z, [0.0, 5.6], [0.0, 9.6], THROUGH_SHELF_W),
        lane_box(
            FLOOR_COLOR,
            z,
            [0.0, 5.6],
            [2.8, THROUGH_UPPER_FLOOR],
            [THROUGH_CORRIDOR_W[1], THROUGH_SHELF_W],
        ),
        lane_box(WALL_COLOR, z, [5.6, 5.6 + THIN], [0.0, 9.6], corridor),
        // The room behind the wall.
        floor_face(z, [-2.4, back], THROUGH_UPPER_FLOOR, lane),
        ceiling_face(z, [-2.4, back], THROUGH_UPPER_CEILING, lane),
        wall_face(z, -2.4, [0.0, THROUGH_UPPER_CEILING], lane),
    ]
}

/// hydro2 Ladder 551 (#802): eleven stacked rungs (mission -2.4 -> 6.4, the
/// ceiling) 0.2 in front of a wall, rising through a 1.96 x 1.8 hole in an
/// office floor 4.4 above the lower floor. The climber walks off the office
/// floor onto the middle of the ladder.
pub const MIDMOUNT_Z: f32 = 84.0;
const MIDMOUNT_RUNG_D: f32 = 0.2;
const MIDMOUNT_SLAB_UNDERSIDE: f32 = 4.0;
const MIDMOUNT_OFFICE: f32 = 4.4;
const MIDMOUNT_CEILING: f32 = 8.4;
const MIDMOUNT_HOLE_D: f32 = 1.8;
const MIDMOUNT_HOLE_HALF_WIDTH: f32 = 0.98;

fn midmount_boxes() -> Vec<SceneBox> {
    let z = MIDMOUNT_Z;
    let lane = [-4.0, 4.0];
    let (hw, hd) = (MIDMOUNT_HOLE_HALF_WIDTH, MIDMOUNT_HOLE_D);
    let mut boxes = vec![
        wall_face(z, 0.0, [0.0, MIDMOUNT_CEILING], lane),
        ceiling_face(z, [0.0, 6.0], MIDMOUNT_CEILING, lane),
        // Hole edges, from the office floor down to the slab underside.
        lane_box(
            WALL_COLOR,
            z,
            [hd, hd + THIN],
            [MIDMOUNT_SLAB_UNDERSIDE, MIDMOUNT_OFFICE],
            [-hw, hw],
        ),
        side_face(
            z,
            [0.0, hd],
            [MIDMOUNT_SLAB_UNDERSIDE, MIDMOUNT_OFFICE],
            -hw,
        ),
        side_face(z, [0.0, hd], [MIDMOUNT_SLAB_UNDERSIDE, MIDMOUNT_OFFICE], hw),
    ];
    // The office floor (and the lower room's ceiling) around the hole.
    for (d, w) in [
        ([hd, 6.0], lane),
        ([0.0, hd], [lane[0], -hw]),
        ([0.0, hd], [hw, lane[1]]),
    ] {
        boxes.push(floor_face(z, d, MIDMOUNT_OFFICE, w));
        boxes.push(ceiling_face(z, d, MIDMOUNT_SLAB_UNDERSIDE, w));
    }
    boxes
}

/// rick2 Ladder 210/211/212 (#907): a ladder column 0.19 in front of the
/// wall of a pipe shaft, from below the floor (mission 76.1, floor 78.4) to
/// 0.5 above the 94.8 ceiling. The climber jumps to it from the top of a pipe
/// (93.2) 1.4 west and 0.4 south of it.
pub const JUMP_GRAB_Z: f32 = 96.0;
const JUMP_GRAB_CEILING: f32 = 16.4;
const JUMP_GRAB_LADDER_D: f32 = 0.19;
const JUMP_GRAB_LADDER_BOTTOM: f32 = -2.3;
/// The pipe the jump starts from: its top, and its footprint.
pub const JUMP_GRAB_PLATFORM_TOP: f32 = 14.8;
const JUMP_GRAB_PLATFORM_D: [f32; 2] = [0.57, 1.77];
const JUMP_GRAB_PLATFORM_W: [f32; 2] = [-3.4, -1.0];
/// `RickPipe_1x16`: two stand vertically between the pipe and the ladder.
const RICK_PIPE_1X16: i32 = -2979;

fn jump_grab_boxes() -> Vec<SceneBox> {
    let z = JUMP_GRAB_Z;
    let shaft = [-3.37, 1.93];
    vec![
        wall_face(z, 0.0, [0.0, JUMP_GRAB_CEILING], shaft),
        wall_face(z, 7.7 + THIN, [0.0, JUMP_GRAB_CEILING], shaft),
        side_face(z, [0.0, 7.7], [0.0, JUMP_GRAB_CEILING], shaft[0]),
        side_face(z, [0.0, 7.7], [0.0, JUMP_GRAB_CEILING], shaft[1]),
        ceiling_face(z, [0.0, 7.7], JUMP_GRAB_CEILING, shaft),
        lane_box(
            FLOOR_COLOR,
            z,
            JUMP_GRAB_PLATFORM_D,
            [JUMP_GRAB_PLATFORM_TOP - 2.4, JUMP_GRAB_PLATFORM_TOP],
            JUMP_GRAB_PLATFORM_W,
        ),
    ]
}

fn repro_ladders() -> Vec<Effect> {
    let d = 0.1;
    // Mission +Z (into the ladder) is -X here: a -90 degree yaw.
    let mission_yaw = Quaternion::from_angle_y(Deg(-90.0));
    let mut effects = vec![
        lane_ladder(
            LADDER_16,
            SETBACK_Z,
            SETBACK_LADDER_D,
            LADDER_16_HEIGHT / 2.0 - 0.12,
            0.0,
        ),
        // Mission Pipe 633: centered 40.2, 0.8 behind the ladder plane.
        spawn_at_oriented(
            PIPE_24X4,
            Point3::new(STATION_FACE_X + 1.18, 10.6, SETBACK_Z - 1.63),
            mission_yaw * Quaternion::new(0.5, -0.5, 0.5, 0.5),
        ),
    ];
    for w in [-3.68, -0.48, 2.72] {
        effects.push(spawn_at_oriented(
            RICK_CONDUIT,
            Point3::new(STATION_FACE_X + 0.1, 6.0, SETBACK_Z + w),
            mission_yaw * Quaternion::new(0.0, 0.0, FRAC_1_SQRT_2, FRAC_1_SQRT_2),
        ));
    }
    for index in 0..3 {
        effects.push(lane_ladder(
            LADDER_16,
            JUMP_GRAB_Z,
            JUMP_GRAB_LADDER_D,
            JUMP_GRAB_LADDER_BOTTOM + LADDER_16_HEIGHT * (index as f32 + 0.5),
            0.0,
        ));
    }
    for w in [-1.53, -0.93] {
        effects.push(spawn_at_oriented(
            RICK_PIPE_1X16,
            Point3::new(STATION_FACE_X + 0.31, 14.0, JUMP_GRAB_Z + w),
            mission_yaw * Quaternion::new(FRAC_1_SQRT_2, -FRAC_1_SQRT_2, 0.0, 0.0),
        ));
    }
    for index in 0..RUNG_COUNT {
        effects.push(lane_ladder(
            LADDER_RUNG,
            MIDMOUNT_Z,
            MIDMOUNT_RUNG_D,
            RUNG_SPACING * index as f32,
            0.0,
        ));
    }
    effects.extend([
        lane_ladder(ENG_LADDER_16, THROUGH_Z, 0.07, 3.2, 0.0),
        lane_ladder(
            LADDER_16,
            RECESS_Z,
            RECESS_LADDER_D,
            LADDER_16_HEIGHT / 2.0 - 0.12,
            0.0,
        ),
        // Mission Curved Pipe 735 on the strip, 1.78 east of the ladder
        // (this lane mirrors the mission: east is -z).
        spawn_at_oriented(
            CURVED_PIPE_1FT,
            Point3::new(STATION_FACE_X + 0.42, 4.84, RECESS_Z - 1.78),
            Quaternion::from_angle_y(Deg(90.0))
                * Quaternion::new(0.0, 0.0, FRAC_1_SQRT_2, FRAC_1_SQRT_2),
        ),
        lane_ladder(LADDER_16, CAPPED_Z, d, -1.2 + LADDER_16_HEIGHT / 2.0, 0.0),
        lane_ladder(
            LADDER_16,
            CAPPED_Z,
            d,
            CAPPED_COLUMN_TOP - LADDER_16_HEIGHT / 2.0,
            0.0,
        ),
    ]);
    effects
}

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
        // floor, extended along +z under the repro lanes
        (
            vec3(0.18, 0.18, 0.22),
            vec3(0.0, -0.5, FLOOR_MAX_Z / 2.0 - 15.0),
            vec3(60.0, 1.0, FLOOR_MAX_Z + 30.0),
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
    boxes.extend(repro_boxes());

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
        effects.extend(repro_ladders());
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
