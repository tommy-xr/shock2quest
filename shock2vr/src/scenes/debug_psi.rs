//! Debug scene for psi amp / psi power work.
//!
//! The player spawns holding the psi amp on a floor facing a flat wall (same
//! layout as `debug_weapons`), so each power's cast can be observed in
//! isolation. Select a power with `CyclePsiPower` (`Y` on desktop /
//! `POST /v1/input/action {"action":"CyclePsiPower"}`) and fire; projectile
//! powers (Cryokinesis, Pyrokinesis, ...) land on the wall, and the psi bar
//! on the HUD drains by the power's tier per cast.

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
    game_scene::GameScene,
    mission::{GlobalContext, SpawnLocation, mission_core::MissionCore},
    scripts::Effect,
};

use super::debug_common::{
    DebugSceneBuildOptions, DebugSceneBuilder, DebugSceneHooks, HookedDebugScene,
};

/// The Psi Amp player weapon.
const PSI_AMP_TEMPLATE_ID: i32 = -247;

/// Distance (world units) from the player to the test wall, straight ahead
/// (-X, the default-view forward).
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

pub fn create_debug_psi_scene(
    global_context: &GlobalContext,
    game_options: &GameOptions,
    asset_cache: &mut AssetCache,
    audio_context: &mut AudioContext<EntityId, String>,
) -> Box<dyn GameScene> {
    // Same controlled environment as debug_weapons: a floor under the player
    // and a vertical wall straight ahead to catch projectiles.
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

    let builder = DebugSceneBuilder::new("debug_psi")
        .with_spawn_location(SpawnLocation::PositionRotation(
            vec3(0.0, 2.0, 0.0),
            Quaternion::from_angle_y(Deg(0.0)),
        ))
        .with_physics_geometry(collider)
        .add_scene_object(floor)
        .add_scene_object(wall);

    let core = builder.build_core(DebugSceneBuildOptions {
        global_context,
        game_options,
        asset_cache,
        audio_context,
    });
    Box::new(HookedDebugScene::new(core, PsiHooks::new()))
}

struct PsiHooks {
    equipped: bool,
}

impl PsiHooks {
    fn new() -> Self {
        println!(
            "[debug_psi] Player is equipped with the Psi Amp.\n\
             Select a power with the `CyclePsiPower` input action (`Y` on desktop),\n\
             then fire to cast it. Projectile powers land on the wall ahead."
        );
        Self { equipped: false }
    }
}

impl DebugSceneHooks for PsiHooks {
    fn before_handle_effects(
        &mut self,
        core: &mut MissionCore,
        _effects: &mut Vec<Effect>,
        global_context: &GlobalContext,
        game_options: &GameOptions,
        asset_cache: &mut AssetCache,
        audio_context: &mut AudioContext<EntityId, String>,
    ) {
        if self.equipped {
            return;
        }
        // In flat presentation SpawnInFrontOfPlayer auto-wields a weapon when
        // the player is unarmed - so this both spawns and equips the amp.
        let spawn = Effect::SpawnInFrontOfPlayer {
            template_id: PSI_AMP_TEMPLATE_ID,
            head_rotation: Quaternion::new(1.0, 0.0, 0.0, 0.0),
            auto_wield: true,
        };
        core.handle_effects(
            vec![spawn],
            global_context,
            game_options,
            asset_cache,
            audio_context,
        );
        self.equipped = true;
    }
}
