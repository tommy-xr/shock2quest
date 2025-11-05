use std::{collections::HashMap, fs::OpenOptions};

use engine::{assets::asset_cache::AssetCache, audio::AudioContext};
use shipyard::EntityId;

use crate::{
    game_scene::GameScene,
    mission::{
        entity_populator::{EntityPopulator, MissionEntityPopulator, SaveFileEntityPopulator},
        GlobalContext, Mission,
    },
    save_load::{EntitySaveData, HeldItemSaveData, SaveData},
    GameOptions, QuestInfo, SpawnLocation,
};

pub mod debug_entity_playground;
pub mod debug_hud;
pub mod debug_map;
pub mod debug_minimal;
pub mod debug_ragdoll;

pub use debug_entity_playground::DebugEntityPlaygroundScene;
pub use debug_hud::DebugHudScene;
pub use debug_map::DebugMapScene;
pub use debug_minimal::DebugMinimalScene;
pub use debug_ragdoll::DebugRagdollScene;

pub struct SceneInitResult {
    pub scene: Box<dyn GameScene>,
    pub mission_save_data: HashMap<String, EntitySaveData>,
}

pub fn create_initial_scene(
    asset_cache: &mut AssetCache,
    audio_context: &mut AudioContext<EntityId, String>,
    global_context: &GlobalContext,
    options: &GameOptions,
) -> SceneInitResult {
    if options.mission.eq_ignore_ascii_case("debug_minimal") {
        return SceneInitResult {
            scene: Box::new(DebugMinimalScene::new()),
            mission_save_data: HashMap::new(),
        };
    }

    if options
        .mission
        .eq_ignore_ascii_case("debug_entity_playground")
    {
        return SceneInitResult {
            scene: Box::new(DebugEntityPlaygroundScene::new(
                global_context,
                options,
                asset_cache,
                audio_context,
            )),
            mission_save_data: HashMap::new(),
        };
    }

    if options.mission.eq_ignore_ascii_case("debug_hud") {
        return SceneInitResult {
            scene: Box::new(DebugHudScene::new()),
            mission_save_data: HashMap::new(),
        };
    }

    if options.mission.eq_ignore_ascii_case("debug_map") {
        return SceneInitResult {
            scene: Box::new(DebugMapScene::new()),
            mission_save_data: HashMap::new(),
        };
    }

    if options.mission.eq_ignore_ascii_case("debug_ragdoll") {
        return SceneInitResult {
            scene: Box::new(DebugRagdollScene::new(
                global_context,
                options,
                asset_cache,
                audio_context,
            )),
            mission_save_data: HashMap::new(),
        };
    }

    if let Some(save_file_path) = &options.save_file {
        let mut file = OpenOptions::new().read(true).open(save_file_path).unwrap();
        let save_data = SaveData::read(&mut file);
        let (mission, mission_to_save_data) = load_mission_from_save_data(
            save_data,
            asset_cache,
            audio_context,
            global_context,
            options,
        );
        return SceneInitResult {
            scene: Box::new(mission),
            mission_save_data: mission_to_save_data,
        };
    }

    let mission_save_data = HashMap::new();
    let active_mission = Mission::load(
        options.mission.to_owned(),
        asset_cache,
        audio_context,
        global_context,
        options.spawn_location.clone(),
        QuestInfo::new(),
        Box::new(MissionEntityPopulator::create()),
        HeldItemSaveData::empty(),
        options,
    );

    SceneInitResult {
        scene: Box::new(active_mission),
        mission_save_data,
    }
}

pub fn load_mission_from_save_data(
    save_data: SaveData,
    asset_cache: &mut AssetCache,
    audio_context: &mut AudioContext<EntityId, String>,
    global_context: &GlobalContext,
    game_options: &GameOptions,
) -> (Mission, HashMap<String, EntitySaveData>) {
    let current_mission = save_data.global_data.active_mission.clone();

    let populator: Box<dyn EntityPopulator> = {
        if let Some(save_data) = save_data
            .level_data
            .get(&current_mission.to_ascii_lowercase())
        {
            let save_data_cloned = save_data.clone();
            let populator = SaveFileEntityPopulator::create(save_data_cloned);
            Box::new(populator)
        } else {
            Box::new(MissionEntityPopulator::create())
        }
    };

    let spawn_loc = SpawnLocation::PositionRotation(
        save_data.global_data.position,
        save_data.global_data.rotation,
    );

    let active_mission = Mission::load(
        current_mission,
        asset_cache,
        audio_context,
        global_context,
        spawn_loc,
        save_data.global_data.quest_info,
        populator,
        save_data.global_data.held_items,
        game_options,
    );

    (active_mission, save_data.level_data)
}
