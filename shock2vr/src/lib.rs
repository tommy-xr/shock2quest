pub mod audio_log;
pub mod game_scene;
pub mod hand_pose;
pub mod input;
pub mod input_context;
pub mod inventory;
pub mod map_renderer;
pub mod save_load;
pub mod scenes;
pub mod teleport;
pub mod time;

pub mod career;
mod creature;
mod flat_player_controller;
mod gui;
mod hand_glove;
mod hud;
mod interaction;
mod mission;
pub mod palette;
pub mod pathfinding;
pub mod paths;
mod physics;
pub mod player_stats;
mod psi;
pub mod quest_info;
mod runtime_props;
mod scripts;
mod systems;
mod ui;
mod util;
mod virtual_hand;
mod vr_config;
pub mod zip_asset_path;

use scenes::{SceneInitResult, create_initial_scene, load_mission_from_save_data};

pub use mission::SpawnLocation;
pub use mission::visibility_engine::CullingInfo;

/// Player eye (camera) height above the body/feet position, in SS2 units
/// (before the `dark::SCALE_FACTOR` world-scale divide). Shared by every
/// runtime's render camera (`head_offset`) AND the flat controller's
/// shot/viewmodel origin, so the rendered view and where shots come from stay
/// consistent - if these drift, shots no longer line up with the crosshair and
/// the debug runtime renders at a different height than desktop. Crouch swaps
/// this for [`PLAYER_CROUCH_EYE_HEIGHT`] - flat runtimes should use
/// [`Game::player_eye_height`] rather than reading the constants directly.
pub const PLAYER_EYE_HEIGHT: f32 = 4.0;

/// Crouched eye height above the (crouched) body position, in SS2 units.
/// The crouched capsule is 2.8 ft tall with its center 1.4 ft above the feet;
/// +1.2 puts the eye at 2.6 ft above the feet - ~90% of crouched body height
/// like the original engine's crouch, and inside the capsule so the camera
/// cannot poke through a low ceiling the collider clears.
pub const PLAYER_CROUCH_EYE_HEIGHT: f32 = 1.2;

/// The single mapping from crouch state to eye height. The render camera and
/// the flat controller's shot/viewmodel origin must both go through this (or
/// [`Game::player_eye_height`], which wraps it) so shots stay on the
/// crosshair.
pub fn player_eye_height_for(crouched: bool) -> f32 {
    if crouched {
        PLAYER_CROUCH_EYE_HEIGHT
    } else {
        PLAYER_EYE_HEIGHT
    }
}

use std::{
    collections::{HashMap, HashSet},
    fs::{File, OpenOptions},
    io::BufReader,
    path::Path,
    rc::Rc,
    sync::Arc,
    thread,
};

use cgmath::{Matrix4, Quaternion, Vector2, Vector3, vec3};
use dark::{
    gamesys,
    importers::{AUDIO_IMPORTER, FONT_IMPORTER, STRINGS_IMPORTER},
    motion::MotionDB,
};
use engine::{
    assets::{asset_cache::AssetCache, asset_paths::AssetPath, bundle_asset_path::BundleAssetPath},
    audio::{AudioClip, AudioContext},
    file_system::Storage,
    game_log,
    scene::SceneObject,
};

use mission::entity_populator::{EntityPopulator, MissionEntityPopulator, SaveFileEntityPopulator};
use quest_info::QuestInfo;

use save_load::{EntitySaveData, GlobalData, HeldItemSaveData, PlayerVitals, SaveData};
use scenes::LoadingScene;
use scripts::{GlobalEffect, PlayerVitalsTransition};
use shipyard::*;
use time::Time;
use tracing::{Level, info, span, trace, warn};

use crate::{
    game_scene::GameScene,
    mission::{GlobalContext, Mission, PlayerInfo},
    scripts::Effect,
};
use zip_asset_path::ZipAssetPath;

/// Whether `data_root` is a 25th Anniversary Edition install rather than a
/// classic one. The remaster ships its data inside `sshock2.kpf`; a classic
/// install has loose `.crf` archives instead.
pub fn is_25th_anniversary_install() -> bool {
    Path::new(&resource_path("sshock2.kpf")).exists()
}

/// Resource families, in the order a lookup should consult them.
///
/// This is the same precedence the classic `.crf` mount list uses: `obj` first
/// so a model material resolves to a model texture, `iface` and friends after.
const RESOURCE_FAMILIES: &[&str] = &[
    "obj", "bitmap", "book", "fam", "iface", "intrface", "mesh", "motions", "objicon", "snd",
    "snd2", "song", "strings",
];

/// The 25AE mod stack, highest priority first, exactly as
/// `base.kpf:defaults/cam_mod.ini` declares it.
///
/// `sshock2ee` (the Nightdive layer) is deliberately excluded for `strings`:
/// 41 of its 42 string tables are `$`-token stubs that KEX resolves through
/// `localization/loc_english.txt`, which we do not implement - honouring them
/// renders raw keys like `$PSI6` in place of real text.
const MOD_ARCHIVES: &[&str] = &[
    "mods/sshock2ee.kpf",
    "mods/400.kpf",
    "mods/shtup.kpf",
    "mods/scp.kpf",
    "mods/patch_ext.kpf",
];

/// Whether a mod layer is allowed to override this resource family.
///
/// Two deliberate carve-outs, both of which would otherwise be *regressions* on
/// the modded path rather than upgrades:
///
/// - `strings` from the Nightdive layer: 41 of its 42 tables are `$`-token stubs
///   that KEX resolves through `localization/loc_english.txt`, which we do not
///   implement, so honouring them renders raw keys like `$PSI6`.
/// - `fam` (terrain) from any layer: the upgraded terrain textures are
///   higher-resolution than the originals and carry `terrain_scale` in their
///   `.mtl`, which we do not read yet. Taking the texture without the scale
///   tiles the world wrong. Lift this once the `.mtl` scale subset lands.
fn mod_layer_may_override(family: &str, archive: &str) -> bool {
    match family {
        "strings" => !archive.contains("sshock2ee"),
        "fam" => false,
        _ => true,
    }
}

