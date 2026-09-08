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
    scripts::GlobalEffect,
};

pub mod cutscene_player;
pub mod cutscene_skip;
pub mod debug_camera;
pub mod debug_common;
pub mod debug_gloves;
pub mod debug_hand_poses;
pub mod debug_hitbox;
pub mod debug_hud;
pub mod debug_interactions;
pub mod debug_joint_constraint;
pub mod debug_ladder;
pub mod debug_map;
pub mod debug_melee;
pub mod debug_minimal;
pub mod debug_particles;
pub mod debug_protocol_droid;
pub mod debug_psi;
pub mod debug_ragdoll;
pub mod debug_teleport;
pub mod debug_turret;
pub mod debug_weapons;
pub mod developer;
pub mod game_over;
pub mod load_game;
pub mod loading;
pub mod main_menu;
pub mod no_assets;

pub use cutscene_player::CutscenePlayerScene;
pub use debug_camera::DebugCameraScene;
pub use debug_gloves::DebugGlovesScene;
pub use debug_hand_poses::DebugHandPosesScene;
pub use debug_hitbox::DebugHitboxScene;
pub use debug_hud::DebugHudScene;
pub use debug_joint_constraint::DebugJointConstraintScene;
pub use debug_ladder::create_debug_ladder_scene;
pub use debug_map::DebugMapScene;
pub use debug_melee::create_debug_melee_scene;
pub use debug_minimal::DebugMinimalScene;
pub use debug_particles::DebugParticlesScene;
pub use debug_protocol_droid::DebugProtocolDroidScene;
pub use debug_psi::create_debug_psi_scene;
pub use debug_ragdoll::DebugRagdollScene;
pub use debug_teleport::DebugTeleportScene;
pub use debug_turret::DebugTurretScene;
pub use debug_weapons::create_debug_weapons_scene;
pub use developer::DeveloperScene;
pub use game_over::GameOverScene;
pub use load_game::LoadGameScene;
pub use loading::LoadingScene;
pub use main_menu::MainMenuScene;
pub use no_assets::NoAssetsScene;

/// The minimal world a non-mission screen (menu, loading, game over) needs.
///
/// These scenes have no simulation, but the shared save/transition machinery
/// still runs `to_save_data` on the outgoing scene's world, so the uniques it
/// reads must exist.
pub(crate) fn ui_scene_world() -> shipyard::World {
    use cgmath::{Quaternion, vec3};

    use crate::{
        inventory::PlayerInventoryEntity,
        mission::{GlobalEntityMetadata, GlobalTemplateIdMap, PlayerInfo},
        quest_info::QuestInfo,
        time::Time,
    };

    let mut world = shipyard::World::new();
    let player_entity = world.add_entity(());
    let inventory_entity = PlayerInventoryEntity::create(&mut world);
    PlayerInventoryEntity::set_position_rotation(
        &mut world,
        vec3(0.0, -1000.0, 0.0),
        Quaternion::new(1.0, 0.0, 0.0, 0.0),
    );
    world.add_unique(PlayerInfo {
        pos: vec3(0.0, 0.0, 0.0),
        rotation: Quaternion::new(1.0, 0.0, 0.0, 0.0),
        entity_id: player_entity,
        left_hand_entity_id: None,
        right_hand_entity_id: None,
        inventory_entity_id: inventory_entity,
    });
    world.add_unique(QuestInfo::new());
    world.add_unique(GlobalTemplateIdMap(HashMap::new()));
    world.add_unique(GlobalEntityMetadata(HashMap::new()));
    world.add_unique(Time::default());
    world
}

pub struct SceneInitResult {
    pub scene: Box<dyn GameScene>,
    pub mission_save_data: HashMap<String, EntitySaveData>,
}

// Movie names are not present in the mission or gamesys data - the original
// picked them in its engine/game-script code - so the finale's videos are named
// here.
const ENDING_CUTSCENE_CANDIDATES: &[&str] = &["enhanced/cs3.ogv", "cs3.avi", "cs3.ogv"];
const CREDITS_CUTSCENE_CANDIDATES: &[&str] =
    &["enhanced/credits.ogv", "credits.avi", "credits.ogv"];
