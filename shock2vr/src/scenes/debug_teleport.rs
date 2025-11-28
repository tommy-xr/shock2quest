use engine::{assets::asset_cache::AssetCache, audio::AudioContext};
use shipyard::EntityId;

use crate::{GameOptions, mission::GlobalContext};

use super::debug_common::{DebugScene, DebugSceneBuildOptions, DebugSceneBuilder};

/// Minimal debug scene with teleport experimental feature enabled
pub struct DebugTeleportScene;

impl DebugTeleportScene {
    pub fn create(
        global_context: &GlobalContext,
        game_options: &GameOptions,
        asset_cache: &mut AssetCache,
        audio_context: &mut AudioContext<EntityId, String>,
    ) -> DebugScene {
        // Create new game options with teleport feature enabled
        let mut experimental_features = game_options.experimental_features.clone();
        experimental_features.insert("teleport".to_string());

        let teleport_options = GameOptions {
            mission: game_options.mission.clone(),
            spawn_location: game_options.spawn_location.clone(),
            save_file: game_options.save_file.clone(),
            render_particles: game_options.render_particles,
            debug_physics: game_options.debug_physics,
            debug_draw: game_options.debug_draw,
            debug_portals: game_options.debug_portals,
            debug_show_ids: game_options.debug_show_ids,
            debug_skeletons: game_options.debug_skeletons,
            debug_ai: game_options.debug_ai,
            experimental_features,
        };

        let builder = DebugSceneBuilder::new("debug_teleport").with_default_floor();

        let build_options = DebugSceneBuildOptions {
            global_context,
            game_options: &teleport_options,
            asset_cache,
            audio_context,
        };

        builder.build(build_options)
    }
}
