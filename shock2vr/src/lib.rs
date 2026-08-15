pub mod audio_log;
pub mod game_scene;
pub mod hand_pose;
pub mod hand_pose_library;
pub mod input;
pub mod input_context;
pub mod install;
pub mod inventory;
pub mod message_trace;
pub mod save_load;
pub mod scenes;
pub mod teleport;
pub mod time;

pub mod career;
mod creature;
pub mod data_files;
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
pub mod research;
mod runtime_props;
mod scripts;
mod systems;
#[cfg(test)]
mod test_support;
/// Shared 2D canvas UI: layout, the screen-space presentation, and the
/// world-space (VR) panel. Public so a runtime can check where its simulated
/// controller ray lands on a frontend panel (`frontend_panel` + `ray_to_canvas`).
pub mod ui;
mod util;
mod virtual_hand;
mod vr_config;
pub mod vr_crouch;
pub mod zip_asset_path;

use scenes::{
    CutscenePlayerScene, GameOverScene, LoadGameScene, MainMenuScene, SceneInitResult,
    create_initial_scene, load_mission_from_save_data, resolve_ending_cutscene,
};

pub use mission::SpawnLocation;
pub use mission::visibility_engine::CullingInfo;

/// Player eye (camera) height above the physics body's center, in SS2 units
/// (before the `dark::SCALE_FACTOR` world-scale divide). Shared by every
/// runtime's render camera (`head_offset`) AND the flat controller's
/// shot/viewmodel origin, so the rendered view and where shots come from stay
/// consistent - if these drift, shots no longer line up with the crosshair and
/// the debug runtime renders at a different height than desktop. Crouch swaps
/// this for [`PLAYER_CROUCH_EYE_HEIGHT`] - flat runtimes should use
/// [`Game::player_eye_height`] rather than reading the constants directly.
///
/// This follows the original game's camera placement: the viewpoint is
/// anchored to the player's head sphere - `(PLAYER_HEIGHT / 2) -
/// PLAYER_RADIUS` = 1.8 ft above the body origin - and then raised by the
/// default eye offset ("eyeloc") of 0.8 ft, for a standing eye 2.6 ft above
/// the body center, i.e. 5.6 ft above the floor. Deriving it from the
/// collision profile keeps the eye *inside* the standing collider (2.6 ft is
/// below the 3.0 ft capsule crown): an eye above the crown starts the
/// crosshair ray outside any room whose ceiling the body itself clears, so the
/// ray hits that ceiling from above and nothing in the room can be highlighted
/// or frobbed (#795).
pub const PLAYER_EYE_HEIGHT: f32 = physics::PLAYER_HEAD_POS + physics::PLAYER_EYE_OFFSET;

/// Crouched eye height above the (crouched) body position, in SS2 units.
/// The crouched capsule is 2.8 ft tall with its center 1.4 ft above the feet;
/// +1.2 puts the eye at 2.6 ft above the feet - ~90% of crouched body height
/// like the original engine's crouch, and inside the capsule so the camera
/// cannot poke through a low ceiling the collider clears.
pub const PLAYER_CROUCH_EYE_HEIGHT: f32 = 1.2;

/// Real-world meters per world unit: 1 world unit is `dark::SCALE_FACTOR`
/// (2.5) SS2 feet, and an SS2 foot is a real foot (0.3048 m). VR runtimes
/// divide floor-relative tracked poses (meters) by this before feeding them
/// to the game so the world renders at true scale and a tracked eye N meters
/// above the physical floor lands the equivalent height above the game floor.
pub const METERS_PER_WORLD_UNIT: f32 = 0.3048 * dark::SCALE_FACTOR;

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
    io,
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

const AUDIO_EAR_OFFSET: f32 = 1.0;

fn listener_ear_positions(
    position: Vector3<f32>,
    player_rotation: Quaternion<f32>,
    head_rotation: Quaternion<f32>,
) -> (Vector3<f32>, Vector3<f32>) {
    let right = (player_rotation * head_rotation) * vec3(AUDIO_EAR_OFFSET, 0.0, 0.0);
    (position - right, position + right)
}

fn read_save_file(path: &Path) -> io::Result<SaveData> {
    let mut file = File::open(path)?;
    Ok(SaveData::read(&mut file))
}