fn build_25th_anniversary_mounts(
    bundle_storage: Arc<dyn Storage>,
) -> Vec<Box<dyn engine::assets::asset_paths::AbstractAssetPath>> {
    let mut mounts: Vec<Box<dyn engine::assets::asset_paths::AbstractAssetPath>> = Vec::new();

    for family in RESOURCE_FAMILIES {
        for archive in MOD_ARCHIVES {
            if !mod_layer_may_override(family, archive) {
                continue;
            }
            let path = resource_path(archive);
            if Path::new(&path).exists() {
                mounts.push(ZipAssetPath::with_prefix(path, &format!("{family}/")));
            }
        }
        // The base archive keeps the original `data/res/<family>/` layout.
        mounts.push(ZipAssetPath::with_prefix(
            resource_path("sshock2.kpf"),
            &format!("data/res/{family}/"),
        ));
    }

    // `motiondb.bin` moved from the data root to `res/mschema/`, and the
    // missions + gamesys live under `data/`.
    mounts.push(ZipAssetPath::with_prefix(
        resource_path("sshock2.kpf"),
        "data/res/mschema/",
    ));
    mounts.push(ZipAssetPath::with_prefix(
        resource_path("sshock2.kpf"),
        "data/",
    ));

    mounts.push(BundleAssetPath::new("".to_owned(), bundle_storage));
    mounts.push(AssetPath::folder("".to_owned()));
    mounts
}

pub fn resource_path(str: &str) -> String {
    paths::data_root().join(str).to_string_lossy().into_owned()
}

/// Resolve a bare save name to its on-disk `.sav` path under `<data_root>/saves`.
///
/// `name` is expected to be a bare name (no extension / path separators - the
/// caller validates that at the edge). Named saves live in a stable directory
/// keyed off the data root so a save written in one runtime launch can be
/// reloaded in a later launch (frontier persistence for automated play-through
/// loops).
pub fn save_file_path(name: &str) -> std::path::PathBuf {
    paths::data_root().join("saves").join(format!("{name}.sav"))
}

/// How the game is presented and controlled.
///
/// `Vr` is the existing head/hands interaction model (forearm HUD panels,
/// `VirtualHand` grab-and-hold). `Flat` is the classic flatscreen presentation:
/// a screen-space 2D HUD, 2D menus, and first-person interaction. The flag is
/// threaded only to the presentation/interaction edges - the simulation is
/// shared. See `projects/flatscreen-and-vr-architecture.md`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum PresentationMode {
    #[default]
    Vr,
    Flat,
}

pub struct GameOptions {
    pub mission: String,
    pub presentation_mode: PresentationMode,
    pub spawn_location: SpawnLocation,
    pub save_file: Option<String>,
    pub render_particles: bool,
    pub debug_physics: bool,
    pub debug_draw: bool,
    pub debug_portals: bool,
    pub debug_show_ids: bool,
    pub debug_skeletons: bool,
    pub debug_ai: bool,
    pub debug_pathfinding: bool,
    pub experimental_features: HashSet<String>,
}

impl Default for GameOptions {
    fn default() -> Self {
        Self {
            mission: "earth.mis".to_owned(),
            presentation_mode: PresentationMode::Vr,
            spawn_location: SpawnLocation::MapDefault,
            save_file: None,
            debug_draw: false,
            debug_portals: false,
            debug_physics: false,
            debug_show_ids: false,
            debug_skeletons: false,
            debug_ai: false,
            debug_pathfinding: false,
            render_particles: true,
            experimental_features: HashSet::new(),
        }
    }
}

/// A level transition that has been started (outgoing scene already saved, loading
/// screen showing) and whose blocking load is deferred to the next `update` frame.
/// Only used when the experimental `loading_screen` feature is enabled.
struct PendingTransition {
    level_name: String,
    spawn_loc: SpawnLocation,
    entities_to_trigger: Vec<String>,
    quest_info: QuestInfo,
    held_data: HeldItemSaveData,
    player_vitals: Option<PlayerVitals>,
    /// The CPU parse running on a worker thread. While this is in flight the loading
    /// screen renders (and animates) every frame; once it finishes, the main thread runs
    /// the GPU `build` and swaps to the mission.
    parse_handle: thread::JoinHandle<dark::mission::SystemShock2Level>,
    /// Frames the loading screen has rendered so far (frame-counted, so it works under
    /// the debug runtime's zero-dt stepping). A minimum display so the loading screen is
    /// shown for a perceptible moment even if the parse finishes very quickly.
    frames_shown: u32,
}

/// Minimum number of frames to show the loading screen before swapping to the mission,
/// even if the background parse finished sooner. ~0.4s at 60fps.
const MIN_LOADING_FRAMES: u32 = 24;

pub struct Game {
    options: GameOptions,
    pub asset_cache: AssetCache,
    // `Arc` so the gamesys + definitions can be cloned into a background level-parse
    // thread (projects/loading-screen.md). Shared immutably after init.
    global_context: Arc<GlobalContext>,
    active_game_scene: Box<dyn GameScene>,
    /// Set while a deferred transition is in flight (loading screen showing).
    pending_transition: Option<PendingTransition>,
    // physics: PhysicsWorld,
    // script_world: ScriptWorld,
    audio_context: AudioContext<EntityId, String>,
    // id_to_scene_objects: HashMap<EntityId, Vec<RefCell<SceneObject>>>,
    // id_to_physics: HashMap<EntityId, RigidBodyHandle>,
    // scene_objects: Vec<RefCell<SceneObject>>,
    //world: World,
    last_music_cue: Option<String>,
    last_env_sound: Option<String>,

    mission_to_save_data: HashMap<String, EntitySaveData>,

    // Set when a scene requests quitting (e.g. the main menu's Quit). The
    // runtime observes this via `should_quit` and closes its window.
    should_quit: bool,
}

