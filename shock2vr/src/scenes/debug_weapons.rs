//! Debug scene for first-person weapon work (viewmodel framing, aim, reload).
//!
//! A clean, controlled environment: the player stands on a floor facing a flat
//! wall a known distance straight ahead. Cycle weapons with `DebugCycleWeapon`
//! (`B` on desktop / `POST /v1/input/action {"action":"DebugCycleWeapon"}`) and fire;
//! shots land on the wall as hit-spangs, so it's easy to see whether a weapon's
//! barrel and its projectiles track the crosshair - without a real mission's
//! cluttered geometry hiding the viewmodel or swallowing the shot.
//!
//! A bench on the player's left (out of the firing lane, so wall shots and
//! staged VR gestures are unaffected) carries the full player gun arsenal and
//! one box of every ammo type each gun takes, and the character sheet is maxed
//! on entry - so reload, ammo cycling, and the VR clip-insert gesture can all
//! be exercised without provisioning anything first.

use cgmath::{Deg, Point3, Quaternion, Rotation3, vec3};
use engine::{assets::asset_cache::AssetCache, audio::AudioContext};
use rapier3d::prelude::{ColliderBuilder, Isometry, SharedShape};
use shipyard::EntityId;

use crate::{
    GameOptions,
    game_scene::{DebugPlayerStatsRequest, DebugSkillLevelsRequest, DebuggableScene, GameScene},
    mission::{GlobalContext, SpawnLocation, mission_core::MissionCore},
    scenes::debug_common::{
        DebugSceneBuildOptions, DebugSceneBuilder, DebugSceneHooks, cube_object, spawn_at,
    },
    scripts::Effect,
    scripts::gui::{PSI_TIER_CAP, SKILL_CAP, STAT_CAP},
};

/// Distance (world units) from the player to the test wall, straight ahead (-X,
/// the default-view forward).
const WALL_DISTANCE: f32 = 12.0;

/// Every player-obtainable gun, in bench order - grouped
/// so each gun sits near its ammo row entry below.
const BENCH_GUNS: &[(i32, &str)] = &[
    (-17, "Pistol"),
    (-18, "Assault Rifle"),
    (-19, "Shotgun"),
    (-21, "Gren Launcher"),
    (-22, "Laser Pistol"),
    (-23, "EMP Rifle"),
    (-25, "Stasis Field Generator"),
    (-26, "Fusion Cannon"),
    (-27, "Worm Launcher"),
    (-29, "Viral Prolif"),
    (-247, "Psi Amp"),
];

/// One pickup of every ammo type the guns above take (the world clip/box
/// templates the `Clip` relation resolves to). Energy weapons (laser, EMP) and
/// the psi amp have no ammo items - they recharge / draw psi points.
const BENCH_AMMO: &[(i32, &str)] = &[
    (-31, "Standard Clip"),
    (-32, "HE Clip"),
    (-307, "AP Clip"),
    (-42, "Pellet Shot Box"),
    (-43, "Rifled Slug Box"),
    (-36, "Frag Grenade"),
    (-39, "Prox Grenade"),
    (-38, "Incend Grenade"),
    (-37, "EMP Grenade"),
    (-40, "Disruption Grenade"),
    (-41, "Small Prism"),
    (-44, "Large Prism"),
    (-48, "Small Worm Beaker"),
    (-1264, "Large Worm Beaker"),
];

/// The bench sits beside the firing lane on the player's left (+Z with the -X
/// forward), long axis along X: guns on the outer row, ammo on the inner row.
const BENCH_HEIGHT: f32 = 1.1;
const BENCH_Z: f32 = 3.0;
const BENCH_HALF_LENGTH: f32 = 4.6;
const BENCH_HALF_WIDTH: f32 = 1.2;
/// X of the bench slot nearest the player; items step -X-ward from here.
const BENCH_NEAR_X: f32 = 1.6;
const GUN_SPACING: f32 = 0.8;
const AMMO_SPACING: f32 = 0.6;
/// Height of the retaining lip above the bench top.
const BENCH_LIP_HEIGHT: f32 = 0.5;
const BENCH_LIP_THICKNESS: f32 = 0.08;