const ANNIVERSARY_CUTSCENE_LAYERS: &[&str] = &["enhanced", "original", "kex"];

/// How a debug scene is built. Every entry in [`DEBUG_SCENES`] has this shape,
/// so a scene that needs none of the arguments simply ignores them.
type DebugSceneCtor = fn(
    &GlobalContext,
    &GameOptions,
    &mut AssetCache,
    &mut AudioContext<EntityId, String>,
) -> Box<dyn GameScene>;

/// Every debug scene, by the name that launches it.
///
/// The single source of truth: [`create_debug_scene`] dispatches from this
/// table and the Developer screen's launcher lists it, so a scene added here
/// is both reachable by name (`--mission debug_x`) and offered in the launcher
/// - they cannot drift apart the way a hand-copied menu list would.
const DEBUG_SCENES: &[(&str, DebugSceneCtor)] = &[
    // Visual-only scene to inspect the missing-assets screen. The real one is
    // shown by the runtimes *instead of* a `Game`, since it exists precisely
    // when there is no gamesys to build one from - which also means it cannot
    // be reached on a machine that has the data. This entry renders the same
    // scene against a stand-in status so the screen can be render-verified in
    // both presentations without uninstalling the game.
    ("debug_no_assets", |_global, _options, _assets, _audio| {
        // A stand-in root, deliberately NOT the real one. On a machine that has
        // the data, `paths::data_root()` points at a directory that is not in
        // fact missing anything, so the message would be nonsense - and it puts
        // a local home-directory path (with the developer's username) into every
        // screenshot taken of this scene.
        let status = crate::install::InstallStatus {
            data_root: std::path::PathBuf::from("/sdcard/shock2quest"),
            kind: crate::install::InstallKind::Missing,
            found: Vec::new(),
            missing_mods: Vec::new(),
        };
        Box::new(NoAssetsScene::new(&status))
    }),
    // Visual-only scene to inspect the loading screen UI
    // (projects/loading-screen.md, PR 1) independent of any real loading.
    ("debug_loading", |_global, _options, _assets, _audio| {
        Box::new(LoadingScene::new_demo())
    }),
    ("debug_minimal", |global, options, assets, audio| {
        Box::new(DebugMinimalScene::create(global, options, assets, audio))
    }),
    ("debug_weapons", create_debug_weapons_scene),
    ("debug_melee", create_debug_melee_scene),
    (
        "debug_interactions",
        debug_interactions::create_debug_interactions_scene,
    ),
    ("debug_ladder", create_debug_ladder_scene),
    ("debug_psi", create_debug_psi_scene),
    ("debug_teleport", |global, options, assets, audio| {
        Box::new(DebugTeleportScene::create(global, options, assets, audio))
    }),
    ("debug_camera", DebugCameraScene::new),
    ("debug_protocol_droid", DebugProtocolDroidScene::new),
    ("debug_turret", DebugTurretScene::new),
    ("debug_hud", |_global, _options, _assets, _audio| {
        Box::new(DebugHudScene::new())
    }),
    ("debug_gloves", DebugGlovesScene::new),
    ("debug_hand_poses", DebugHandPosesScene::new),
    ("debug_joint_constraint", DebugJointConstraintScene::new),
    ("debug_map", |_global, _options, _assets, _audio| {
        Box::new(DebugMapScene::new())
    }),
    ("debug_ragdoll", DebugRagdollScene::new),
    ("debug_particles", DebugParticlesScene::new),
    ("debug_hitbox", DebugHitboxScene::new),
];

/// The debug scenes' names, in the order the launcher lists them.
pub fn debug_scene_names() -> impl Iterator<Item = &'static str> {
    DEBUG_SCENES.iter().map(|(name, _)| *name)
}