/// Player state for debug introspection. Entity ids use `EntityId::inner() as
/// i32`, matching the debug runtime's entity endpoints. In flatscreen mode
/// `wielded_entity_id` is the first-person weapon (the controller wields into the
/// player's "left hand" slot); in VR the two hand slots hold whatever is grabbed.
#[derive(Clone, Debug)]
pub struct PlayerStateSnapshot {
    pub entity_id: i32,
    pub position: [f32; 3],
    pub rotation: [f32; 4],
    pub wielded_entity_id: Option<i32>,
    pub right_hand_entity_id: Option<i32>,
    /// Whether the wielded weapon is mid-reload, and (if so) the current
    /// reload RAMP angle in degrees (0 -> authored peak -> 0) and the reload
    /// progress (0..1). When not reloading: `false`, `0.0`, `0.0`. Lets tooling
    /// verify the reload animation headlessly. NB: the applied viewmodel pitch
    /// is not this value verbatim - the flat controller interpolates from its
    /// carry pitch to the peak using this ramp (see `FlatPlayerController`).
    pub reloading: bool,
    pub reload_pitch_deg: f32,
    pub reload_progress: f32,
    /// The wielded weapon's selected ammo type (`ammotype` class-tag, e.g. "std"
    /// / "he" / "ap"), or `None` when unarmed or the weapon has no projectile
    /// links (melee). Reported whenever there is a projectile link, even if there
    /// is only one (it just cannot be cycled).
    pub wielded_ammo_type: Option<String>,
    /// The player's hit points (current, max), or `None` when the player has
    /// no health pool. Seeded from `The Player` template; drained by psi
    /// burnout (real damage handling is still TODO).
    pub hit_points: Option<(i32, i32)>,
    /// The player's psi pool (current, max), or `None` when the player has no
    /// psi state (e.g. gamesys not loaded).
    pub psi_points: Option<(i32, i32)>,
    /// The gamesys name of the currently selected psi power (what the psi amp
    /// casts), e.g. "Cryokinesis".
    pub selected_psi_power: Option<String>,
    /// The psi amp's hold-to-overload meter: `(fraction 0..1, phase)` where
    /// phase is "charging" / "overloaded" / "burnout". `None` when no charge
    /// is in progress.
    pub psi_charge: Option<(f32, &'static str)>,
    /// The gamesys names of the currently active sustained psi powers (e.g.
    /// "Inviso"), in activation order. Empty when none are active.
    pub active_psi_powers: Vec<String>,
    /// The player's persistent character sheet (primary stats, skills, mastered
    /// psi disciplines), accumulated from career + training tours. `None` when
    /// the scene has no `QuestInfo` (e.g. a menu). See `crate::player_stats`.
    pub stats: Option<crate::player_stats::PlayerStats>,
    /// The audio logs the player has collected (frobbed), in pickup order. Empty
    /// when the scene has no `QuestInfo`. Persisted in `QuestInfo`, so it
    /// survives level transitions and save/load. See `crate::quest_info`.
    pub collected_logs: Vec<crate::quest_info::CollectedLog>,
    /// The automap locations explored in the current mission (ascending).
    /// Empty when the scene has no `QuestInfo` or no automap. Persisted per
    /// mission in `QuestInfo`. See `projects/flat-ui-panels.md` §5.
    pub explored_map_locations: Vec<i32>,
}

impl Game {
    /// A snapshot of the player (position, look rotation, held/wielded entities)
    /// for debug tooling, or `None` if the active scene has no player (e.g. a
    /// menu). Reads the `PlayerInfo` unique from the active world.
    pub fn player_state(&self) -> Option<PlayerStateSnapshot> {
        use crate::mission::mission_core::PlayerInfo;
        use crate::runtime_props::RuntimePropReloading;
        let world = self.world();
        let info = world.borrow::<shipyard::UniqueView<PlayerInfo>>().ok()?;
        let reload = info.left_hand_entity_id.and_then(|weapon| {
            world
                .borrow::<shipyard::View<RuntimePropReloading>>()
                .ok()
                .and_then(|v| {
                    use shipyard::Get;
                    v.get(weapon).ok().map(|r| (r.pitch_deg(), r.progress()))
                })
        });
        Some(PlayerStateSnapshot {
            entity_id: info.entity_id.inner() as i32,
            position: [info.pos.x, info.pos.y, info.pos.z],
            rotation: [
                info.rotation.v.x,
                info.rotation.v.y,
                info.rotation.v.z,
                info.rotation.s,
            ],
            wielded_entity_id: info.left_hand_entity_id.map(|e| e.inner() as i32),
            right_hand_entity_id: info.right_hand_entity_id.map(|e| e.inner() as i32),
            reloading: reload.is_some(),
            reload_pitch_deg: reload.map(|(p, _)| p).unwrap_or(0.0),
            reload_progress: reload.map(|(_, p)| p).unwrap_or(0.0),
            wielded_ammo_type: crate::hud::get_wielded_ammo_type(world),
            hit_points: (|| {
                use dark::properties::{PropHitPoints, PropMaxHitPoints};
                let v_hp = world.borrow::<shipyard::View<PropHitPoints>>().ok()?;
                let v_max = world.borrow::<shipyard::View<PropMaxHitPoints>>().ok()?;
                use shipyard::Get;
                let hp = v_hp.get(info.entity_id).ok()?.hit_points;
                let max = v_max.get(info.entity_id).ok()?.hit_points;
                Some((hp, max as i32))
            })(),
            psi_points: world
                .borrow::<shipyard::View<dark::properties::PropPsiState>>()
                .ok()
                .and_then(|v| {
                    use shipyard::Get;
                    v.get(info.entity_id)
                        .ok()
                        .map(|p| (p.psi_points, p.max_psi_points))
                }),
            selected_psi_power: (|| {
                let powers = world
                    .borrow::<UniqueView<crate::psi::GlobalPsiPowers>>()
                    .ok()?;
                let selection = world
                    .borrow::<UniqueView<crate::psi::PsiPowerSelection>>()
                    .ok()?;
                powers.0.get(selection.index).map(|p| p.name.clone())
            })(),
            psi_charge: info.left_hand_entity_id.and_then(|weapon| {
                use crate::runtime_props::{PsiChargePhase, RuntimePropPsiCharge};
                let v = world
                    .borrow::<shipyard::View<RuntimePropPsiCharge>>()
                    .ok()?;
                use shipyard::Get;
                v.get(weapon).ok().map(|c| {
                    let phase = match c.phase {
                        PsiChargePhase::Charging => "charging",
                        PsiChargePhase::Overloaded => "overloaded",
                        PsiChargePhase::Burnout => "burnout",
                    };
                    (c.fraction, phase)
                })
            }),
            active_psi_powers: world
                .borrow::<UniqueView<crate::psi::ActivePsiPowers>>()
                .map(|active| active.0.iter().map(|p| p.name.clone()).collect())
                .unwrap_or_default(),
            stats: world
                .borrow::<UniqueView<QuestInfo>>()
                .ok()
                .map(|q| q.player_stats().clone()),
            collected_logs: world
                .borrow::<UniqueView<QuestInfo>>()
                .ok()
                .map(|q| q.collected_logs().to_vec())
                .unwrap_or_default(),
            explored_map_locations: world
                .borrow::<UniqueView<QuestInfo>>()
                .ok()
                .map(|q| q.explored_map_locations(self.scene_name()))
                .unwrap_or_default(),
        })
    }

