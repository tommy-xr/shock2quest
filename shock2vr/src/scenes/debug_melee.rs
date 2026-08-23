//! Debug scene for VR melee work: grip calibration, physical contact, damage.
//!
//! Melee had no playground of its own. Both existing melee e2e tests borrow
//! MedSci geometry - a shipped breakable pane, a debug-spawned creature, and a
//! staging pose measured against a corridor - which is why they are slow and
//! why a change to the weapon body forces the *tests* to be re-staged. This
//! scene is the controlled equivalent: a rack of every authored player melee
//! weapon within arm's reach, a row of damageable creatures at known
//! distances, and a flat wall to swing into.
//!
//! Contact damage here needs no trigger. The scene raises
//! [`dev_params::MELEE_FREE_SWING_SPEED`], so a swing damages because the
//! weapon was moving - which is the model physical melee is heading toward and
//! the one worth exercising. Missions are untouched; the parameter defaults to
//! 0 (the shipped trigger window) everywhere else.
//!
//! Grip calibration: raise `melee_glove_overlay` (Developer screen, or
//! `POST /v1/dev-params`) to draw the tracked-hand glove *as well as* the
//! weapon's own `_h` model. The glove's origin is the controller pose exactly;
//! the `_h` rig's baked fist is where the wield believes the hand is. Any gap
//! between them is the calibration error, and it is only visible with both on
//! screen at once.

use cgmath::{Deg, Matrix4, Point3, Quaternion, Rotation3, SquareMatrix, Vector3, vec3};
use engine::{
    assets::asset_cache::AssetCache,
    audio::AudioContext,
    scene::{SceneObject, color_material, cube},
};
use rapier3d::prelude::{ColliderBuilder, Isometry, SharedShape};
use shipyard::EntityId;

use crate::{
    GameOptions, dev_params,
    game_scene::GameScene,
    mission::{
        GlobalContext, SpawnLocation, entity_creator::CreateEntityOptions,
        mission_core::MissionCore,
    },
    scenes::debug_common::{
        DebugSceneBuildOptions, DebugSceneBuilder, DebugSceneHooks, HookedDebugScene,
    },
    scripts::Effect,
};

/// Every authored player melee weapon (`PropLimbModel`), in rack order.
const MELEE_WEAPONS: &[(i32, &str)] = &[
    (-928, "Wrench"),
    (-24, "Electro Shock"),
    (-28, "Crystal Shard"),
    (-2291, "Psi Sword"),
];

/// The creature the ragdoll scene already spawns; a real actor, so contact
/// resolves through the authored stim/receptron path rather than a stand-in.
const TARGET_CREATURE: i32 = -397;

/// Distances (world units) ahead of the player for the target row. The first
/// is a step away, the others need walking to - spaced so a swing can only
/// ever reach one of them.
const TARGET_DISTANCES: &[f32] = &[4.0, 6.0, 8.0];

/// Contact speed a swing must carry to damage, in world units per second.
/// Resting a weapon against a creature does nothing; a deliberate swing does.
///
/// Measured in this scene rather than guessed: a held weapon at rest reads
/// ~0.005, and a brisk controller sweep peaks at ~1.6 - far below the hand's
/// own ~10, because the spring drive currently attenuates a swing several-fold
/// (see `physics::spring_sweep`). 0.5 separates the two cleanly today and
/// still will once the drive tracks properly, since that only raises the
/// swinging figure.
const FREE_SWING_SPEED: f32 = 0.5;

/// Distance to the wall the player can swing into, straight ahead.
const WALL_DISTANCE: f32 = 12.0;

/// Where the weapon rack's top surface sits, and how far ahead of the player.
/// Within a seated arm's reach (a world unit is 0.76 m), so a weapon can be
/// picked up without walking.
const RACK_HEIGHT: f32 = 1.1;
const RACK_DISTANCE: f32 = 1.0;

fn cube_object(color: Vector3<f32>, translation: Vector3<f32>, scale: Vector3<f32>) -> SceneObject {
    let mut object = SceneObject::new(color_material::create(color), Box::new(cube::create()));
    object.set_transform(
        Matrix4::from_translation(translation)
            * Matrix4::from_nonuniform_scale(scale.x, scale.y, scale.z),
    );
    object
}