pub fn create_debug_weapons_scene(
    global_context: &GlobalContext,
    game_options: &GameOptions,
    asset_cache: &mut AssetCache,
    audio_context: &mut AudioContext<EntityId, String>,
) -> Box<dyn GameScene> {
    // Visual floor (under the player) + a vertical wall straight ahead + the
    // weapons bench on the left. The unit cube spans [-0.5, 0.5], so a `cuboid`
    // collider half-extent matches a nonuniform scale of 2x the half-extent.
    let bench_center_x = BENCH_NEAR_X - BENCH_HALF_LENGTH + 0.4;
    // Growing either roster past the bench pushes a spawn into the static lip
    // collider - exactly the interpenetration launch the drop heights dodge.
    debug_assert!(
        BENCH_NEAR_X - (BENCH_GUNS.len() - 1) as f32 * GUN_SPACING
            > bench_center_x - BENCH_HALF_LENGTH + 0.4
    );
    debug_assert!(
        BENCH_NEAR_X - (BENCH_AMMO.len() - 1) as f32 * AMMO_SPACING
            > bench_center_x - BENCH_HALF_LENGTH + 0.4
    );
    // Raised lip around the bench top: grenades and prisms are rolling bodies
    // and a flat table sheds them onto the floor within seconds.
    let lip_color = vec3(0.36, 0.31, 0.24);
    let lip_top = BENCH_HEIGHT + BENCH_LIP_HEIGHT / 2.0;
    let boxes: &[(
        cgmath::Vector3<f32>,
        cgmath::Vector3<f32>,
        cgmath::Vector3<f32>,
    )] = &[
        // floor
        (
            vec3(0.18, 0.18, 0.22),
            vec3(0.0, -0.5, 0.0),
            vec3(40.0, 1.0, 40.0),
        ),
        // wall
        (
            vec3(0.45, 0.45, 0.5),
            vec3(-WALL_DISTANCE, 5.0, 0.0),
            vec3(1.0, 30.0, 30.0),
        ),
        // weapons bench
        (
            vec3(0.30, 0.26, 0.20),
            vec3(bench_center_x, BENCH_HEIGHT / 2.0, BENCH_Z),
            vec3(
                2.0 * BENCH_HALF_LENGTH,
                BENCH_HEIGHT,
                2.0 * BENCH_HALF_WIDTH,
            ),
        ),
        // lip: long sides (inner/outer edges) ...
        (
            lip_color,
            vec3(bench_center_x, lip_top, BENCH_Z - BENCH_HALF_WIDTH),
            vec3(
                2.0 * BENCH_HALF_LENGTH,
                BENCH_LIP_HEIGHT,
                BENCH_LIP_THICKNESS,
            ),
        ),
        (
            lip_color,
            vec3(bench_center_x, lip_top, BENCH_Z + BENCH_HALF_WIDTH),
            vec3(
                2.0 * BENCH_HALF_LENGTH,
                BENCH_LIP_HEIGHT,
                BENCH_LIP_THICKNESS,
            ),
        ),
        // ... and short ends
        (
            lip_color,
            vec3(bench_center_x - BENCH_HALF_LENGTH, lip_top, BENCH_Z),
            vec3(
                BENCH_LIP_THICKNESS,
                BENCH_LIP_HEIGHT,
                2.0 * BENCH_HALF_WIDTH,
            ),
        ),
        (
            lip_color,
            vec3(bench_center_x + BENCH_HALF_LENGTH, lip_top, BENCH_Z),
            vec3(
                BENCH_LIP_THICKNESS,
                BENCH_LIP_HEIGHT,
                2.0 * BENCH_HALF_WIDTH,
            ),
        ),
    ];

    let scene_objects = boxes
        .iter()
        .map(|(color, translation, scale)| cube_object(*color, *translation, *scale))
        .collect::<Vec<_>>();
    let collider = ColliderBuilder::compound(
        boxes
            .iter()
            .map(|(_, translation, scale)| {
                (
                    Isometry::translation(translation.x, translation.y, translation.z),
                    SharedShape::cuboid(scale.x / 2.0, scale.y / 2.0, scale.z / 2.0),
                )
            })
            .collect(),
    )
    .build();

    // Spawn at the floor with identity yaw: the default view forward is -X, so
    // the wall sits dead ahead and the bench is a quarter-turn to the left.
    let mut builder = DebugSceneBuilder::new("debug_weapons")
        .with_spawn_location(SpawnLocation::PositionRotation(
            vec3(0.0, 2.0, 0.0),
            Quaternion::from_angle_y(Deg(0.0)),
        ))
        .with_physics_geometry(collider);
    for scene_object in scene_objects {
        builder = builder.add_scene_object(scene_object);
    }

    println!(
        "[debug_weapons] The wall ahead catches shots as hit-spangs; cycle flat\n\
         viewmodels with the `DebugCycleWeapon` input action (`B` on desktop).\n\
         The bench on the left carries every player gun and one box of each ammo\n\
         type, and the character sheet is maxed - grab (VR) or frob (flat) and go."
    );

    builder.build_with_hooks(
        DebugSceneBuildOptions {
            global_context,
            game_options,
            asset_cache,
            audio_context,
        },
        ArsenalHooks::default(),
    )
}