    /// Whether the experimental loading screen (deferred transitions) is enabled.
    fn loading_screen_enabled(&self) -> bool {
        self.options
            .experimental_features
            .contains("loading_screen")
    }

    /// Capture the outgoing scene's save data (so its state survives the transition)
    /// and return the context the new mission needs. Must run while the outgoing scene
    /// is still active.
    fn save_active_scene(&mut self) -> (QuestInfo, HeldItemSaveData, Option<PlayerVitals>) {
        let current_quest_info = self
            .active_game_scene
            .world()
            .borrow::<UniqueView<QuestInfo>>()
            .unwrap()
            .clone();

        let (current_save_data, held_data) =
            save_load::to_save_data(self.active_game_scene.world());
        game_log!(
            DEBUG,
            "Saving {} entities to save data",
            current_save_data.all_entities.len()
        );

        self.mission_to_save_data.insert(
            self.active_game_scene.scene_name().to_ascii_lowercase(),
            current_save_data,
        );

        let player_vitals = save_load::capture_player_vitals(self.active_game_scene.world());

        (current_quest_info, held_data, player_vitals)
    }

    /// Load a mission (using previously-captured save context) and make it the active
    /// scene. This is the blocking part of a transition.
    fn load_mission_into_scene(
        &mut self,
        level_name: String,
        spawn_loc: SpawnLocation,
        quest_info: QuestInfo,
        held_data: HeldItemSaveData,
        player_vitals: Option<PlayerVitals>,
    ) {
        let populator: Box<dyn EntityPopulator> = {
            if let Some(save_data) = self
                .mission_to_save_data
                .get(&level_name.to_ascii_lowercase())
            {
                let save_data_cloned = save_data.clone();
                let populator = SaveFileEntityPopulator::create(save_data_cloned);
                Box::new(populator)
            } else {
                Box::new(MissionEntityPopulator::create())
            }
        };

        let active_mission = Mission::load(
            level_name,
            &mut self.asset_cache,
            &mut self.audio_context,
            &self.global_context,
            spawn_loc,
            quest_info,
            populator,
            held_data,
            &self.options,
        );
        save_load::restore_player_vitals(&active_mission.mission_core.world, player_vitals);
        self.active_game_scene = Box::new(active_mission);
    }

    fn switch_mission(
        &mut self,
        level_name: String,
        spawn_loc: SpawnLocation,
        vitals_transition: PlayerVitalsTransition,
    ) {
        let (quest_info, held_data, mut player_vitals) = self.save_active_scene();
        if vitals_transition == PlayerVitalsTransition::InitializeFromDestination {
            player_vitals = None;
        }
        self.load_mission_into_scene(level_name, spawn_loc, quest_info, held_data, player_vitals);
    }

    /// Start a background transition: save the outgoing scene, spawn the GL-free level
    /// parse on a worker thread, and swap to the loading screen. The loading screen
    /// animates while the parse runs; `update` completes it once the parse finishes.
    fn begin_transition(
        &mut self,
        level_name: String,
        spawn_loc: SpawnLocation,
        entities_to_trigger: Vec<String>,
        vitals_transition: PlayerVitalsTransition,
    ) {
        tracing::info!(
            "[loading-screen] begin_transition -> {} (background parse)",
            level_name
        );
        let (quest_info, held_data, mut player_vitals) = self.save_active_scene();
        if vitals_transition == PlayerVitalsTransition::InitializeFromDestination {
            player_vitals = None;
        }

        // Spawn the GL-free parse off-thread. It owns `Arc`s of the asset-path layer and
        // the global context (gamesys + definitions), all `Send + Sync`, and produces a
        // `Send` `SystemShock2Level`.
        let asset_paths = self.asset_cache.asset_paths_arc();
        let base_path = self.asset_cache.base_path().to_string();
        let global_context = Arc::clone(&self.global_context);
        let parse_name = level_name.clone();
        let parse_handle = thread::spawn(move || {
            Mission::parse(&**asset_paths, &base_path, &parse_name, &global_context)
        });

        self.active_game_scene = Box::new(LoadingScene::new());
        self.pending_transition = Some(PendingTransition {
            level_name,
            spawn_loc,
            entities_to_trigger,
            quest_info,
            held_data,
            player_vitals,
            parse_handle,
            frames_shown: 0,
        });
    }

    /// Complete a background transition queued by [`begin_transition`]: join the finished
    /// parse and run the main-thread GPU `build`, then swap to the mission.
    fn finish_transition(&mut self, pending: PendingTransition) {
        tracing::info!(
            "[loading-screen] finish_transition -> {} (parse done, building)",
            pending.level_name
        );
        let level = pending
            .parse_handle
            .join()
            .expect("background level-parse thread panicked");

        let populator: Box<dyn EntityPopulator> = {
            if let Some(save_data) = self
                .mission_to_save_data
                .get(&pending.level_name.to_ascii_lowercase())
            {
                Box::new(SaveFileEntityPopulator::create(save_data.clone()))
            } else {
                Box::new(MissionEntityPopulator::create())
            }
        };

        let mission = Mission::build(
            level,
            pending.level_name,
            &mut self.asset_cache,
            &mut self.audio_context,
            &self.global_context,
            pending.spawn_loc,
            pending.quest_info,
            populator,
            pending.held_data,
            &self.options,
        );
        save_load::restore_player_vitals(&mission.mission_core.world, pending.player_vitals);
        self.active_game_scene = Box::new(mission);

        for entity_name in pending.entities_to_trigger {
            self.active_game_scene.queue_entity_trigger(entity_name);
        }
    }

    fn switch_mission_with_trigger(
        &mut self,
        level_name: String,
        spawn_loc: SpawnLocation,
        entities_to_trigger: Vec<String>,
        vitals_transition: PlayerVitalsTransition,
    ) {
        // First, switch to the new mission
        self.switch_mission(level_name, spawn_loc, vitals_transition);

        // Then, queue the entities to be triggered after scripts are initialized
        for entity_name in entities_to_trigger {
            println!("Queueing entity trigger for: {}", entity_name);
            self.active_game_scene.queue_entity_trigger(entity_name);
        }
    }

