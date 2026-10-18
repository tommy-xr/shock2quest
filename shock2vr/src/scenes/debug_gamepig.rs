//! The GamePig handheld with every cartridge installed, on a quiet floor.
//!
//! The player spawns with the GamePig auto-equipped; in VR it lands in front of
//! the player to pick up. Every game is on the index from the start, so each
//! one's controls can be tried without hunting cartridges through the station.

use cgmath::{Deg, Quaternion, Rotation3, vec3};
use engine::{assets::asset_cache::AssetCache, audio::AudioContext};
use shipyard::{EntityId, UniqueViewMut};

use super::debug_common::{
    AutoEquipHooks, DebugSceneBuildOptions, DebugSceneBuilder, DebugSceneHooks, boxes_to_geometry,
};
use crate::{
    GameOptions,
    game_scene::GameScene,
    mission::{GlobalContext, SpawnLocation, mission_core::MissionCore},
    quest_info::QuestInfo,
    scripts::Effect,
};

/// The GamePig ("Gameboy") handheld.
const GAMEPIG_TEMPLATE_ID: i32 = -3864;
/// Every game's install bit (`P$MiniGames`; games are bits 1..=12).
const ALL_GAMES: u32 = 0x1ffe;

pub fn create_debug_gamepig_scene(
    global_context: &GlobalContext,
    game_options: &GameOptions,
    asset_cache: &mut AssetCache,
    audio_context: &mut AudioContext<EntityId, String>,
) -> Box<dyn GameScene> {
    let (objects, collider) = boxes_to_geometry(&[(
        vec3(0.16, 0.18, 0.22),
        vec3(0.0, -0.5, 0.0),
        vec3(10.0, 1.0, 10.0),
    )]);
    let mut builder = DebugSceneBuilder::new("debug_gamepig")
        .with_spawn_location(SpawnLocation::PositionRotation(
            vec3(0.0, 1.8, 0.0),
            Quaternion::from_angle_y(Deg(0.0)),
        ))
        .with_physics_geometry(collider);
    for object in objects {
        builder = builder.add_scene_object(object);
    }
    builder.build_with_hooks(
        DebugSceneBuildOptions {
            global_context,
            game_options,
            asset_cache,
            audio_context,
        },
        GamePigHooks {
            installed: false,
            equip: AutoEquipHooks::new(GAMEPIG_TEMPLATE_ID),
        },
    )
}

struct GamePigHooks {
    installed: bool,
    equip: AutoEquipHooks,
}

impl DebugSceneHooks for GamePigHooks {
    fn before_handle_effects(
        &mut self,
        core: &mut MissionCore,
        effects: &mut Vec<Effect>,
        global_context: &GlobalContext,
        game_options: &GameOptions,
        asset_cache: &mut AssetCache,
        audio_context: &mut AudioContext<EntityId, String>,
    ) {
        if !self.installed {
            self.installed = true;
            core.world
                .borrow::<UniqueViewMut<QuestInfo>>()
                .unwrap()
                .mini_games = ALL_GAMES;
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