#[derive(Default)]
struct ArsenalHooks {
    populated: bool,
}

impl DebugSceneHooks for ArsenalHooks {
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

        // "Full stats": max the character sheet through the same provisioning
        // path as `POST /v1/player/stats`, so nothing (skill gates, psi tiers)
        // stands between the tester and any weapon on the bench.
        let request = DebugPlayerStatsRequest {
            strength: Some(STAT_CAP),
            endurance: Some(STAT_CAP),
            agility: Some(STAT_CAP),
            psionic_ability: Some(STAT_CAP),
            cyber_affinity: Some(STAT_CAP),
            skills: DebugSkillLevelsRequest {
                standard_weapons: Some(SKILL_CAP),
                energy_weapons: Some(SKILL_CAP),
                heavy_weapons: Some(SKILL_CAP),
                exotic_weapons: Some(SKILL_CAP),
                hack: Some(SKILL_CAP),
                repair: Some(SKILL_CAP),
                modify: Some(SKILL_CAP),
                maintenance: Some(SKILL_CAP),
                research: Some(SKILL_CAP),
            },
            psi_tier: Some(PSI_TIER_CAP),
            cyber_modules: None,
        };
        if let Err(err) = core.set_player_stats(&request) {
            tracing::warn!("[debug_weapons] failed to max player stats: {err}");
        }

        // Guns on the outer row, ammo on the inner row. Drop heights are
        // load-bearing: a big gun's collider extends below its origin, and a
        // spawn that interpenetrates the tabletop gets solver-launched across
        // the room (+0.05 sent grenades 15 units away) - while a *high* drop
        // makes the bouncy grenades hop the lip. Small ammo drops low, guns
        // higher.
        let mut effects = Vec::new();
        for (index, (template_id, _)) in BENCH_GUNS.iter().enumerate() {
            let x = BENCH_NEAR_X - index as f32 * GUN_SPACING;
            effects.push(spawn_at(
                *template_id,
                Point3::new(x, BENCH_HEIGHT + 0.4, BENCH_Z + 0.5),
            ));
        }
        for (index, (template_id, _)) in BENCH_AMMO.iter().enumerate() {
            let x = BENCH_NEAR_X - index as f32 * AMMO_SPACING;
            // Zigzag so a bouncing neighbor can't billiard the whole row.
            let z = BENCH_Z - 0.55 - 0.25 * (index % 2) as f32;
            effects.push(spawn_at(
                *template_id,
                Point3::new(x, BENCH_HEIGHT + 0.15, z),
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