    /// Get access to the world for debugging purposes
    pub fn world(&self) -> &shipyard::World {
        self.active_game_scene.world()
    }

    /// Whether a scene has requested to quit the game (e.g. the main menu's
    /// Quit). Runtimes should observe this and close their window.
    pub fn should_quit(&self) -> bool {
        self.should_quit
    }

    /// Whether the active scene wants a 2D mouse cursor (e.g. a menu). Flat
    /// runtimes use this to show the OS cursor and feed `InputContext::pointer`
    /// rather than capturing the mouse for look.
    pub fn wants_pointer(&self) -> bool {
        self.active_game_scene.wants_pointer()
    }

    /// Name of the currently active scene (e.g. "medsci1.mis"), tracking
    /// level transitions
    pub fn scene_name(&self) -> &str {
        self.active_game_scene.scene_name()
    }

    /// Trigger a level transition programmatically (debug/testing lever).
    ///
    /// Mirrors what an in-game `TrapTripLevel` trigger emits when the player
    /// enters its volume: switch to `level_file` at spawn `loc` (a `PropStartLoc`
    /// marker id, or the map default when `None`). `level_file` should be the
    /// mission filename (e.g. "eng1.mis"). This lets an automated tester warp to
    /// any level in isolation instead of having to physically reach each
    /// transition trigger. With the `loading_screen` feature enabled the switch
    /// is deferred (the outgoing scene renders the loading screen first), so
    /// `scene_name()` only reflects the new level after subsequent updates;
    /// otherwise it is synchronous and observable immediately.
    pub fn transition_level(&mut self, level_file: String, loc: Option<i32>) {
        self.handle_global_effect(GlobalEffect::TransitionLevel {
            level_file,
            loc,
            entities_to_trigger: vec![],
            vitals_transition: PlayerVitalsTransition::Preserve,
        });
    }

    /// Save the current game to a named save file under `<data_root>/saves`.
    ///
    /// `file` is a bare name (no extension / path separators - validate at the
    /// edge); it resolves to `<data_root>/saves/<file>.sav`. This is the same
    /// serialization an in-game quicksave performs (`GlobalEffect::Save`) -
    /// active mission, player position/rotation, quest bits, held items, and
    /// exact current/maximum player vitals.
    /// Returns the scene name that was saved so the caller can report it.
    pub fn save_game(&mut self, file: String) -> String {
        let path = save_file_path(&file);
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        self.handle_global_effect(GlobalEffect::Save {
            file_name: path.to_string_lossy().into_owned(),
        });
        self.scene_name().to_string()
    }

    /// Load a previously-saved game from `<data_root>/saves/<file>.sav`,
    /// restoring the active mission, player position/rotation, quest bits, held
    /// items, and exact current/maximum player vitals. The switch is synchronous
    /// (no loading-screen deferral), so the returned scene name already reflects
    /// the restored mission. The caller must ensure the file exists - the load
    /// path panics on a missing file.
    pub fn load_game(&mut self, file: String) -> String {
        let path = save_file_path(&file);
        self.handle_global_effect(GlobalEffect::Load {
            file_name: path.to_string_lossy().into_owned(),
        });
        self.scene_name().to_string()
    }

    /// Get access to the debug scene interface if available
    ///
    /// Returns a reference to the current scene as a DebuggableScene trait object
    /// if the scene supports debugging capabilities. This provides access to
    /// entity inspection, raycasting, and other debug functionality.
    ///
    /// Returns None if the current scene doesn't implement DebuggableScene.
    pub fn debug_scene(&self) -> Option<&dyn game_scene::DebuggableScene> {
        self.active_game_scene.as_debuggable()
    }

    /// Get mutable access to the debug scene interface if available
    ///
    /// Returns a mutable reference to the current scene as a DebuggableScene
    /// trait object if the scene supports debugging capabilities. This allows
    /// modifying scene state such as teleporting the player.
    ///
    /// Returns None if the current scene doesn't implement DebuggableScene.
    pub fn debug_scene_mut(&mut self) -> Option<&mut dyn game_scene::DebuggableScene> {
        self.active_game_scene.as_debuggable_mut()
    }

