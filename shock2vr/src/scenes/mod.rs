use std::{
    collections::HashMap,
    fs::OpenOptions,
    path::{Path, PathBuf},
};

use engine::{assets::asset_cache::AssetCache, audio::AudioContext};
use shipyard::EntityId;

use crate::{
    GameOptions, QuestInfo, SpawnLocation,
    game_scene::GameScene,
    mission::{
        GlobalContext, Mission,
        entity_populator::{EntityPopulator, MissionEntityPopulator, SaveFileEntityPopulator},
    },
    paths,
    save_load::{EntitySaveData, HeldItemSaveData, SaveData},
};

pub mod cutscene_player;
pub mod debug_camera;
pub mod debug_common;
pub mod debug_gloves;
pub mod debug_hud;
pub mod debug_joint_constraint;
pub mod debug_map;
pub mod debug_minimal;
pub mod debug_ragdoll;
pub mod debug_teleport;
pub mod debug_turret;
pub mod hand_pose;

pub use cutscene_player::CutscenePlayerScene;
pub use debug_camera::DebugCameraScene;
pub use debug_gloves::DebugGlovesScene;
pub use debug_hud::DebugHudScene;
pub use debug_joint_constraint::DebugJointConstraintScene;
pub use debug_map::DebugMapScene;
pub use debug_minimal::DebugMinimalScene;
pub use debug_ragdoll::DebugRagdollScene;
pub use debug_teleport::DebugTeleportScene;
pub use debug_turret::DebugTurretScene;

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
    if is_cutscene_mission(&options.mission) {
        let mission_name = options.mission.clone();
        let cutscene_path = resolve_cutscene_path(&mission_name);
        let cutscene_path_string = cutscene_path.to_string_lossy().into_owned();
        let cutscene = CutscenePlayerScene::new(
            mission_name.clone(),
            cutscene_path_string.clone(),
            audio_context,
        )
        .unwrap_or_else(|err| {
            panic!(
                "Failed to initialize cutscene '{}' from '{}': {}",
                mission_name, cutscene_path_string, err
            )
        });
        return SceneInitResult {
            scene: Box::new(cutscene),
            mission_save_data: HashMap::new(),
        };
    }

    if options.mission.eq_ignore_ascii_case("debug_minimal") {
        return SceneInitResult {
            scene: Box::new(DebugMinimalScene::create(
                global_context,
                options,
                asset_cache,
                audio_context,
            )),
            mission_save_data: HashMap::new(),
        };
    }

    if options.mission.eq_ignore_ascii_case("debug_teleport") {
        return SceneInitResult {
            scene: Box::new(DebugTeleportScene::create(
                global_context,
                options,
                asset_cache,
                audio_context,
            )),
            mission_save_data: HashMap::new(),
        };
    }

    if options.mission.eq_ignore_ascii_case("debug_camera") {
        return SceneInitResult {
            scene: DebugCameraScene::new(global_context, options, asset_cache, audio_context),
            mission_save_data: HashMap::new(),
        };
    }

    if options.mission.eq_ignore_ascii_case("debug_turret") {
        return SceneInitResult {
            scene: DebugTurretScene::new(global_context, options, asset_cache, audio_context),
            mission_save_data: HashMap::new(),
        };
    }

    if options.mission.eq_ignore_ascii_case("debug_hud") {
        return SceneInitResult {
            scene: Box::new(DebugHudScene::new()),
            mission_save_data: HashMap::new(),
        };
    }

    if options.mission.eq_ignore_ascii_case("debug_gloves") {
        return SceneInitResult {
            scene: DebugGlovesScene::new(global_context, options, asset_cache, audio_context),
            mission_save_data: HashMap::new(),
        };
    }

    if options
        .mission
        .eq_ignore_ascii_case("debug_joint_constraint")
    {
        return SceneInitResult {
            scene: DebugJointConstraintScene::new(
                global_context,
                options,
                asset_cache,
                audio_context,
            ),
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
            scene: DebugRagdollScene::new(global_context, options, asset_cache, audio_context),
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

fn is_cutscene_mission(name: &str) -> bool {
    name.trim().to_ascii_lowercase().ends_with(".avi")
}

fn resolve_cutscene_path(name: &str) -> PathBuf {
    let trimmed = name.trim();
    let raw_path = Path::new(trimmed);

    if raw_path.is_absolute() {
        return raw_path.to_path_buf();
    }

    if raw_path.components().count() == 1 {
        paths::data_root().join("cutscenes").join(raw_path)
    } else {
        paths::data_root().join(raw_path)
    }
}