/// Build the debug scene called `name`, or `None` if it is not one.
///
/// Both entry points come through here: the runtime's `--mission debug_x` (via
/// [`create_initial_scene`]) and the Developer screen's launcher
/// (`GlobalEffect::LaunchDebugScene`), so a scene behaves identically however
/// it was started.
pub fn create_debug_scene(
    name: &str,
    global_context: &GlobalContext,
    options: &GameOptions,
    asset_cache: &mut AssetCache,
    audio_context: &mut AudioContext<EntityId, String>,
) -> Option<Box<dyn GameScene>> {
    DEBUG_SCENES
        .iter()
        .find(|(scene_name, _)| name.eq_ignore_ascii_case(scene_name))
        .map(|(_, create)| create(global_context, options, asset_cache, audio_context))
}

pub fn create_initial_scene(
    asset_cache: &mut AssetCache,
    audio_context: &mut AudioContext<EntityId, String>,
    global_context: &GlobalContext,
    options: &GameOptions,
) -> SceneInitResult {
    if is_cutscene_file(&options.mission) {
        let mission_name = options.mission.clone();
        let cutscene =
            CutscenePlayerScene::new(mission_name, GlobalEffect::ShowMainMenu, audio_context)
                // An explicitly requested cutscene that cannot be opened is a
                // bad invocation, so fail loudly rather than boot elsewhere.
                .unwrap_or_else(|err| panic!("{err}"));
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

    if let Some(scene) = create_debug_scene(
        &options.mission,
        global_context,
        options,
        asset_cache,
        audio_context,
    ) {
        return SceneInitResult {
            scene,
            mission_save_data: HashMap::new(),
        };
    }

    if let Some(save_file_path) = &options.save_file {
        let mut file = OpenOptions::new().read(true).open(save_file_path).unwrap();
        let save_data = SaveData::read(&mut file).unwrap();
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
    let active_healing = save_data.global_data.active_healing.clone();
    let active_radiation = save_data.global_data.active_radiation.clone();

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
    if let Ok(mut healing) = active_mission
        .mission_core
        .world
        .borrow::<shipyard::UniqueViewMut<crate::scripts::healing_item::ActiveHealing>>()
    {
        *healing = active_healing;
    }
    if let Ok(mut radiation) = active_mission
        .mission_core
        .world
        .borrow::<shipyard::UniqueViewMut<crate::scripts::radiation::ActiveRadiation>>()
    {
        *radiation = active_radiation;
    }

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

pub fn is_cutscene_file(name: &str) -> bool {
    let normalized = name.trim().to_ascii_lowercase();
    normalized.ends_with(".avi") || normalized.ends_with(".ogv")
}

/// Resolve a classic or 25th Anniversary cutscene name to a loose video file.
///
/// Exact paths retain priority. When a classic `.avi` is absent, Anniversary
/// `.ogv` files are searched in the game's preferred layer order: enhanced
/// videos first, then original, then KEX-specific videos.
pub fn resolve_cutscene_path(name: &str) -> PathBuf {
    resolve_cutscene_path_from(&paths::data_root(), name)
}

fn resolve_cutscene_path_from(data_root: &Path, name: &str) -> PathBuf {
    let trimmed = name.trim();
    let raw_path = Path::new(trimmed);

    if raw_path.is_absolute() {
        return find_requested_cutscene(raw_path).unwrap_or_else(|| raw_path.to_path_buf());
    }

    // dark_viewer historically accepts a path relative to its working
    // directory. Keep that behavior in the shared resolver so the tool and
    // game select the same file for any given request.
    if let Some(path) = find_requested_cutscene(raw_path) {
        return path;
    }

    let mut components = raw_path.components();
    let relative_cutscene_path = if components
        .next()
        .is_some_and(|component| component.as_os_str().eq_ignore_ascii_case("cutscenes"))
    {
        components.as_path()
    } else {
        raw_path
    };
    let rooted_path = data_root.join("cutscenes").join(relative_cutscene_path);

    if let Some(path) = find_requested_cutscene(&rooted_path) {
        return path;
    }

    // An explicit layer remains explicit. Layer fallback is only for the
    // classic bare names authored by the game, such as `Intro.avi`.
    if relative_cutscene_path.components().count() == 1
        && is_cutscene_file(relative_cutscene_path.to_string_lossy().as_ref())
    {
        let file_stem = relative_cutscene_path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        let anniversary_name = format!("{file_stem}.ogv");

        for layer in ANNIVERSARY_CUTSCENE_LAYERS {
            let candidate = data_root
                .join("cutscenes")
                .join(layer)
                .join(&anniversary_name);
            if let Some(path) = find_file_ignoring_ascii_case(&candidate) {
                return path;
            }
        }
    }

    rooted_path
}

fn find_requested_cutscene(path: &Path) -> Option<PathBuf> {
    find_file_ignoring_ascii_case(path).or_else(|| {
        path.extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("avi"))
            .then(|| path.with_extension("ogv"))
            .and_then(|candidate| find_file_ignoring_ascii_case(&candidate))
    })
}

fn find_file_ignoring_ascii_case(path: &Path) -> Option<PathBuf> {
    if path.is_file() {
        return Some(path.to_path_buf());
    }

    let requested_name = path.file_name()?;
    path.parent()?
        .read_dir()
        .ok()?
        .filter_map(Result::ok)
        .find(|entry| entry.file_name().eq_ignore_ascii_case(requested_name))
        .map(|entry| entry.path())
}

/// Resolve the retail ending for both 25th Anniversary (`enhanced/cs3.ogv`)
/// and classic (`cs3.avi`) installs.
pub(crate) fn resolve_ending_cutscene() -> String {
    first_present_cutscene(ENDING_CUTSCENE_CANDIDATES)
        .unwrap_or_else(|| ENDING_CUTSCENE_CANDIDATES[0].to_string())
}

/// The credits roll that follows the ending, or `None` on an install that ships
/// without one (the finale then returns straight to the menu).
pub(crate) fn resolve_credits_cutscene() -> Option<String> {
    first_present_cutscene(CREDITS_CUTSCENE_CANDIDATES)
}

/// What the ending cutscene hands off to: the credits roll when the install
/// ships one, then the main menu either way.
pub(crate) fn finale_follow_on(credits: Option<String>) -> GlobalEffect {
    match credits {
        Some(video) => GlobalEffect::ShowMainMenu.after_cutscene(&video),
        None => GlobalEffect::ShowMainMenu,
    }
}

/// Every cutscene the install ships, as bare names ready for
/// [`resolve_cutscene_path`] (e.g. `cs1.avi`), sorted and deduplicated.
///
/// A 25th Anniversary install layers the same clip under `enhanced/`,
/// `original/` and `kex/`, so the layers are folded into one entry per stem and
/// the bare name lets the resolver pick its preferred layer at playback -
/// exactly what an authored moment gets.
pub(crate) fn cutscene_names() -> Vec<String> {
    cutscene_names_from(&paths::data_root())
}

fn cutscene_names_from(data_root: &Path) -> Vec<String> {
    let root = data_root.join("cutscenes");
    let mut stems: Vec<String> = std::iter::once(root.clone())
        .chain(
            ANNIVERSARY_CUTSCENE_LAYERS
                .iter()
                .map(|layer| root.join(layer)),
        )
        .filter_map(|dir| dir.read_dir().ok())
        .flatten()
        .filter_map(Result::ok)
        .filter(|entry| is_cutscene_file(&entry.file_name().to_string_lossy()))
        .filter_map(|entry| {
            Path::new(&entry.file_name())
                .file_stem()
                .map(|stem| stem.to_string_lossy().to_ascii_lowercase())
        })
        .collect();
    stems.sort();
    stems.dedup();
    stems
        .into_iter()
        .map(|stem| format!("{stem}.avi"))
        .collect()
}

fn first_present_cutscene(candidates: &[&str]) -> Option<String> {
    candidates
        .iter()
        .find(|name| resolve_cutscene_path(name).is_file())
        .map(|name| (*name).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The launcher's list folds an Anniversary install's layers into one
    /// entry per clip and hands back names the resolver understands - a name
    /// per layer would offer the same movie three times.
    #[test]
    fn the_cutscene_list_folds_the_anniversary_layers_into_one_entry_each() {
        let root = crate::test_support::TempDir::new("cutscenes");
        let cutscenes = root.path().join("cutscenes");
        for (layer, name) in [
            ("enhanced", "cs1.ogv"),
            ("original", "cs1.ogv"),
            ("kex", "Kex.ogv"),
            ("", "intro.avi"),
            // Not a video: the list must not offer it.
            ("", "notes.txt"),
        ] {
            let dir = cutscenes.join(layer);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join(name), b"").unwrap();
        }

        assert_eq!(
            cutscene_names_from(root.path()),
            vec!["cs1.avi", "intro.avi", "kex.avi"]
        );
    }

    /// The finale always ends up at the menu, through the credits when the
    /// install has them and directly when it does not.
    #[test]
    fn the_finale_chains_through_the_credits_only_when_they_are_installed() {
        match finale_follow_on(Some("enhanced/credits.ogv".to_string())) {
            GlobalEffect::PlayCutscene { video, then } => {
                assert_eq!(video, "enhanced/credits.ogv");
                assert!(matches!(*then, GlobalEffect::ShowMainMenu));
            }
            other => panic!("expected the credits to play, got {other:?}"),
        }

        assert!(matches!(finale_follow_on(None), GlobalEffect::ShowMainMenu));
    }

    /// The launcher offers exactly what the dispatcher can build, and every
    /// name is a distinct `debug_` name the CLI accepts too.
    #[test]
    fn the_debug_scene_registry_is_one_list_of_distinct_launchable_names() {
        let names: Vec<&str> = debug_scene_names().collect();
        assert_eq!(names.len(), DEBUG_SCENES.len());
        assert!(names.len() > 1);
        for name in &names {
            assert!(name.starts_with("debug_"), "'{name}' is not a debug scene");
            assert_eq!(
                names.iter().filter(|other| *other == name).count(),
                1,
                "'{name}' is listed twice"
            );
        }
        // A few the docs promise by name, so a rename shows up here.
        for expected in ["debug_ragdoll", "debug_hud", "debug_weapons"] {
            assert!(names.contains(&expected), "'{expected}' is missing");
        }
        // Nothing else answers to a debug name: an unknown one is not a scene,
        // and the lookup is case-insensitive like the rest of the dispatcher.
        assert!(
            DEBUG_SCENES
                .iter()
                .any(|(name, _)| "DEBUG_Ragdoll".eq_ignore_ascii_case(name))
        );
        assert!(
            !DEBUG_SCENES
                .iter()
                .any(|(name, _)| "debug_nonexistent".eq_ignore_ascii_case(name))
        );
    }

    /// A scratch data root that removes itself after each resolver test.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir()
                .join(format!("shock2vr-cutscenes-{}-{name}", std::process::id()));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }

        fn write(&self, relative_path: &str) -> PathBuf {
            let path = self.0.join(relative_path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, b"cutscene fixture").unwrap();
            path
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn recognizes_classic_and_enhanced_cutscene_extensions() {
        assert!(is_cutscene_file("cs3.avi"));
        assert!(is_cutscene_file("enhanced/cs3.ogv"));
        assert!(!is_cutscene_file("shodan.mis"));
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

    #[test]
    fn classic_name_resolves_to_anniversary_ogv_counterpart() {
        let root = TempDir::new("anniversary-counterpart");
        let expected = root.write("cutscenes/original/intro.ogv");

        assert_eq!(
            resolve_cutscene_path_from(root.path(), "Intro.avi"),
            expected
        );
    }

    #[test]
    fn enhanced_anniversary_layer_precedes_original() {
        let root = TempDir::new("anniversary-precedence");
        let expected = root.write("cutscenes/enhanced/cs1.ogv");
        root.write("cutscenes/original/cs1.ogv");

        assert_eq!(resolve_cutscene_path_from(root.path(), "cs1.avi"), expected);
    }
}