    pub fn init(options: GameOptions, bundle_storage: Arc<dyn Storage>) -> Game {
        // A 25th Anniversary install keeps everything inside KPF archives, so it
        // needs a different mount list from a classic install's loose `.crf`s.
        let asset_paths = if is_25th_anniversary_install() {
            info!("25th Anniversary Edition install detected; mounting KPF archives");
            AssetPath::combine(build_25th_anniversary_mounts(bundle_storage.clone()))
        } else {
            AssetPath::combine(vec![
                AssetPath::folder(resource_path("res/mesh")),
                // AssetPath::folder(resource_path("res/mesh/txt16")),
                AssetPath::folder(resource_path("res/obj")),
                // AssetPath::folder(resource_path("res/obj/txt16")),
                ZipAssetPath::new(resource_path("res/obj.crf")),
                ZipAssetPath::new(resource_path("res/bitmap.crf")),
                // Log/email sender portraits + deck icons (the reader panel art).
                ZipAssetPath::new(resource_path("res/book.crf")),
                ZipAssetPath::new(resource_path("res/fam.crf")),
                // Also mounted under the "iface/" namespace: iface.crf shares seven
                // basenames with the obj/bitmap mounts above (access/block/log/
                // plant1/repair/stats.pcx + palette1.pal), and first-mount-wins
                // means those plain names must keep resolving to the model
                // textures. GUI code that wants the interface art requests the
                // archive-qualified "iface/<name>" key instead.
                ZipAssetPath::with_namespace(resource_path("res/iface.crf"), "iface"),
                ZipAssetPath::new(resource_path("res/intrface.crf")),
                ZipAssetPath::new(resource_path("res/mesh.crf")),
                ZipAssetPath::new(resource_path("res/motions.crf")),
                ZipAssetPath::new(resource_path("res/objicon.crf")),
                ZipAssetPath::new(resource_path("res/snd.crf")),
                ZipAssetPath::new(resource_path("res/snd2.crf")),
                ZipAssetPath::new(resource_path("res/song.crf")),
                ZipAssetPath::new2(resource_path("res/strings.crf"), false),
                // Bundle assets
                BundleAssetPath::new("".to_owned(), bundle_storage),
                // Textures
                // AssetPath::folder("res/bitmap".to_owned()),
                // AssetPath::folder("res/bitmap/txt16".to_owned()),
                //AssetPath::folder("res/fam".to_owned()),
                // Models
                // AssetPath::folder("res/mesh/txt16".to_owned()),
                // AssetPath::folder("res/obj".to_owned()),
                // AssetPath::folder("res/mesh".to_owned()),
                // Animations
                //AssetPath::folder("res/motions".to_owned()),
                // Motion db
                AssetPath::folder("".to_owned()),
                // Audio
                // AssetPath::folder("res/snd".to_owned()),
                // AssetPath::folder("res/snd/amb".to_owned()),
                // AssetPath::folder("res/snd/Assassin".to_owned()),
                // AssetPath::folder("res/snd/BBetty".to_owned()),
                // AssetPath::folder("res/snd/Devices".to_owned()),
                // AssetPath::folder("res/snd/GRUB".to_owned()),
                // AssetPath::folder("res/snd/HITS".to_owned()),
                // AssetPath::folder("res/snd/MaintBot/english".to_owned()),
                // AssetPath::folder("res/snd/Midwife/english".to_owned()),
                // AssetPath::folder("res/snd/OGRUNT/english".to_owned()),
                // AssetPath::folder("res/snd/Overlord/english".to_owned()),
                // AssetPath::folder("res/snd/SONGS".to_owned()),
                // AssetPath::folder("res/snd/TurrCam".to_owned()),
                // AssetPath::folder("res/snd/Weapons".to_owned()),
                // AssetPath::folder("res/snd2/vBriefs/ENGLISH".to_owned()),
                // AssetPath::folder("res/snd2/vCs/ENGLISH".to_owned()),
                // AssetPath::folder("res/snd2/vEmails/english".to_owned()),
                // AssetPath::folder("res/snd2/vLogs/english".to_owned()),
                // AssetPath::folder("res/snd2/vTriggers/english".to_owned()),
            ])
        };
        // Global items
        let base_path = paths::data_root().to_string_lossy().into_owned();
        let mut asset_cache = AssetCache::new(base_path, asset_paths);

        // TODO: Start ffmpeg stuff
        #[cfg(feature = "ffmpeg")]
        engine_ffmpeg::init().unwrap();

        let (properties, links, links_with_data) = dark::properties::get();

        // NOT yet routed through the asset paths, unlike `motiondb.bin` below:
        // `dark::properties::get()` is instantiated at `BufReader<File>` and the
        // resulting `PropertyDefinition`s are shared with mission loading, so
        // handing `gamesys::read` a boxed archive reader does not typecheck
        // without making that whole chain generic. Until then a 25AE install
        // needs `shock2.gam` extracted from `sshock2.kpf` alongside it.
        let game_file = File::open(resource_path("shock2.gam")).unwrap();
        let mut game_reader = BufReader::new(game_file);

        let _strings = asset_cache.get(&STRINGS_IMPORTER, "objname.str");

        // vhot logging:
        // let atek_file = File::open(resource_path("res/obj/ar15_w.bin")).unwrap();
        // let mut atek_reader = BufReader::new(atek_file);
        // let header = ss2_bin_header::read(&mut atek_reader);
        // let obj = ss2_bin_obj_loader::read(&mut atek_reader, &header);

        let gamesys = gamesys::read(&mut game_reader, &links, &links_with_data, &properties);

        // Likewise: 25AE moves this to `data/res/mschema/motiondb.bin`.
        let motiondb_reader = asset_cache
            .get_raw_reader("motiondb.bin")
            .expect("motiondb.bin should be present in the mounted data");
        let motiondb = MotionDB::read(&mut *motiondb_reader.borrow_mut());

        let mut audio_context = AudioContext::new();

        let global_context = GlobalContext {
            links,
            links_with_data,
            properties,
            motiondb,
            gamesys,
        };

        // TEST: Load all missions
        // Load all missions:
        // for _x in 0..100 {
        //     let all_missions = vec![
        //         "earth.mis",
        //         "station.mis",
        //         "medsci1.mis",
        //         "medsci2.mis",
        //         "eng1.mis",
        //         "eng2.mis",
        //         "hydro1.mis",
        //         "hydro2.mis",
        //         "hydro3.mis",
        //         "ops1.mis",
        //         "ops2.mis",
        //         "ops3.mis",
        //         "ops4.mis",
        //         "rec1.mis",
        //         "rec2.mis",
        //         "rec3.mis",
        //         "command1.mis",
        //         "command2.mis",
        //         "rick1.mis",
        //         "rick2.mis",
        //         "rick3.mis",
        //         "rick3.mis",
        //         "many.mis",
        //         "shodan.mis",
        //     ];

        //     for mission in all_missions {
        //         warn!("Loading mission: {}", mission);
        //         let _ = Mission::load(
        //             mission.to_owned(),
        //             &mut asset_cache,
        //             &mut audio_context,
        //             &global_context,
        //             None,
        //             QuestInfo::new(),
        //         );
        //     }
        // }

        let SceneInitResult {
            scene: active_game_scene,
            mission_save_data: mission_to_save_data,
        } = create_initial_scene(
            &mut asset_cache,
            &mut audio_context,
            &global_context,
            &options,
        );

        // log_entities_with_link(&active_mission.world, |link| {
        //     matches!(link, Link::AIWatchObj(_))
        // });
        // panic!();

        // log_property::<PropSignalType>(&active_mission.world);
        // panic!();

        // log_entity(
        //     &active_mission.world,
        //     **(&active_mission.template_to_entity_id.get(&365).unwrap()),
        // );
        // panic!();

        Game {
            asset_cache,
            audio_context,
            active_game_scene,
            pending_transition: None,
            global_context: Arc::new(global_context),
            last_music_cue: None,
            last_env_sound: None,
            options,
            mission_to_save_data,
            should_quit: false,
        }
    }

