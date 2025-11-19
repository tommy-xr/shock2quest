use engine::{assets::asset_cache::AssetCache, audio::AudioContext};
use shipyard::EntityId;

use crate::{GameOptions, mission::GlobalContext};

use super::debug_common::{DebugScene, DebugSceneBuildOptions, DebugSceneBuilder};

/// Minimal debug scene that keeps the player anchored with no extra rendering
pub type DebugMinimalScene = DebugScene;

impl DebugMinimalScene {
    pub fn create(
        global_context: &GlobalContext,
        game_options: &GameOptions,
        asset_cache: &mut AssetCache,
        audio_context: &mut AudioContext<EntityId, String>,
    ) -> Self {
        let builder = DebugSceneBuilder::new("debug_minimal").with_default_floor();

        let build_options = DebugSceneBuildOptions {
            global_context,
            game_options,
            asset_cache,
            audio_context,
        };

        builder.build(build_options)
    }
}