pub fn create_debug_melee_scene(
    global_context: &GlobalContext,
    game_options: &GameOptions,
    asset_cache: &mut AssetCache,
    audio_context: &mut AudioContext<EntityId, String>,
) -> Box<dyn GameScene> {
    // Contact damages on its own here - see the module docs.
    dev_params::set(dev_params::MELEE_FREE_SWING_SPEED, FREE_SWING_SPEED);

    // Same controlled shell as debug_weapons: floor underfoot, wall ahead.
    // The unit cube spans [-0.5, 0.5], so a nonuniform scale is twice the
    // matching collider half-extent.
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
    let rack = cube_object(
        vec3(0.30, 0.26, 0.20),
        vec3(-RACK_DISTANCE, RACK_HEIGHT / 2.0, 0.0),
        vec3(0.6, RACK_HEIGHT, 3.0),
    );

    let collider = ColliderBuilder::compound(vec![
        (
            Isometry::translation(0.0, -0.5, 0.0),
            SharedShape::cuboid(20.0, 0.5, 20.0),
        ),
        (
            Isometry::translation(-WALL_DISTANCE, 5.0, 0.0),
            SharedShape::cuboid(0.5, 15.0, 15.0),
        ),
        (
            Isometry::translation(-RACK_DISTANCE, RACK_HEIGHT / 2.0, 0.0),
            SharedShape::cuboid(0.3, RACK_HEIGHT / 2.0, 1.5),
        ),
    ])
    .build();

    let builder = DebugSceneBuilder::new("debug_melee")
        .with_spawn_location(SpawnLocation::PositionRotation(
            vec3(0.0, 2.0, 0.0),
            Quaternion::from_angle_y(Deg(0.0)),
        ))
        .with_physics_geometry(collider)
        .add_scene_object(floor)
        .add_scene_object(wall)
        .add_scene_object(rack);

    let core = builder.build_core(DebugSceneBuildOptions {
        global_context,
        game_options,
        asset_cache,
        audio_context,
    });

    println!(
        "[debug_melee] A rack of every player melee weapon is within reach ahead;\n\
         grab one (squeeze) and swing at the creatures beyond it. Contact damages\n\
         on its own above {FREE_SWING_SPEED} units/s - no trigger needed.\n\
         Set `melee_glove_overlay` to 1 (Developer screen, or POST /v1/dev-params)\n\
         to draw the tracked-hand glove alongside the weapon's own arm model and\n\
         see how far the `_h` fist is from where the controller actually is."
    );

    Box::new(HookedDebugScene::new(core, MeleeHooks::default()))
}

#[derive(Default)]
struct MeleeHooks {
    populated: bool,
}

impl DebugSceneHooks for MeleeHooks {
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

        let mut effects = Vec::new();
        for (index, (template_id, _)) in MELEE_WEAPONS.iter().enumerate() {
            // Spaced along the rack's long axis, just above its top so each
            // weapon settles onto it instead of through it.
            let z = (index as f32 - (MELEE_WEAPONS.len() as f32 - 1.0) / 2.0) * 0.7;
            effects.push(spawn_at(
                *template_id,
                Point3::new(-RACK_DISTANCE, RACK_HEIGHT + 0.05, z),
            ));
        }
        for distance in TARGET_DISTANCES {
            effects.push(spawn_at(TARGET_CREATURE, Point3::new(-distance, 1.0, 0.0)));
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

fn spawn_at(template_id: i32, position: Point3<f32>) -> Effect {
    Effect::CreateEntity {
        template_id,
        position,
        orientation: Quaternion::new(1.0, 0.0, 0.0, 0.0),
        // Identity, not `from_translation(position)`: the root transform is
        // applied *on top of* `position`, so passing the position twice lands
        // the entity at double the offset (`debug_ragdoll` does this and its
        // hybrid spawns twice as far out as its own focus point claims).
        root_transform: Matrix4::identity(),
        options: CreateEntityOptions::default(),
    }
}