    pub fn update(
        &mut self,
        time: &Time,
        input_context: &input_context::InputContext,
        actions: &mut input::InputActionState,
    ) {
        let span = span!(Level::INFO, "update");
        let _enter = span.enter();
        let delta_time = time.elapsed.as_secs_f32();
        trace!("delta_time: {}", delta_time);

        // Drive a background transition: the loading screen animates while the parse
        // runs on its worker thread. Once the parse has finished AND the loading screen
        // has shown for its minimum, run the main-thread build and swap to the mission.
        if let Some(pending) = self.pending_transition.as_mut() {
            pending.frames_shown += 1;
            let ready =
                pending.parse_handle.is_finished() && pending.frames_shown >= MIN_LOADING_FRAMES;
            if ready {
                let pending = self.pending_transition.take().unwrap();
                self.finish_transition(pending);
            }
        }

        // Convert triggered actions into effects; triggered actions are
        // consumed here so injected actions (e.g. from the debug runtime)
        // apply exactly once.
        let action_effects = input::ActionDispatcher::dispatch(actions, input_context);
        actions.clear_triggered();

        // Update the scene (handles movement, physics, collision, teleport internally)
        let effects = self.active_game_scene.update(
            time,
            input_context,
            &mut self.asset_cache,
            &self.options,
            action_effects,
        );

        // Handle ambient audio
        let ambient_state = self.active_game_scene.ambient_audio_state();
        let listener_position = ambient_state
            .as_ref()
            .map(|state| state.player_position)
            .or_else(|| {
                self.active_game_scene
                    .world()
                    .borrow::<UniqueView<PlayerInfo>>()
                    .ok()
                    .map(|player_info| player_info.pos)
            })
            .unwrap_or(vec3(0.0, 0.0, 0.0));

        if let Some(state) = ambient_state {
            if let Some(cue) = state.music_cue {
                self.update_music_cue_if_necessary(cue);
            }

            if let Some(cue_schema) = state.environmental_cue {
                let resolved = self.resolve_schema(&cue_schema);
                self.update_env_sound_if_necessary(resolved);
            }

            let ambient_sounds = state
                .ambient_emitters
                .into_iter()
                .filter_map(|(id, position, schema_name)| {
                    let asset_name = self.resolve_schema(&schema_name);
                    let maybe_audio_clip = self
                        .asset_cache
                        .get_opt(&AUDIO_IMPORTER, &format!("{asset_name}.wav"));
                    maybe_audio_clip.map(|clip| (id, position, clip.clone()))
                })
                .collect::<Vec<(EntityId, Vector3<f32>, Rc<AudioClip>)>>();

            self.audio_context.update(listener_position, ambient_sounds);
        } else {
            self.audio_context.update(listener_position, Vec::new());
        }

        // Handle global effects
        let global_effects = self.active_game_scene.handle_effects(
            effects,
            &self.global_context,
            &self.options,
            &mut self.asset_cache,
            &mut self.audio_context,
        );

        for effect in global_effects {
            self.handle_global_effect(effect);
        }
    }

    fn save_to_file(&self, file_name: String) {
        let save_data = self.build_save_data();
        let mut zip_file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(file_name)
            .unwrap();
        save_data.write(&mut zip_file);
    }

    fn load_from_file(&mut self, file_name: String) {
        let mut file = OpenOptions::new().read(true).open(file_name).unwrap();
        let save_data = SaveData::read(&mut file);
        let was_crouched = save_data.global_data.is_crouched;
        let (mut mission, level_map) = Self::load_from_save_data(
            save_data,
            &mut self.asset_cache,
            &mut self.audio_context,
            &self.global_context,
            &self.options,
        );
        // The saved position is the standing-equivalent center (see
        // GlobalData) and the player was created standing there; re-applying
        // the crouch drops the capsule back into the saved crouched pose
        // (e.g. inside a crawlspace the standing capsule wouldn't fit).
        if was_crouched {
            mission.mission_core.restore_saved_crouch();
        }
        self.active_game_scene = Box::new(mission);
        self.mission_to_save_data = level_map;
    }

    fn load_from_save_data(
        save_data: SaveData,
        asset_cache: &mut AssetCache,
        audio_context: &mut AudioContext<EntityId, String>,
        global_context: &GlobalContext,
        game_options: &GameOptions,
    ) -> (Mission, HashMap<String, EntitySaveData>) {
        load_mission_from_save_data(
            save_data,
            asset_cache,
            audio_context,
            global_context,
            game_options,
        )
    }

    fn build_save_data(&self) -> SaveData {
        let mut level_data = self.mission_to_save_data.clone();

        let (save_data, held_items) = save_load::to_save_data(self.active_game_scene.world());

        level_data.insert(self.active_game_scene.scene_name().to_string(), save_data);

        let (position, rotation) = {
            let player_info = self
                .active_game_scene
                .world()
                .borrow::<UniqueView<PlayerInfo>>()
                .unwrap();
            (player_info.pos, player_info.rotation)
        };

        // Normalize the saved center to standing height (see GlobalData): the
        // live position of a crouched player is the LOWERED collider center,
        // and load always creates a standing capsule first.
        let is_crouched = self.active_game_scene.player_is_crouched();
        let position = if is_crouched {
            position + vec3(0.0, physics::player_crouch_center_shift(), 0.0)
        } else {
            position
        };

        let quest_info = self
            .active_game_scene
            .world()
            .borrow::<UniqueView<QuestInfo>>()
            .unwrap()
            .clone();

        let global_data = GlobalData {
            held_items,
            position,
            rotation,
            quest_info,
            player_vitals: save_load::capture_player_vitals(self.active_game_scene.world()),
            active_mission: self.active_game_scene.scene_name().to_string(),
            is_crouched,
        };

        SaveData {
            global_data,
            level_data,
        }
    }

