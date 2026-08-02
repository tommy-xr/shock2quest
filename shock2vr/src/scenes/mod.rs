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
pub mod debug_hitbox;
pub mod debug_hud;
pub mod debug_joint_constraint;
pub mod debug_map;
pub mod debug_minimal;
pub mod debug_particles;
pub mod debug_psi;
pub mod debug_ragdoll;
pub mod debug_teleport;
pub mod debug_turret;
pub mod debug_weapons;
pub mod game_over;
pub mod loading;
pub mod main_menu;

pub use cutscene_player::CutscenePlayerScene;
pub use debug_camera::DebugCameraScene;
pub use debug_gloves::DebugGlovesScene;
pub use debug_hitbox::DebugHitboxScene;
pub use debug_hud::DebugHudScene;
pub use debug_joint_constraint::DebugJointConstraintScene;
pub use debug_map::DebugMapScene;
pub use debug_minimal::DebugMinimalScene;
pub use debug_particles::DebugParticlesScene;
pub use debug_psi::create_debug_psi_scene;
pub use debug_ragdoll::DebugRagdollScene;
pub use debug_teleport::DebugTeleportScene;
pub use debug_turret::DebugTurretScene;
pub use debug_weapons::create_debug_weapons_scene;
pub use game_over::GameOverScene;
pub use loading::LoadingScene;
pub use main_menu::MainMenuScene;

pub struct SceneInitResult {
    pub scene: Box<dyn GameScene>,
    pub mission_save_data: HashMap<String, EntitySaveData>,
}

const ENDING_CUTSCENE_CANDIDATES: &[&str] = &["enhanced/cs3.ogv", "cs3.avi", "cs3.ogv"];

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

    if options.mission.eq_ignore_ascii_case("main_menu") {
        return SceneInitResult {
            scene: Box::new(MainMenuScene::new()),
            mission_save_data: HashMap::new(),
        };
    }

    // Visual-only scene to inspect the loading screen UI (projects/loading-screen.md,
    // PR 1) independent of any real loading.
    if options.mission.eq_ignore_ascii_case("debug_loading") {
        return SceneInitResult {
            scene: Box::new(LoadingScene::new_demo()),
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

    if options.mission.eq_ignore_ascii_case("debug_weapons") {
        return SceneInitResult {
            scene: Box::new(create_debug_weapons_scene(
                global_context,
                options,
                asset_cache,
                audio_context,
            )),
            mission_save_data: HashMap::new(),
        };
    }

    if options.mission.eq_ignore_ascii_case("debug_psi") {
        return SceneInitResult {
            scene: create_debug_psi_scene(global_context, options, asset_cache, audio_context),
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

    if options.mission.eq_ignore_ascii_case("debug_particles") {
        return SceneInitResult {
            scene: DebugParticlesScene::new(global_context, options, asset_cache, audio_context),
            mission_save_data: HashMap::new(),
        };
    }

    if options.mission.eq_ignore_ascii_case("debug_hitbox") {
        return SceneInitResult {
            scene: DebugHitboxScene::new(global_context, options, asset_cache, audio_context),
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

    let mut active_mission = Mission::load(
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
    crate::save_load::restore_player_vitals(
        &active_mission.mission_core.world,
        save_data.global_data.player_vitals,
    );

    // A loaded mission rebuilds Rapier from scratch. Mark the restored player
    // so the first BeginIntersect events reconstruct already-existing sensor
    // overlaps without replaying their ENTER actions (#547). This is distinct
    // from locomotion teleport (which must trigger) and scripted teleport
    // suppression (#515).
    let player_entity = {
        let player = active_mission
            .mission_core
            .world
            .borrow::<shipyard::UniqueView<crate::mission::PlayerInfo>>()
            .unwrap();
        player.entity_id
    };
    active_mission.mission_core.world.add_component(
        player_entity,
        dark::properties::PropTeleported::with_source(
            dark::properties::TeleportSource::LoadRestore,
        ),
    );

    (active_mission, save_data.level_data)
}

fn is_cutscene_mission(name: &str) -> bool {
    let normalized = name.trim().to_ascii_lowercase();
    normalized.ends_with(".avi") || normalized.ends_with(".ogv")
}

fn resolve_cutscene_path(name: &str) -> PathBuf {
    resolve_cutscene_path_from(&paths::data_root(), name)
}

fn resolve_cutscene_path_from(data_root: &Path, name: &str) -> PathBuf {
    let trimmed = name.trim();
    let raw_path = Path::new(trimmed);

    if raw_path.is_absolute() {
        return raw_path.to_path_buf();
    }

    let begins_with_cutscenes = raw_path
        .components()
        .next()
        .is_some_and(|component| component.as_os_str().eq_ignore_ascii_case("cutscenes"));
    if begins_with_cutscenes {
        data_root.join(raw_path)
    } else {
        data_root.join("cutscenes").join(raw_path)
    }
}

/// Resolve the retail ending for both 25th Anniversary (`enhanced/cs3.ogv`)
/// and classic (`cs3.avi`) installs.
pub(crate) fn resolve_ending_cutscene() -> (String, PathBuf) {
    ENDING_CUTSCENE_CANDIDATES
        .iter()
        .map(|name| ((*name).to_string(), resolve_cutscene_path(name)))
        .find(|(_, path)| path.is_file())
        .unwrap_or_else(|| {
            let name = ENDING_CUTSCENE_CANDIDATES[0].to_string();
            let path = resolve_cutscene_path(&name);
            (name, path)
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_classic_and_enhanced_cutscene_extensions() {
        assert!(is_cutscene_mission("cs3.avi"));
        assert!(is_cutscene_mission("enhanced/cs3.ogv"));
        assert!(!is_cutscene_mission("shodan.mis"));
    }

    #[test]
    fn nested_cutscene_names_resolve_below_the_cutscene_directory() {
        let root = Path::new("/retail");

        assert_eq!(
            resolve_cutscene_path_from(root, "enhanced/cs3.ogv"),
            root.join("cutscenes/enhanced/cs3.ogv")
        );
        assert_eq!(
            resolve_cutscene_path_from(root, "cutscenes/enhanced/cs3.ogv"),
            root.join("cutscenes/enhanced/cs3.ogv")
        );
    }
}
