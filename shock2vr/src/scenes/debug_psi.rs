//! Debug scene for psi amp / psi power work.
//!
//! The player spawns holding the psi amp with a full psi pool and a maxed
//! character sheet, facing a walled pen of creatures straight ahead with a
//! flat wall behind it. Select a power with `CyclePsiPower` (`Y` on desktop /
//! `POST /v1/input/action {"action":"CyclePsiPower"}`) and fire; projectile
//! powers (Cryokinesis, Pyrokinesis, ...) hit the creatures or the wall, the
//! pen keeps the targets at range so offensive powers can be measured against
//! a live creature, and the psi bar on the HUD drains by the power's tier per
//! cast.

use cgmath::{Deg, Point3, Quaternion, Rotation3, vec3};
use engine::{assets::asset_cache::AssetCache, audio::AudioContext};
use shipyard::{EntityId, Get, UniqueView, ViewMut};

use crate::{
    GameOptions,
    game_scene::GameScene,
    mission::{
        GlobalContext, SpawnLocation,
        mission_core::{MissionCore, PlayerInfo},
    },
    scripts::Effect,
};

use super::debug_common::{
    AutoEquipHooks, DebugBox, DebugSceneBuildOptions, DebugSceneBuilder, DebugSceneHooks,
    boxes_to_geometry, max_player_stats, spawn_at,
};

/// The Psi Amp player weapon.
const PSI_AMP_TEMPLATE_ID: i32 = -247;

/// The penned creature: -397 = grunt og-pipe (pipe hybrid), the same actor
/// `debug_melee` and `debug_hitbox` use, so damage/AI observations line up
/// across scenes.
const TARGET_CREATURE: i32 = -397;

/// Where the creatures stand inside the pen, as (x, z): world units ahead of
/// the player along -X, and across the firing line. The near one sits on the
/// line so a straight cast lands without aiming; the far one is offset so the
/// near one does not screen it.
const TARGET_POSITIONS: &[(f32, f32)] = &[(9.0, 0.0), (11.0, 1.8)];

/// The pen: a box closed on all four sides. Debug scenes have no nav mesh, so
/// a loose creature would walk straight at the player and turn every
/// measurement into a melee. The wall height is a measurement, not a rule:
/// 1.2 was verified to hold an alerted pipe hybrid (~3.4 tall, no step over)
/// while a level cast from the player's eye (~2.3) clears it onto the
/// creature's torso. Re-check it if `TARGET_CREATURE` changes.
const PEN_NEAR: f32 = 7.0;
const PEN_FAR: f32 = 13.0;
const PEN_HALF_WIDTH: f32 = 3.0;
const PEN_WALL_HEIGHT: f32 = 1.2;

/// Distance (world units) from the player to the backstop wall, straight
/// ahead (-X, the default-view forward), behind the pen.
const WALL_DISTANCE: f32 = 15.0;

/// The hazard patch: `Rad Burst`, the persistent radius radiation source the
/// game spawns from a destroyed rad barrel (`StimSource` Radiation, intensity
/// 8, radius 6). It renders nothing, so a floor patch marks it.
const RADIATION_SOURCE_TEMPLATE_ID: i32 = -1219;
const RADIATION_SOURCE_RADIUS: f32 = 6.0;

/// Where the hazard patch sits, as (x, z): beside the player and off the
/// firing line, far enough that the spawn point is outside the source's
/// 6-unit radius - so the player starts clean and only gets irradiated after
/// a deliberate `POST /v1/player/teleport` into it.
const RADIATION_SOURCE_POSITION: (f32, f32) = (0.0, 9.0);