    fn handle_global_effect(&mut self, global_effect: GlobalEffect) {
        match global_effect {
            GlobalEffect::Save { file_name } => self.save_to_file(file_name),
            GlobalEffect::Load { file_name } => self.load_from_file(file_name),
            GlobalEffect::TransitionLevel {
                level_file,
                loc,
                entities_to_trigger,
                vitals_transition,
            } => {
                let spawn_loc = match loc {
                    None => SpawnLocation::MapDefault,
                    Some(marker) => SpawnLocation::Marker(marker),
                };

                if self.loading_screen_enabled() {
                    self.begin_transition(
                        level_file,
                        spawn_loc,
                        entities_to_trigger,
                        vitals_transition,
                    );
                } else {
                    self.switch_mission_with_trigger(
                        level_file,
                        spawn_loc,
                        entities_to_trigger,
                        vitals_transition,
                    );
                }
            }
            GlobalEffect::TestReload => {
                let (position, rotation) = {
                    let player_info = self
                        .active_game_scene
                        .world()
                        .borrow::<UniqueView<PlayerInfo>>()
                        .unwrap();
                    (player_info.pos, player_info.rotation)
                };
                // As with saves, respawn a crouched player at the
                // standing-equivalent center (the reload creates a standing
                // capsule; held crouch input re-applies on the next step).
                let position = if self.active_game_scene.player_is_crouched() {
                    position + vec3(0.0, physics::player_crouch_center_shift(), 0.0)
                } else {
                    position
                };
                let level_name = self.active_game_scene.scene_name().to_string();
                let spawn_loc = SpawnLocation::PositionRotation(position, rotation);
                if self.loading_screen_enabled() {
                    self.begin_transition(
                        level_name,
                        spawn_loc,
                        vec![],
                        PlayerVitalsTransition::Preserve,
                    );
                } else {
                    self.switch_mission(level_name, spawn_loc, PlayerVitalsTransition::Preserve);
                }
            }
            GlobalEffect::Quit => {
                self.should_quit = true;
            }
        }
    }

    /// Get hand spotlights for enhanced lighting when experimental flag is enabled
    pub fn get_hand_spotlights(&self) -> Vec<engine::scene::light::SpotLight> {
        self.active_game_scene.get_hand_spotlights(&self.options)
    }

    /// Eye (render camera) height above the pawn position returned by
    /// [`Game::render`], in SS2 units - crouch-aware, reflecting the player's
    /// *actual* collider state (stand-up can be refused for lack of headroom).
    /// Flat runtimes should use this for their camera `head_offset` so the
    /// view matches the flat controller's shot origin.
    pub fn player_eye_height(&self) -> f32 {
        player_eye_height_for(self.active_game_scene.player_is_crouched())
    }

    pub fn render(&mut self) -> (Vec<SceneObject>, Vector3<f32>, Quaternion<f32>) {
        let (scene, pos, rot) = self
            .active_game_scene
            .render(&mut self.asset_cache, &self.options);

        // let font = File::open(resource_path("res/fonts/mainfont.FON")).unwrap();
        // let mut font_reader = BufReader::new(font);
        // let font: Rc<Box<dyn engine::Font>> =
        //     Rc::new(Box::new(dark::font::Font::read(&mut font_reader)));

        // let text = SceneObject::world_space_text("test1234567890", font, 0.0);
        // scene.push(RefCell::new(text));

        (scene, pos, rot)
    }

    pub fn render_per_eye(
        &mut self,
        view: Matrix4<f32>,
        projection: Matrix4<f32>,
        screen_size: Vector2<f32>,
    ) -> Vec<SceneObject> {
        let hand_material = engine::scene::color_material::create(vec3(1.0, 0.0, 0.0));
        let transform = Matrix4::from_scale(0.25) * Matrix4::from_translation(vec3(0.0, 4.0, 0.0));
        let mut hand_obj = SceneObject::new(hand_material, Box::new(engine::scene::cube::create()));
        hand_obj.set_transform(transform);

        // Sample for rendering
        let font = self.asset_cache.get(&FONT_IMPORTER, "mainfont.fon");
        // let text_obj_0_0 =
        //     SceneObject::screen_space_text("0, 0", font.clone(), 16.0, 0.5, 0.0, 0.0);

        let mut objs = self.active_game_scene.render_per_eye(
            &mut self.asset_cache,
            view,
            projection,
            screen_size,
            &self.options,
        );

        let world_position = vec3(0.0, 1.0, 0.0);
        let screen_width = screen_size.x;
        let screen_height = screen_size.y;
        let world_space_pos = engine::util::project(
            view,
            projection,
            world_position,
            screen_width,
            screen_height,
        );
        let _text_obj_dynamic = SceneObject::screen_space_text(
            "{dynamic}",
            font.clone(),
            16.0,
            0.5,
            world_space_pos.x,
            world_space_pos.y,
        );
        // let text_material =
        //     TextMaterial::create(font.get_texture().clone(), vec4(1.0, 0.0, 0.0, 1.0));
        // let font_size = 30.0f32;

        // 4 years earlier
        // Ramsey Recruitment Ctr
        // let text_string = "Ramsey Recruitment Ctr.";
        // Debug placeholder cube; keep it out of the clean flatscreen view.
        if self.options.presentation_mode == PresentationMode::Vr {
            objs.extend(vec![hand_obj /*  text_obj_dynamic*/]);
        }
        objs
    }

    pub fn finish_render(
        &mut self,
        view: Matrix4<f32>,
        projection: Matrix4<f32>,
        screen_size: Vector2<f32>,
    ) {
        self.active_game_scene
            .finish_render(&mut self.asset_cache, view, projection, screen_size)
    }

    fn update_music_cue_if_necessary(&mut self, new_cue: String) {
        if self.last_music_cue.is_none() || !self.last_music_cue.as_ref().unwrap().eq(&new_cue) {
            info!("updating music cue: {}", new_cue);
            self.audio_context
                .set_background_music_cue(new_cue.to_owned());
            self.last_music_cue = Some(new_cue);
        }
    }

    fn update_env_sound_if_necessary(&mut self, new_cue: String) {
        if self.last_env_sound.is_none() || !self.last_env_sound.as_ref().unwrap().eq(&new_cue) {
            let maybe_audio_clip = self
                .asset_cache
                .get_opt(&AUDIO_IMPORTER, &format!("{new_cue}.wav"));

            if let Some(audio_clip) = maybe_audio_clip {
                info!("updating env_sound: {}", new_cue);
                self.audio_context.set_environmental_sound(audio_clip);
                self.last_env_sound = Some(new_cue);
            } else {
                warn!("env_sound: unable to load sound: {}", new_cue)
            }
        }
    }

    fn resolve_schema(&self, name: &str) -> String {
        let sound_schema = &self.global_context.gamesys.sound_schema;
        let ret = sound_schema
            .get_random_sample(name)
            .unwrap_or_else(|| name.to_owned());
        trace!("resolved sound schema {} to {}", name, ret);
        ret
    }
}
