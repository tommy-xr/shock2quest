//! One ordinary casino machine with a small bankroll for repeatable testing.

use cgmath::{Deg, Point3, Quaternion, Rotation3, vec3};
use engine::{assets::asset_cache::AssetCache, audio::AudioContext};
use shipyard::EntityId;

use super::debug_common::{
    DebugSceneBuildOptions, DebugSceneBuilder, DebugSceneHooks, HookedDebugScene,
    boxes_to_geometry, spawn_at_oriented,
};
use crate::{
    GameOptions,
    game_scene::GameScene,
    mission::{GlobalContext, SpawnLocation, mission_core::MissionCore},
    scripts::Effect,
};

pub fn create_debug_slot_scene(
    global_context: &GlobalContext,
    game_options: &GameOptions,
    asset_cache: &mut AssetCache,
    audio_context: &mut AudioContext<EntityId, String>,
) -> Box<dyn GameScene> {
    let (objects, collider) = boxes_to_geometry(&[(
        vec3(0.16, 0.18, 0.22),
        vec3(-2.0, -0.5, 0.0),
        vec3(12.0, 1.0, 10.0),
    )]);
    let mut builder = DebugSceneBuilder::new("debug_slot")
        .with_spawn_location(SpawnLocation::PositionRotation(
            vec3(3.0, 1.8, 0.0),
            Quaternion::from_angle_y(Deg(0.0)),
        ))
        .with_physics_geometry(collider);
    for object in objects {
        builder = builder.add_scene_object(object);
    }
    let core = builder.build_core(DebugSceneBuildOptions {
        global_context,
        game_options,
        asset_cache,
        audio_context,
    });
    Box::new(HookedDebugScene::new(core, SlotHooks { populated: false }))
}

struct SlotHooks {
    populated: bool,
}

impl DebugSceneHooks for SlotHooks {
    fn before_handle_effects(
        &mut self,
        _core: &mut MissionCore,
        effects: &mut Vec<Effect>,
        _global_context: &GlobalContext,
        _game_options: &GameOptions,
        _asset_cache: &mut AssetCache,
        _audio_context: &mut AudioContext<EntityId, String>,
    ) {
        if self.populated {
            return;
        }
        self.populated = true;
        effects.extend([
            spawn_at_oriented(
                -937,
                Point3::new(-3.0, 1.922, 0.0),
                Quaternion::from_angle_y(Deg(90.0)),
            ),
            Effect::AwardNanites { amount: 20 },
        ]);
    }
}