/// Whether the resolved data root is a 25th Anniversary Edition install rather
/// than a classic one. The remaster ships its data inside `sshock2.kpf`; a
/// classic install has loose `.crf` archives instead.
pub fn is_25th_anniversary_install() -> bool {
    data_files::is_25th_anniversary_install(paths::data_root())
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
pub(crate) const MOD_ARCHIVES: &[&str] = &[
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

/// Mount one resource family, matching the options its `.crf` counterpart uses.
///
/// `strings` must not collapse basenames (translated tables share them), and
/// `iface` additionally registers an `iface/`-qualified key because it collides
/// with `obj`/`bitmap` on seven names. Getting these wrong is silent: the wrong
/// string table or the wrong texture simply resolves first.
fn mount_family(
    archive: String,
    prefix: &str,
    family: &str,
) -> Box<dyn engine::assets::asset_paths::AbstractAssetPath> {
    match family {
        "strings" => ZipAssetPath::with_prefix_opts(archive, prefix, false, None),
        "iface" => ZipAssetPath::with_prefix_opts(archive, prefix, true, Some("iface")),
        _ => ZipAssetPath::with_prefix(archive, prefix),
    }
}

/// The 25AE mounts for one resource family, highest priority first: every mod
/// layer allowed to override it, then the base archive.
fn anniversary_family_mounts(
    family: &str,
) -> Vec<Box<dyn engine::assets::asset_paths::AbstractAssetPath>> {
    let mut mounts: Vec<Box<dyn engine::assets::asset_paths::AbstractAssetPath>> = Vec::new();
    for archive in MOD_ARCHIVES {
        if !mod_layer_may_override(family, archive) {
            continue;
        }
        let path = resource_path(archive);
        if Path::new(&path).exists() {
            mounts.push(mount_family(path, &format!("{family}/"), family));
        }
    }
    // The base archive keeps the original `data/res/<family>/` layout.
    mounts.push(mount_family(
        resource_path("sshock2.kpf"),
        &format!("data/res/{family}/"),
        family,
    ));
    mounts
}

/// The mounts for a *single* resource family, for a CLI tool that needs one
/// family's files without a renderer - `dq maps` reads the map rectangles out
/// of `intrface`, which a classic install keeps in `res/intrface.crf` and a
/// 25AE one inside the KPF layers.
///
/// Same archives, same precedence the game resolves that family through. A
/// missing archive contributes no mount, so a lookup comes back empty instead
/// of panicking inside the archive reader.
pub fn resource_family_paths(
    family: &str,
) -> Box<dyn engine::assets::asset_paths::AbstractAssetPath> {
    let mounts = if is_25th_anniversary_install() {
        anniversary_family_mounts(family)
    } else {
        // A `.crf` holds one family at its root, so it needs no prefix.
        let archive = resource_path(&format!("res/{family}.crf"));
        if Path::new(&archive).exists() {
            vec![mount_family(archive, "", family)]
        } else {
            Vec::new()
        }
    };
    AssetPath::combine(mounts)
}

fn build_25th_anniversary_mounts(
    bundle_storage: Arc<dyn Storage>,
) -> Vec<Box<dyn engine::assets::asset_paths::AbstractAssetPath>> {
    let mut mounts: Vec<Box<dyn engine::assets::asset_paths::AbstractAssetPath>> = Vec::new();

    for family in RESOURCE_FAMILIES {
        mounts.extend(anniversary_family_mounts(family));
    }

    // The gamesys, missions and motiondb - shared with the CLI tools, which
    // need those files without any of the resource families above.
    mounts.extend(data_files::data_file_mounts(paths::data_root()));

    mounts.push(BundleAssetPath::new("".to_owned(), bundle_storage));
    mounts.push(AssetPath::folder("".to_owned()));
    mounts
}

/// Whether a mission file is available to load, in whichever layout is in use.
///
/// A classic install has the `.mis` on disk; a 25AE install has it inside
/// `sshock2.kpf` under `data/`. Callers that want to validate a mission name
/// before asking the game to load it (the debug runtime's `transition-level`
/// endpoint, for one) must not just stat the filesystem.
pub fn mission_exists(mission_file: &str) -> bool {
    if Path::new(&resource_path(mission_file)).exists() {
        return true;
    }
    data_files::mission_names(paths::data_root())
        .iter()
        .any(|mission| mission.eq_ignore_ascii_case(mission_file))
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
    save_load::save_directory().join(format!("{name}.sav"))
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

    // Set only once the retail finale chain reaches DIE-SHODAN-DIE and the
    // ending cutscene has become the active scene.
    campaign_completed: bool,
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
    /// "alive", terminally "dead", or waiting for an activated QBR while
    /// "respawning". This is the automation-visible loss signal.
    pub life_state: String,
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
    /// no health pool. Seeded from `The Player` template and consumed by every
    /// damage path, with zero entering the player death lifecycle.
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
    /// True while a deferred level transition (`--experimental loading_screen`)
    /// is in flight. `update` is what advances it (`pending_transition.frames_shown`
    /// and the background parse-completion check), so a caller that only calls
    /// `update` on activity - e.g. the debug runtime's idle-throttled loop
    /// (#784) - needs this to know a transition still needs frames pumped even
    /// while otherwise idle.
    pub fn has_pending_transition(&self) -> bool {
        self.pending_transition.is_some()
    }

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
            life_state: world
                .borrow::<shipyard::UniqueView<crate::mission::PlayerLifeState>>()
                .map(|life| life.as_str().to_owned())
                .unwrap_or_else(|_| "alive".to_owned()),
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

        let (current_save_data, held_data) = save_load::to_save_data_with_scripts(
            self.active_game_scene.world(),
            self.active_game_scene.script_world(),
        );
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
        self.set_active_scene(Box::new(active_mission));
        self.campaign_completed = false;
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

        self.set_active_scene(Box::new(LoadingScene::new()));
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
        self.set_active_scene(Box::new(mission));
        self.campaign_completed = false;

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

    /// Whether the retail campaign finale has entered its terminal cutscene.
    pub fn campaign_completed(&self) -> bool {
        self.campaign_completed
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
    /// Returns the scene name that was saved so the caller can report it, or
    /// an error when transient locomotion has no collision-valid standing pose.
    pub fn save_game(&mut self, file: String) -> Result<String, String> {
        let path = save_file_path(&file);
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        self.save_to_file(path.to_string_lossy().into_owned())?;
        Ok(self.scene_name().to_string())
    }

    /// Load a previously-saved game from `<data_root>/saves/<file>.sav`,
    /// restoring the active mission, player position/rotation, quest bits, held
    /// items, and exact current/maximum player vitals. The switch is synchronous
    /// (no loading-screen deferral), so the returned scene name already reflects
    /// the restored mission. A missing save leaves the active game unchanged.
    pub fn load_game(&mut self, file: String) -> io::Result<String> {
        let path = save_file_path(&file);
        self.load_from_file(path.to_string_lossy().into_owned())?;
        Ok(self.scene_name().to_string())
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

    /// Debug provisioning: instantiate an item template straight into the
    /// player's backpack (see
    /// [`DebuggableScene::spawn_item_for_player`](game_scene::DebuggableScene::spawn_item_for_player)).
    /// Lives on `Game` rather than being reachable through `debug_scene_mut`
    /// because entity creation also needs the game-owned `AssetCache`.
    pub fn debug_spawn_item(
        &mut self,
        template: &game_scene::DebugItemTemplate,
    ) -> Result<game_scene::DebugSpawnedItem, String> {
        let asset_cache = &mut self.asset_cache;
        self.active_game_scene
            .as_debuggable_mut()
            .ok_or_else(|| "no debuggable scene available".to_string())?
            .spawn_item_for_player(asset_cache, template)
    }

    /// Resolve the high-detail (`PMNM`) mesh setting from the experimental flags,
    /// falling back to the platform default.
    ///
    /// Two explicit flags rather than one, because the default differs per
    /// platform: `high_detail_meshes` forces it on (for measuring on Quest, where
    /// it is off by default), `no_high_detail_meshes` forces it off (to rule it
    /// out on desktop, where it is on).
    fn resolve_high_detail_meshes(features: &HashSet<String>) -> bool {
        if features.contains("no_high_detail_meshes") {
            false
        } else if features.contains("high_detail_meshes") {
            true
        } else {
            dark::high_detail::default_enabled()
        }
    }

    pub fn init(options: GameOptions, bundle_storage: Arc<dyn Storage>) -> Game {
        // Must happen before any model is loaded.
        let high_detail = Self::resolve_high_detail_meshes(&options.experimental_features);
        dark::high_detail::set_enabled(high_detail);
        info!("high-detail (PMNM) meshes: {high_detail}");

        // What data is actually here, decided once and reported before anything
        // mounts it. `println!` rather than `info!`: neither `desktop_runtime`
        // nor `oculus_runtime` installs a tracing subscriber, so an `info!` here
        // is dropped on the desktop and on the Quest - the two places this line
        // is needed most. On Quest it is the only way to tell a
        // wrongly-provisioned headset from a broken build. (ndk-glue redirects
        // stdout into logcat, so `println!` does arrive there.)
        let install = install::probe_data_root();
        println!("{}", install.summary());

        // Fail here rather than several layers down. Without this, an empty data
        // root reaches the mount list and dies inside the archive reader on
        // whichever `.crf` it happens to open first - a stack trace that names a
        // zip path and says nothing about the actual problem.
        //
        // `App::init` routes this case to the missing-assets screen instead, so
        // the player never reaches this panic. It still guards `debug_runtime`
        // and `dark_viewer`, which build a `Game` directly.
        if !install.has_data() {
            panic!("cannot load the game: {}", install.summary());
        }

        // A 25th Anniversary install keeps everything inside KPF archives, so it
        // needs a different mount list from a pre-remaster install's loose `.crf`s.
        let asset_paths = if install.kind == install::InstallKind::Anniversary {
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

        // Through the asset paths, like the missions and motiondb: on a 25AE
        // install the gamesys lives inside `sshock2.kpf`.
        let game_reader = asset_cache.get_raw_reader("shock2.gam").unwrap_or_else(|| {
            panic!(
                "cannot load the game: shock2.gam not found in the mounted data ({})",
                install.summary()
            )
        });

        let _strings = asset_cache.get(&STRINGS_IMPORTER, "objname.str");

        // vhot logging:
        // let atek_file = File::open(resource_path("res/obj/ar15_w.bin")).unwrap();
        // let mut atek_reader = BufReader::new(atek_file);
        // let header = ss2_bin_header::read(&mut atek_reader);
        // let obj = ss2_bin_obj_loader::read(&mut atek_reader, &header);

        let gamesys = gamesys::read(
            &mut *game_reader.borrow_mut(),
            &links,
            &links_with_data,
            &properties,
        );

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
            campaign_completed: false,
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

        // Publish the simulation clock for the diagnostics ring buffers
        // (audio log / message trace), so their entries can be stamped
        // without threading `Time` through every record site.
        audio_log::set_sim_time(time.total.as_secs_f64());

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
        let player_pose = self
            .active_game_scene
            .world()
            .borrow::<UniqueView<PlayerInfo>>()
            .ok()
            .map(|player_info| (player_info.pos, player_info.rotation));
        let listener_position = ambient_state
            .as_ref()
            .map(|state| state.player_position)
            .or_else(|| player_pose.map(|pose| pose.0))
            .unwrap_or(vec3(0.0, 0.0, 0.0));
        let player_rotation = player_pose
            .map(|pose| pose.1)
            .unwrap_or_else(|| Quaternion::new(1.0, 0.0, 0.0, 0.0));
        let (left_ear_position, right_ear_position) = listener_ear_positions(
            listener_position,
            player_rotation,
            input_context.head.rotation,
        );

        let ambient_sounds = if let Some(state) = ambient_state {
            if let Some(cue) = state.music_cue {
                self.update_music_cue_if_necessary(cue);
            }

            if let Some(cue_schema) = state.environmental_cue {
                let resolved = self.resolve_schema(&cue_schema);
                self.update_env_sound_if_necessary(resolved);
            }

            state
                .ambient_emitters
                .into_iter()
                .filter_map(|(id, position, schema_name)| {
                    let asset_name = self.resolve_schema(&schema_name);
                    let maybe_audio_clip = self
                        .asset_cache
                        .get_opt(&AUDIO_IMPORTER, &format!("{asset_name}.wav"));
                    maybe_audio_clip.map(|clip| (id, position, clip.clone()))
                })
                .collect::<Vec<(EntityId, Vector3<f32>, Rc<AudioClip>)>>()
        } else {
            Vec::new()
        };

        let world = self.active_game_scene.world();
        self.audio_context.update(
            left_ear_position,
            right_ear_position,
            ambient_sounds,
            |entity_id| util::get_entity_position(world, entity_id),
        );

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

    fn save_to_file(&self, file_name: String) -> Result<(), String> {
        let Some(save_data) = self.build_save_data() else {
            return Err(format!(
                "Unable to save '{}': no collision-valid standing player pose is currently available",
                file_name
            ));
        };
        // Saves live in `<data_root>/saves`, which may not exist yet on a fresh
        // install - a quicksave must create it rather than fail (and a failure
        // must not take the game down).
        let path = Path::new(&file_name);
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            std::fs::create_dir_all(parent)
                .map_err(|error| format!("Unable to create '{}': {}", parent.display(), error))?;
        }
        let mut zip_file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(path)
            .map_err(|error| format!("Unable to save '{}': {}", file_name, error))?;
        save_data.write(&mut zip_file);
        Ok(())
    }

    fn load_from_file(&mut self, file_name: String) -> io::Result<()> {
        let save_data = read_save_file(Path::new(&file_name))?;
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
        self.set_active_scene(Box::new(mission));
        self.mission_to_save_data = level_map;
        self.campaign_completed = false;
        Ok(())
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

    /// Player transform with the center normalized to a collision-valid
    /// standing pose. Both persistent saves and debug reloads recreate a
    /// standing capsule, so neither may use a transient compressed mantle pose.
    /// `None` defers the operation when a live blocker has occupied the
    /// mantle's cached recovery pose.
    fn player_standing_transform(&self) -> Option<(Vector3<f32>, Quaternion<f32>)> {
        let safe_position = self.active_game_scene.player_save_position()?;
        let (position, rotation) = {
            let player_info = self
                .active_game_scene
                .world()
                .borrow::<UniqueView<PlayerInfo>>()
                .unwrap();
            (safe_position, player_info.rotation)
        };
        let position = if self.active_game_scene.player_is_crouched() {
            position + vec3(0.0, physics::player_crouch_center_shift(), 0.0)
        } else {
            position
        };
        Some((position, rotation))
    }

    fn build_save_data(&self) -> Option<SaveData> {
        let mut level_data = self.mission_to_save_data.clone();

        let (save_data, held_items) = save_load::to_save_data_with_scripts(
            self.active_game_scene.world(),
            self.active_game_scene.script_world(),
        );

        level_data.insert(self.active_game_scene.scene_name().to_string(), save_data);

        let is_crouched = self.active_game_scene.player_is_crouched();
        let (position, rotation) = self.player_standing_transform()?;

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

        Some(SaveData {
            global_data,
            level_data,
        })
    }

    fn handle_global_effect(&mut self, global_effect: GlobalEffect) {
        match global_effect {
            GlobalEffect::Save { file_name } => {
                if let Err(error) = self.save_to_file(file_name) {
                    warn!("{error}");
                }
            }
            GlobalEffect::Load { file_name } => {
                if let Err(error) = self.load_from_file(file_name.clone()) {
                    warn!("Unable to load save '{}': {}", file_name, error);
                }
            }
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
                let Some((position, rotation)) = self.player_standing_transform() else {
                    warn!(
                        "Unable to reload level: no collision-valid standing player pose is currently available"
                    );
                    return;
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
            GlobalEffect::GameOver => {
                // The run is over, so the dead mission is deliberately NOT
                // written back to the in-memory level ledger: the only way
                // forward is the screen's own recovery path (reload a save),
                // which replaces the ledger wholesale.
                self.pending_transition = None;
                self.set_active_scene(Box::new(GameOverScene::new()));
            }
            GlobalEffect::ShowLoadGame => {
                // A frontend screen swap, not a level transition: no ledger
                // write-back, and any pending transition is abandoned.
                self.pending_transition = None;
                self.set_active_scene(Box::new(LoadGameScene::new()));
            }
            GlobalEffect::ShowMainMenu => {
                self.pending_transition = None;
                self.set_active_scene(Box::new(MainMenuScene::new()));
            }
            GlobalEffect::CompleteCampaign => {
                // Preserve the destroyed head and the rest of the finale state
                // in the in-memory mission ledger before the cutscene replaces
                // the active world.
                self.save_active_scene();

                let (cutscene_name, cutscene_path) = resolve_ending_cutscene();
                let path_string = cutscene_path.to_string_lossy().into_owned();
                let cutscene = CutscenePlayerScene::new(
                    cutscene_name.clone(),
                    path_string.clone(),
                    &mut self.audio_context,
                )
                .unwrap_or_else(|error| {
                    panic!(
                        "Failed to initialize ending cutscene '{}' from '{}': {}",
                        cutscene_name, path_string, error
                    )
                });

                self.pending_transition = None;
                self.set_active_scene(Box::new(cutscene));
                self.campaign_completed = true;
            }
            GlobalEffect::Quit => {
                self.should_quit = true;
            }
        }
    }

    /// Replace the active scene, giving the outgoing one a chance to release
    /// what it owns beyond its own frame (see [`GameScene::on_exit`]). Every
    /// scene swap goes through here so that hook cannot be forgotten.
    fn set_active_scene(&mut self, scene: Box<dyn GameScene>) {
        self.active_game_scene.on_exit(&mut self.audio_context);
        self.active_game_scene = scene;
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

    /// Height (world units) of the player collider's center above the surface
    /// it stands on, for the current stance. VR runtimes subtract this from
    /// the pawn position returned by [`Game::render`] to anchor floor-relative
    /// tracked poses at the player's feet instead of the collider center.
    pub fn player_center_above_floor(&self) -> f32 {
        physics::player_center_above_floor(self.active_game_scene.player_is_crouched())
    }

    /// Highest the eye may sit (world units) above the player collider's
    /// center for the current stance. VR runtimes clamp the tracked eye to
    /// this so the camera cannot leave the collider crown; see
    /// [`physics::player_eye_cap_above_center`].
    pub fn player_eye_cap_above_center(&self) -> f32 {
        physics::player_eye_cap_above_center(self.active_game_scene.player_is_crouched())
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
        // Sample for rendering
        let font = self.asset_cache.get(&FONT_IMPORTER, "mainfont.fon");
        // let text_obj_0_0 =
        //     SceneObject::screen_space_text("0, 0", font.clone(), 16.0, 0.5, 0.0, 0.0);

        let objs = self.active_game_scene.render_per_eye(
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

#[cfg(test)]
mod tests {
    use super::*;
    use cgmath::{Deg, InnerSpace, Rotation3};
    use std::path::PathBuf;

    fn features(list: &[&str]) -> HashSet<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn defaults_to_the_platform_default_when_unspecified() {
        assert_eq!(
            Game::resolve_high_detail_meshes(&features(&[])),
            dark::high_detail::default_enabled()
        );
    }

    #[test]
    fn opt_out_flag_disables_it() {
        assert!(!Game::resolve_high_detail_meshes(&features(&[
            "no_high_detail_meshes"
        ])));
    }

    #[test]
    fn opt_in_flag_enables_it() {
        assert!(Game::resolve_high_detail_meshes(&features(&[
            "high_detail_meshes"
        ])));
    }

    /// Opting out is the safer outcome, so it wins a contradiction.
    #[test]
    fn opt_out_beats_opt_in() {
        assert!(!Game::resolve_high_detail_meshes(&features(&[
            "high_detail_meshes",
            "no_high_detail_meshes"
        ])));
    }

    #[test]
    fn missing_save_file_returns_an_error_instead_of_panicking() {
        let missing = std::env::temp_dir().join(PathBuf::from(format!(
            "shock2vr-missing-save-{}-{}.sav",
            std::process::id(),
            std::thread::current().name().unwrap_or("test")
        )));

        let result = read_save_file(&missing);

        assert!(result.is_err());
    }

    #[test]
    fn listener_ears_follow_head_rotation() {
        let position = vec3(10.0, 20.0, 30.0);
        let player_rotation = Quaternion::new(1.0, 0.0, 0.0, 0.0);
        let head_rotation = Quaternion::from_angle_y(Deg(90.0));

        let (left, right) = listener_ear_positions(position, player_rotation, head_rotation);

        assert!((left - vec3(10.0, 20.0, 31.0)).magnitude() < 0.0001);
        assert!((right - vec3(10.0, 20.0, 29.0)).magnitude() < 0.0001);
    }
}

/// What a runtime drives: either the game, or the screen explaining that there
/// is no game data to run.
///
/// [`Game::init`] needs the gamesys, so it cannot be built at all on a machine
/// with no data - and the failure it used to produce was a panic, which on a
/// headset is a silent return to the Horizon shell. The runtimes construct this
/// instead, and drive it with the same calls they already made on `Game`, so
/// neither of them needs a second render loop for the one screen that has to
/// work when nothing else does.
pub enum App {
    Ready(Box<Game>),
    MissingAssets(MissingAssets),
}

impl App {
    /// Probe the data root, then build whichever of the two is appropriate.
    pub fn init(options: GameOptions, bundle_storage: Arc<dyn Storage>) -> App {
        let install = install::probe_data_root();
        if !install.has_data() {
            // `Game::init` reports the install itself; it never runs here.
            println!("{}", install.summary());
            return App::MissingAssets(MissingAssets::new(&install, options));
        }
        App::Ready(Box::new(Game::init(options, bundle_storage)))
    }

    pub fn update(
        &mut self,
        time: &Time,
        input_context: &input_context::InputContext,
        action_state: &mut input::InputActionState,
    ) {
        match self {
            App::Ready(game) => game.update(time, input_context, action_state),
            App::MissingAssets(missing) => missing.update(time, input_context),
        }
    }

    pub fn render(&mut self) -> (Vec<SceneObject>, Vector3<f32>, Quaternion<f32>) {
        match self {
            App::Ready(game) => game.render(),
            App::MissingAssets(missing) => missing.render(),
        }
    }

    pub fn render_per_eye(
        &mut self,
        view: Matrix4<f32>,
        projection: Matrix4<f32>,
        screen_size: Vector2<f32>,
    ) -> Vec<SceneObject> {
        match self {
            App::Ready(game) => game.render_per_eye(view, projection, screen_size),
            App::MissingAssets(missing) => missing.render_per_eye(view, projection, screen_size),
        }
    }

    pub fn finish_render(
        &mut self,
        view: Matrix4<f32>,
        projection: Matrix4<f32>,
        screen_size: Vector2<f32>,
    ) {
        if let App::Ready(game) = self {
            game.finish_render(view, projection, screen_size);
        }
    }

    pub fn should_quit(&self) -> bool {
        match self {
            App::Ready(game) => game.should_quit(),
            App::MissingAssets(_) => false,
        }
    }

    pub fn wants_pointer(&self) -> bool {
        match self {
            App::Ready(game) => game.wants_pointer(),
            // Nothing to click; leave the cursor to the window manager so the
            // player can close the window.
            App::MissingAssets(_) => true,
        }
    }

    pub fn get_hand_spotlights(&self) -> Vec<engine::scene::light::SpotLight> {
        match self {
            App::Ready(game) => game.get_hand_spotlights(),
            App::MissingAssets(_) => Vec::new(),
        }
    }

    // The three pose accessors below must answer exactly as an uncrouched
    // `Game` does. They are not cosmetic: the VR runtime derives the tracked
    // head offset from them, and a wrong *unit* here (unscaled eye height where
    // a scaled collider-center height is expected) tilts the panel tens of
    // degrees off the player's gaze - on the one screen whose entire job is to
    // be read. `no_assets_pose_matches_a_standing_game` pins them.
    pub fn player_eye_height(&self) -> f32 {
        match self {
            App::Ready(game) => game.player_eye_height(),
            App::MissingAssets(_) => PLAYER_EYE_HEIGHT,
        }
    }

    pub fn player_center_above_floor(&self) -> f32 {
        match self {
            App::Ready(game) => game.player_center_above_floor(),
            App::MissingAssets(_) => physics::player_center_above_floor(false),
        }
    }

    pub fn player_eye_cap_above_center(&self) -> f32 {
        match self {
            App::Ready(game) => game.player_eye_cap_above_center(),
            App::MissingAssets(_) => physics::player_eye_cap_above_center(false),
        }
    }
}

/// The missing-assets screen and the little it needs to render itself.
///
/// The asset cache is real but empty: [`scenes::NoAssetsScene`] draws only text
/// in the engine's compiled-in font, so nothing ever resolves through it. It
/// exists because the shared canvas rendering takes one.
pub struct MissingAssets {
    scene: scenes::NoAssetsScene,
    asset_cache: AssetCache,
    options: GameOptions,
}

impl MissingAssets {
    /// Takes no bundle storage: nothing here resolves an asset, and not taking
    /// it keeps the type constructible in a unit test.
    fn new(install: &install::InstallStatus, options: GameOptions) -> MissingAssets {
        // Deliberately empty: the scene draws only text in the compiled-in
        // font, so nothing resolves through here. Mounting the working
        // directory would let a stray future lookup succeed by accident
        // instead of failing loudly.
        let asset_paths = AssetPath::combine(Vec::new());
        MissingAssets {
            scene: scenes::NoAssetsScene::new(install),
            asset_cache: AssetCache::new(
                paths::data_root().to_string_lossy().into_owned(),
                asset_paths,
            ),
            options,
        }
    }

    fn update(&mut self, time: &Time, input_context: &input_context::InputContext) {
        use crate::game_scene::GameScene;
        self.scene.update(
            time,
            input_context,
            &mut self.asset_cache,
            &self.options,
            Vec::new(),
        );
    }

    fn render(&mut self) -> (Vec<SceneObject>, Vector3<f32>, Quaternion<f32>) {
        use crate::game_scene::GameScene;
        self.scene.render(&mut self.asset_cache, &self.options)
    }

    fn render_per_eye(
        &mut self,
        view: Matrix4<f32>,
        projection: Matrix4<f32>,
        screen_size: Vector2<f32>,
    ) -> Vec<SceneObject> {
        use crate::game_scene::GameScene;
        self.scene.render_per_eye(
            &mut self.asset_cache,
            view,
            projection,
            screen_size,
            &self.options,
        )
    }
}

#[cfg(test)]
mod app_tests {
    use super::*;

    fn missing_assets_app() -> App {
        let status = install::InstallStatus {
            data_root: std::path::PathBuf::from("/nowhere"),
            kind: install::InstallKind::Missing,
            found: Vec::new(),
            missing_mods: Vec::new(),
        };
        App::MissingAssets(MissingAssets::new(&status, GameOptions::default()))
    }

    /// The VR runtime derives the tracked head offset from these three, so a
    /// value in the wrong *unit space* silently tilts the panel tens of degrees
    /// off the player's gaze - on the one screen whose whole job is to be read,
    /// and in a state no debug scene can reach (the debug scene runs inside a
    /// real `Game`). They must answer exactly as an uncrouched player does.
    #[test]
    fn the_missing_assets_pose_matches_a_standing_player() {
        let app = missing_assets_app();
        assert_eq!(
            app.player_center_above_floor(),
            physics::player_center_above_floor(false)
        );
        assert_eq!(
            app.player_eye_cap_above_center(),
            physics::player_eye_cap_above_center(false)
        );
        assert_eq!(app.player_eye_height(), PLAYER_EYE_HEIGHT);
    }

    /// The specific mix-up that shipped: `PLAYER_EYE_HEIGHT` is an *unscaled*
    /// eye height, while these two are *scaled* world units. Returning the
    /// former for either is wrong by a factor of `SCALE_FACTOR`.
    #[test]
    fn the_pose_accessors_are_in_scaled_world_units() {
        let app = missing_assets_app();
        assert!(app.player_center_above_floor() < PLAYER_EYE_HEIGHT);
        assert!(app.player_eye_cap_above_center() > 0.0);
        assert!(app.player_eye_cap_above_center() < PLAYER_EYE_HEIGHT);
    }
}