pub fn create_debug_psi_scene(
    global_context: &GlobalContext,
    game_options: &GameOptions,
    asset_cache: &mut AssetCache,
    audio_context: &mut AudioContext<EntityId, String>,
) -> Box<dyn GameScene> {
    let pen_wall = vec3(0.55, 0.32, 0.28);
    let pen_mid_x = -(PEN_NEAR + PEN_FAR) / 2.0;
    let pen_len = PEN_FAR - PEN_NEAR;
    let boxes: &[DebugBox] = &[
        // floor
        (
            vec3(0.18, 0.18, 0.22),
            vec3(0.0, -0.5, 0.0),
            vec3(40.0, 1.0, 40.0),
        ),
        // backstop wall
        (
            vec3(0.45, 0.45, 0.5),
            vec3(-WALL_DISTANCE, 5.0, 0.0),
            vec3(1.0, 30.0, 30.0),
        ),
        // pen: front and back
        (
            pen_wall,
            vec3(-PEN_NEAR, PEN_WALL_HEIGHT / 2.0, 0.0),
            vec3(1.0, PEN_WALL_HEIGHT, 2.0 * PEN_HALF_WIDTH),
        ),
        (
            pen_wall,
            vec3(-PEN_FAR, PEN_WALL_HEIGHT / 2.0, 0.0),
            vec3(1.0, PEN_WALL_HEIGHT, 2.0 * PEN_HALF_WIDTH),
        ),
        // hazard patch: a floor marker for the (invisible) radiation source.
        // The field is a 6-unit sphere; the marker is the square inscribed in
        // it, so every point on the marker is inside the field. Sunk into the
        // floor slab so a walking tester does not step onto a lip.
        (
            vec3(0.15, 0.45, 0.15),
            vec3(
                RADIATION_SOURCE_POSITION.0,
                -0.02,
                RADIATION_SOURCE_POSITION.1,
            ),
            vec3(
                RADIATION_SOURCE_RADIUS * std::f32::consts::SQRT_2,
                0.04,
                RADIATION_SOURCE_RADIUS * std::f32::consts::SQRT_2,
            ),
        ),
        // pen: sides
        (
            pen_wall,
            vec3(pen_mid_x, PEN_WALL_HEIGHT / 2.0, -PEN_HALF_WIDTH),
            vec3(pen_len, PEN_WALL_HEIGHT, 1.0),
        ),
        (
            pen_wall,
            vec3(pen_mid_x, PEN_WALL_HEIGHT / 2.0, PEN_HALF_WIDTH),
            vec3(pen_len, PEN_WALL_HEIGHT, 1.0),
        ),
    ];
    let (scene_objects, collider) = boxes_to_geometry(boxes);

    let mut builder = DebugSceneBuilder::new("debug_psi")
        .with_spawn_location(SpawnLocation::PositionRotation(
            vec3(0.0, 2.0, 0.0),
            Quaternion::from_angle_y(Deg(0.0)),
        ))
        .with_physics_geometry(collider);
    for scene_object in scene_objects {
        builder = builder.add_scene_object(scene_object);
    }

    println!(
        "[debug_psi] Player is equipped with the Psi Amp, psi pool full, every stat at cap.\n\
         Select a power with the `CyclePsiPower` input action (`Y` on desktop),\n\
         then fire to cast it. Two pipe hybrids stand in the pen ahead,\n\
         and a radiation hazard patch sits to the side (+Z); teleport into it\n\
         to accumulate radiation."
    );
    builder.build_with_hooks(
        DebugSceneBuildOptions {
            global_context,
            game_options,
            asset_cache,
            audio_context,
        },
        PsiHooks {
            populated: false,
            equip: AutoEquipHooks::new(PSI_AMP_TEMPLATE_ID),
        },
    )
}

struct PsiHooks {
    populated: bool,
    equip: AutoEquipHooks,
}

impl DebugSceneHooks for PsiHooks {
    fn before_handle_effects(
        &mut self,
        core: &mut MissionCore,
        effects: &mut Vec<Effect>,
        global_context: &GlobalContext,
        game_options: &GameOptions,
        asset_cache: &mut AssetCache,
        audio_context: &mut AudioContext<EntityId, String>,
    ) {
        if !self.populated {
            self.populated = true;

            // Nothing on the psi cast path reads the character sheet yet
            // (every power is already known in a debug scene, and the amp's
            // effective PSI stat is a constant), so this is parity with
            // `debug_weapons` and future-proofing, not a gate being lifted.
            max_player_stats(core, "debug_psi");
            // The psi pool is what actually limits a test session: the player
            // template seeds it short of its maximum. Start full.
            core.world.run(
                |player: UniqueView<PlayerInfo>,
                 mut psi: ViewMut<dark::properties::PropPsiState>| {
                    if let Ok(psi) = (&mut psi).get(player.entity_id) {
                        psi.psi_points = psi.max_psi_points;
                    }
                },
            );

            let mut spawns: Vec<Effect> = TARGET_POSITIONS
                .iter()
                .map(|(x, z)| spawn_at(TARGET_CREATURE, Point3::new(-x, 1.0, *z)))
                .collect();
            spawns.push(spawn_at(
                RADIATION_SOURCE_TEMPLATE_ID,
                Point3::new(
                    RADIATION_SOURCE_POSITION.0,
                    1.0,
                    RADIATION_SOURCE_POSITION.1,
                ),
            ));
            core.handle_effects(
                spawns,
                global_context,
                game_options,
                asset_cache,
                audio_context,
            );
        }
        self.equip.before_handle_effects(
            core,
            effects,
            global_context,
            game_options,
            asset_cache,
            audio_context,
        );
    }
}
