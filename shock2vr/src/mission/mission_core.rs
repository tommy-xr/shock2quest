use std::{
    collections::{HashMap, HashSet, VecDeque},
    path::Path,
    rc::Rc,
    sync::Arc,
    time::{Duration, SystemTime},
};

use engine::assets::asset_paths::ReadableAndSeekable;

use cgmath::{EuclideanSpace, Zero};
use cgmath::{
    InnerSpace, Matrix3, Matrix4, Point3, Quaternion, Rotation, Rotation3, SquareMatrix, Transform,
    Vector2, Vector3, num_traits::ToPrimitive, vec2, vec3,
};

use crate::SpawnLocation;
use crate::game_scene::DebuggableScene;
use crate::mission::CullingInfo;
use crate::mission::VisibilityEngine;
use crate::mission::pathfinding_debug;
use crate::pathfinding::{PathfindingService, path_visualization::PathVisualizationSystem};
use crate::psi::{GlobalPsiPowers, PlayerPsiKnownPowers, PsiPowerSelection};
use crate::{mission::entity_creator, scripts::AIPropertyUpdate};

use dark::{
    BitmapAnimation, SCALE_FACTOR,
    audio::SongPlayer,
    gamesys::Gamesys,
    importers::{
        ANIMATION_CLIP_IMPORTER, AUDIO_IMPORTER, MODELS_IMPORTER, SONG_IMPORTER, TEXTURE_IMPORTER,
    },
    mission::{SongParams, room_database::RoomDatabase},
    model::Model,
    motion::{
        AnimationClip, AnimationEvent, AnimationPlayer, MotionDB, MotionQuery, MotionQueryItem,
        MotionQuerySelectionStrategy,
    },
    properties::{
        AmbientSoundFlags, Link, LinkDefinition, LinkDefinitionWithData, Links, PhysicsModelType,
        PropAIAlertness, PropAIMode, PropAmbientHacked, PropAnimLight, PropClassTag, PropCreature,
        PropFrameAnimState, PropHasRefs, PropHitPoints, PropLimbModel, PropLocalPlayer,
        PropModelName, PropMotionActorTags, PropObjState, PropParticleGroup,
        PropParticleLaunchInfo, PropPhysDimensions, PropPhysInitialVelocity, PropPhysState,
        PropPhysType, PropPlayerGun, PropPosition, PropRenderType, PropScripts, PropSymName,
        PropTeleported, PropTripFlags, PropTweqDeleteConfig, PropTweqDeleteState,
        PropTweqModelConfig, PropertyDefinition, RenderType, TeleportSource, ToLink, TripFlags,
        TweqAnimationState, WrappedEntityId,
    },
    ss2_entity_info::{self, SystemShock2EntityInfo},
    tag_database::{TagQuery, TagQueryItem},
};
use engine::{
    assets::asset_cache::AssetCache,
    audio::{AudioChannel, AudioContext, AudioHandle, AudioPlaybackSettings},
    game_log, profile,
    scene::{
        BillboardMaterial, ParticleSystem, RenderLayer, SceneObject, VertexPosition,
        light::SpotLight, quad,
    },
    texture::{TextureOptions, TextureTrait, init_from_memory2},
    texture_format::{PixelFormat, RawTextureData},
};
use physics::PhysicsWorld;
use rand::{
    Rng, distributions::WeightedIndex, prelude::Distribution, seq::SliceRandom, thread_rng,
};
use rapier3d::prelude::{Collider, RigidBodyHandle};
use scripts::ScriptWorld;

use shipyard::*;
use shipyard::{self, View, World};
use tracing::{info, trace, warn};

use crate::{
    GameOptions,
    creature::{HitBoxManager, RagDollManager, get_creature_definition},
    game_scene::AmbientAudioState,
    game_scene::PlayerSavePoseError,
    gui::GuiManager,
    hud::{draw_item_name, draw_item_outline},
    input_context::{self, InputContext},
    interaction::{FlatInteraction, InteractionContext, PlayerInteraction, VrInteraction},
    inventory::PlayerInventoryEntity,
    mission::{SpatialQueryEngine, entity_populator::EntityPopulator},
    physics::{self, CollisionGroup, PlayerHandle},
    quest_info::QuestInfo,
    runtime_props::{
        RuntimePropAIBehavior, RuntimePropAttachment, RuntimePropDeathPose,
        RuntimePropDoNotSerialize, RuntimePropFlatAim, RuntimePropJointTransforms,
        RuntimePropLaunchedProjectile, RuntimePropReloading, RuntimePropSelectedAmmo,
        RuntimePropTransform, RuntimePropVhots, RuntimePropVrGripOffset,
    },
    save_load::HeldItemSaveData,
    scripts::{
        self, Effect, GlobalEffect, Message, MessagePayload,
        internal_fast_projectile::InternalFastProjectileScript,
        script_util::{
            get_all_links_with_template, get_environmental_sound_query, has_death_links,
        },
        speech_registry::SpeechVoiceRegistry,
    },
    systems::{
        run_attachment_update, run_bitmap_animation, run_tweq, turn_off_tweqs, turn_on_tweqs,
    },
    teleport::{TeleportSystem, TeleportUI, TeleportVisualStyle},
    time::Time,
    util::{debug_entity, get_email_sound_file, get_entity_position, has_refs, vec3_to_point3},
    virtual_hand::VirtualHandEffect,
    vr_config,
};

use crate::mission::entity_creator::{CreateEntityOptions, EntityCreationInfo};

/// `The Player` gamesys template - carries the player archetype data
/// (starting hit points, psi pool, base stats, vulnerabilities, ...).
pub const THE_PLAYER_TEMPLATE_ID: i32 = -384;

/// Vertical clearance (world units) a death-handoff ragdoll spawns with, so a
/// floor-lying crumple pose doesn't start deeply interpenetrating the level
/// trimesh (see `spawn_ragdoll`).
const RAGDOLL_SPAWN_LIFT: f32 = 0.05;

/// Resolve optional media-reader portrait/icon art before it becomes a shared
/// UI image. STR tables author extension-less PCX-era names, while replacement
/// layers may provide the same art under a modern encoding (SCP's Earth
/// `RamsIcon` is PNG). Missing optional art is omitted here; the shared canvas
/// renderer remains strict for required UI assets such as the reader backdrop.
fn resolve_optional_log_art_texture(
    asset_cache: &AssetCache,
    deck: u32,
    log: u32,
    role: &str,
    authored: Option<String>,
) -> Option<String> {
    let authored = authored?;
    let authored = authored.trim();
    if authored.is_empty() {
        return None;
    }
    let requested = if Path::new(authored).extension().is_some() {
        authored.to_owned()
    } else {
        format!("{authored}.pcx")
    };
    let resolved = dark::util::resolve_texture_name(asset_cache, &requested);
    if resolved.is_none() {
        game_log!(
            WARN,
            "audio log {deck}/{log} omits missing optional {role} texture '{requested}'"
        );
    }
    resolved
}
/// Cold model loads make entity initialization the longest main-thread load
/// loop, so periodically let the host service its platform queues.
const LOAD_EVENT_PUMP_ENTITY_INTERVAL: usize = 32;

/// Index of a hand in the per-hand `[left, right]` arrays this module keeps
/// (the VR grab swallow, the on-panel arbitration). One conversion, so the two
/// sides of a latch can never disagree about which slot a hand owns.
fn hand_slot(hand: crate::vr_config::Handedness) -> usize {
    match hand {
        crate::vr_config::Handedness::Left => 0,
        crate::vr_config::Handedness::Right => 1,
    }
}

/// Advance the VR grab swallow one frame, per hand (see
/// [`MissionCore::vr_squeeze_swallow`]).
///
/// `claims` is "the cyber interface owns this hand's squeeze right now" - its
/// ray is on the panel with the interface up. The latch turns that instant into
/// a gesture: once set it holds until the squeeze is released, so a squeeze
/// begun on the panel cannot reach the world by sliding off the panel edge or
/// by the mode closing under it. It also drops the moment the hand is holding
/// something, because a hand that just took an item off the panel must see its
/// own squeeze again - masking it would read as a release.
///
/// Pure, so the rule is exercised directly rather than through a whole mission.
fn update_squeeze_swallow(
    latched: &mut [bool; 2],
    squeezing: [bool; 2],
    hand_empty: [bool; 2],
    claims: [bool; 2],
) {
    for slot in 0..latched.len() {
        if !squeezing[slot] || !hand_empty[slot] {
            latched[slot] = false;
        } else if claims[slot] {
            latched[slot] = true;
        }
    }
}

/// Head-relative authored-space placement for the wide VR backpack canvas.
/// After `SCALE_FACTOR` conversion this is 2 world units forward and 1.2 up.
/// The shared canvas keeps its authored pixels; the physical VR boundary
/// scales the unusually wide 15-column strip so all of it fits at arm's reach.
const VR_BACKPACK_WORLD_SCALE: f32 = 0.55;

/// Head-relative authored-space placement for the narrow portrait reader.
/// After Dark's scale conversion the panel center is 1.5 world units forward
/// and 1.4 units above the player origin, keeping its 0.75 x 1.18 canvas fully
/// framed and within ordinary controller reach.
const VR_MEDIA_FORWARD: f32 = 3.75;
const VR_MEDIA_UP: f32 = 3.5;

/// The use-mode ("cyber interface") open/close sting: the shipped gamesys's
/// own `UI_SCH` schema pair for opening/closing the game's main panel
/// (`cargo dq templates 610`/`611`) - a faithful "interface open/close"
/// chime, addressed by symbolic name like `repfail`/`hackfail` elsewhere in
/// this file, rather than an invented sound. Played from the single
/// `enter_use_mode`/`leave_use_mode` pair both presentations share, so flat
/// and VR always sound the same.
const USE_MODE_OPEN_SOUND: &str = "mainpanel_op";
const USE_MODE_CLOSE_SOUND: &str = "mainpanel_cl";

fn presentation_world_panel_size(
    presentation: crate::PresentationMode,
    is_player_backpack: bool,
    authored_size: Vector2<f32>,
) -> Vector2<f32> {
    if presentation == crate::PresentationMode::Vr && is_player_backpack {
        authored_size * VR_BACKPACK_WORLD_SCALE
    } else {
        authored_size
    }
}

#[cfg(test)]
mod vr_backpack_panel_tests {
    use super::*;

    #[test]
    fn only_vr_scales_the_shared_backpack_canvas_at_the_world_boundary() {
        let authored = vec2(635.0, 120.0) * crate::gui::GUI_PIXEL_TO_WORLD_SIZE;
        assert_eq!(
            presentation_world_panel_size(crate::PresentationMode::Flat, true, authored),
            authored
        );
        assert_eq!(
            presentation_world_panel_size(crate::PresentationMode::Vr, false, authored),
            authored
        );
        assert_eq!(
            presentation_world_panel_size(crate::PresentationMode::Vr, true, authored),
            authored * VR_BACKPACK_WORLD_SCALE
        );
    }
}

fn is_realtime_crumple(frame_count: f32) -> bool {
    // `humdieup1` is an authored "already dead" pose: three frames with no
    // fall. It shares the human crumple+die schema, but is not a real-time
    // death and otherwise makes one in five human kills instantaneous.
    frame_count > 3.0
}

fn select_motion_option(
    options: &[String],
    selection_strategy: &MotionQuerySelectionStrategy,
) -> Option<String> {
    if options.is_empty() {
        return None;
    }
    match selection_strategy {
        MotionQuerySelectionStrategy::Random => options.choose(&mut thread_rng()).cloned(),
        MotionQuerySelectionStrategy::Sequential(sequence) => {
            options.get(*sequence as usize % options.len()).cloned()
        }
    }
}

#[derive(Unique, Clone)]
pub struct PlayerInfo {
    pub pos: Vector3<f32>,
    pub rotation: Quaternion<f32>,
    pub entity_id: EntityId,

    pub left_hand_entity_id: Option<EntityId>,
    pub right_hand_entity_id: Option<EntityId>,
    pub inventory_entity_id: EntityId,
}

/// The live player's death/reconstruction lifecycle.
///
/// This is runtime state, never serialized: a dead game cannot be saved (see
/// `player_save_position`), so only the player's persistent vitals and an
/// activated station's authored model state need serialization. Missions own
/// the full state machine; the game-over screen carries the terminal
/// `GameOver` value so automation still sees the loss after the mission is
/// gone.
#[derive(Unique, Clone, Debug, PartialEq)]
pub enum PlayerLifeState {
    Alive,
    /// No activated/affordable QBR exists in this mission. The authored death
    /// sequence plays out for [`PLAYER_DEATH_SEQUENCE_SECONDS`], then the
    /// mission is replaced by the game-over screen.
    Dead {
        elapsed_seconds: f32,
    },
    /// The death sequence finished and the game-over screen has been
    /// requested, so it is never requested twice.
    GameOver,
    /// The retail five-second death pause before a QBR reconstruction.
    Respawning {
        elapsed_seconds: f32,
        position: Vector3<f32>,
        rotation: Quaternion<f32>,
    },
}

impl PlayerLifeState {
    pub fn as_str(&self) -> &'static str {
        match self {
            PlayerLifeState::Alive => "alive",
            PlayerLifeState::Dead { .. } => "dead",
            PlayerLifeState::GameOver => "game_over",
            PlayerLifeState::Respawning { .. } => "respawning",
        }
    }

    pub fn is_alive(&self) -> bool {
        matches!(self, PlayerLifeState::Alive)
    }
}

const PLAYER_RESPAWN_DELAY_SECONDS: f32 = 5.0;
const PLAYER_RESPAWN_NANITE_COST: i32 = 10;
/// How long terminal death lingers in the mission before the game-over screen
/// takes over - the beat where the authored death vocalization plays and the
/// player sees where they fell.
const PLAYER_DEATH_SEQUENCE_SECONDS: f32 = 3.0;
/// The retail player death vocalizations (`PlayerDeath0..4`, SPEECH_TRIGGERS
/// schemas in the gamesys); one is chosen per death, as the original does.
const PLAYER_DEATH_SCHEMAS: [&str; 5] = [
    "PlayerDeath0",
    "PlayerDeath1",
    "PlayerDeath2",
    "PlayerDeath3",
    "PlayerDeath4",
];

/// Resolve the active QBR's authored teleport trap. `ResurrectMachine` uses
/// the scanner's model tweq as its durable activation state: the initial
/// `res_pad` becomes the config's final model (`res_pad2`) when frobbed, and
/// `PropModelName` already persists with the rest of the mission.
fn active_resurrection_target(world: &World) -> Option<(Vector3<f32>, Quaternion<f32>)> {
    let scripts = world.borrow::<View<PropScripts>>().ok()?;
    let models = world.borrow::<View<PropModelName>>().ok()?;
    let tweq_configs = world.borrow::<View<PropTweqModelConfig>>().ok()?;
    let links = world.borrow::<View<Links>>().ok()?;
    let positions = world.borrow::<View<PropPosition>>().ok()?;

    for (button, (button_scripts, model, config)) in
        (&scripts, &models, &tweq_configs).iter().with_id()
    {
        let is_resurrection_button = button_scripts
            .scripts
            .iter()
            .any(|script| script.eq_ignore_ascii_case("resurrectmachine"));
        let is_active = config
            .model_names
            .last()
            .is_some_and(|active_model| model.0.eq_ignore_ascii_case(active_model));
        if !is_resurrection_button || !is_active {
            continue;
        }

        let Some(button_links) = links.get(button).ok() else {
            continue;
        };
        for link in &button_links.to_links {
            if !matches!(link.link, Link::SwitchLink) {
                continue;
            }
            let Some(target) = link.to_entity_id.map(|target| target.0) else {
                continue;
            };
            let target_is_teleport = scripts.get(target).is_ok_and(|target_scripts| {
                target_scripts
                    .scripts
                    .iter()
                    .any(|script| script.eq_ignore_ascii_case("trapteleport"))
            });
            if target_is_teleport {
                if let Ok(position) = positions.get(target) {
                    return Some((position.position, position.rotation));
                }
            }
        }
    }

    None
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PsiKitUseOutcome {
    NotUsed,
    DecrementedStack,
    DestroyEntity,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ComestibleUseOutcome {
    NotUsed,
    Consumed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum HealingItemUseOutcome {
    NotUsed,
    DecrementedStack,
    DestroyEntity,
}

fn update_player_psi_points(world: &World, update: impl FnOnce(i32) -> i32) -> bool {
    let player_entity = world.borrow::<UniqueView<PlayerInfo>>().unwrap().entity_id;
    let mut psi_states = world
        .borrow::<ViewMut<dark::properties::PropPsiState>>()
        .unwrap();
    if let Ok(psi) = (&mut psi_states).get(player_entity) {
        let maximum = psi.max_psi_points.max(0);
        let updated = update(psi.psi_points).clamp(0, maximum);
        let changed = psi.psi_points != updated;
        psi.psi_points = updated;
        changed
    } else {
        false
    }
}

/// Apply the stateful portion of one psi-kit effect against the live world.
/// The caller performs canonical entity teardown for [`PsiKitUseOutcome::DestroyEntity`].
fn apply_psi_kit_use(world: &World, entity_id: EntityId, amount: i32) -> PsiKitUseOutcome {
    if amount <= 0 {
        return PsiKitUseOutcome::NotUsed;
    }
    let is_alive = world
        .borrow::<EntitiesView>()
        .is_ok_and(|entities| entities.is_alive(entity_id));
    if !is_alive {
        return PsiKitUseOutcome::NotUsed;
    }
    let stack_count = world
        .borrow::<View<dark::properties::PropStackCount>>()
        .ok()
        .and_then(|stacks| stacks.get(entity_id).ok().map(|stack| stack.0));
    if stack_count.is_some_and(|stack| stack <= 0) {
        return PsiKitUseOutcome::NotUsed;
    }
    if !update_player_psi_points(world, |current| current.saturating_add(amount)) {
        return PsiKitUseOutcome::NotUsed;
    }

    if stack_count.is_some_and(|stack| stack > 1) {
        let mut stacks = world
            .borrow::<ViewMut<dark::properties::PropStackCount>>()
            .unwrap();
        if let Ok(stack) = (&mut stacks).get(entity_id) {
            stack.0 -= 1;
        }
        PsiKitUseOutcome::DecrementedStack
    } else {
        PsiKitUseOutcome::DestroyEntity
    }
}

/// Move a still-live entity's containment link and mark it as no longer
/// world-referenced. Effects are processed sequentially, so an earlier effect
/// in the same batch may already have consumed the requested item.
fn move_live_entity_into_container(
    world: &mut World,
    container_entity_id: EntityId,
    dropped_entity_id: EntityId,
) -> bool {
    let is_alive = world
        .borrow::<EntitiesView>()
        .is_ok_and(|entities| entities.is_alive(dropped_entity_id));
    if !is_alive {
        return false;
    }

    // Give the item a cell in the container's grid, so it stays where it
    // was put instead of being repacked on every draw. Computed before
    // the borrow below, and *after* the item's own links are irrelevant -
    // it is not in the container yet, so it cannot occupy a cell here.
    let slot = {
        let grid = crate::inventory::grid_for(world, container_entity_id);
        let occupied =
            crate::inventory::Inventory::from_container(world, container_entity_id, grid);
        let dims = world
            .borrow::<View<dark::properties::PropInventoryDimensions>>()
            .ok()
            .and_then(|v| v.get(dropped_entity_id).ok().map(|d| (d.width, d.height)))
            .unwrap_or((1, 1));
        // A full container still takes the item (the drop is already
        // permitted by the caller); it just has no cell to remember.
        occupied
            .first_free_slot(dims.0 as usize, dims.1 as usize)
            .unwrap_or(0)
    };

    let mut was_able_to_drop = false;
    {
        // First, remove any existing contains links for the dropped entity.
        let mut v_links = world.borrow::<ViewMut<Links>>().unwrap();

        for (id, links) in (&mut v_links).iter().with_id() {
            drop_contains_links_to(links, dropped_entity_id);

            // If it is the container, add the replacement link.
            if id == container_entity_id {
                links.to_links.push(ToLink {
                    link: Link::Contains(slot),
                    to_entity_id: Some(dark::properties::WrappedEntityId(dropped_entity_id)),
                    to_template_id: 0, // todo?
                });
                was_able_to_drop = true;
            }
        }
    }
    if was_able_to_drop {
        world.add_component(dropped_entity_id, PropHasRefs(false));
    }
    was_able_to_drop
}

/// Start one retail timed healing course and consume exactly one carried
/// source item. The health check, queue mutation, and stack decrement all run
/// against live state in one effect pass, so duplicate uses safely no-op.
fn apply_healing_item_use(
    world: &World,
    entity_id: EntityId,
    total: i32,
    pulse: i32,
    first_pulse_secs: f32,
    pulse_interval_secs: f32,
) -> HealingItemUseOutcome {
    let is_alive = world
        .borrow::<EntitiesView>()
        .is_ok_and(|entities| entities.is_alive(entity_id));
    if !is_alive || !crate::scripts::script_util::player_carried_items(world).contains(&entity_id) {
        return HealingItemUseOutcome::NotUsed;
    }
    let stack_count = world
        .borrow::<View<dark::properties::PropStackCount>>()
        .ok()
        .and_then(|stacks| stacks.get(entity_id).ok().map(|stack| stack.0));
    if stack_count.is_some_and(|stack| stack <= 0) {
        return HealingItemUseOutcome::NotUsed;
    }

    let player = world.borrow::<UniqueView<PlayerInfo>>().unwrap().entity_id;
    let current = world
        .borrow::<View<dark::properties::PropHitPoints>>()
        .ok()
        .and_then(|hit_points| hit_points.get(player).ok().map(|hp| hp.hit_points));
    let maximum = world
        .borrow::<View<dark::properties::PropMaxHitPoints>>()
        .ok()
        .and_then(|max_hit_points| {
            max_hit_points
                .get(player)
                .ok()
                .map(|hp| hp.hit_points.min(i32::MAX as u32) as i32)
        });
    if !matches!((current, maximum), (Some(current), Some(maximum)) if current > 0 && current < maximum)
    {
        return HealingItemUseOutcome::NotUsed;
    }

    let queued = world
        .borrow::<UniqueViewMut<crate::scripts::healing_item::ActiveHealing>>()
        .is_ok_and(|mut active| active.queue(total, pulse, first_pulse_secs, pulse_interval_secs));
    if !queued {
        return HealingItemUseOutcome::NotUsed;
    }

    if stack_count.is_some_and(|stack| stack > 1) {
        let mut stacks = world
            .borrow::<ViewMut<dark::properties::PropStackCount>>()
            .unwrap();
        if let Ok(stack) = (&mut stacks).get(entity_id) {
            stack.0 -= 1;
        }
        HealingItemUseOutcome::DecrementedStack
    } else {
        HealingItemUseOutcome::DestroyEntity
    }
}

/// Restore a live hand-released item to ordinary world ownership.
///
/// Inventory items may have an authored `HasRefs(false)` and can retain a
/// stale `Contains` link across hand/world transitions. Physics creation is
/// intentionally performed only after this invariant is restored: entity
/// creation treats an unreferenced object as non-world state and otherwise
/// declines to give it a selectable body.
fn restore_live_entity_world_refs(world: &mut World, entity_id: EntityId) -> bool {
    let is_alive = world
        .borrow::<EntitiesView>()
        .is_ok_and(|entities| entities.is_alive(entity_id));
    if !is_alive {
        return false;
    }

    {
        let mut links_view = world.borrow::<ViewMut<Links>>().unwrap();
        for links in (&mut links_view).iter() {
            drop_contains_links_to(links, entity_id);
        }
    }
    world.add_component(entity_id, PropHasRefs(true));
    true
}

#[cfg(test)]
mod released_item_world_refs_tests {
    use super::*;

    fn contains(container: &Links, item: EntityId) -> bool {
        container.to_links.iter().any(|link| {
            matches!(link.link, Link::Contains(_))
                && link.to_entity_id.map(|wrapped| wrapped.0) == Some(item)
        })
    }

    #[test]
    fn hand_release_restores_refs_and_clears_residual_containment() {
        let mut world = World::new();
        let item = world.add_entity(PropHasRefs(false));
        let container = world.add_entity(Links {
            to_links: vec![ToLink {
                link: Link::Contains(4),
                to_entity_id: Some(WrappedEntityId(item)),
                to_template_id: 0,
            }],
        });

        assert!(restore_live_entity_world_refs(&mut world, item));
        assert!(
            world
                .borrow::<View<PropHasRefs>>()
                .unwrap()
                .get(item)
                .unwrap()
                .0
        );
        assert!(!contains(
            world
                .borrow::<View<Links>>()
                .unwrap()
                .get(container)
                .unwrap(),
            item,
        ));
    }

    #[test]
    fn ordinary_container_transfer_still_removes_world_refs() {
        let mut world = World::new();
        let item = world.add_entity(PropHasRefs(true));
        let container = world.add_entity(Links::empty());

        assert!(move_live_entity_into_container(&mut world, container, item));
        assert!(
            !world
                .borrow::<View<PropHasRefs>>()
                .unwrap()
                .get(item)
                .unwrap()
                .0
        );
        assert!(contains(
            world
                .borrow::<View<Links>>()
                .unwrap()
                .get(container)
                .unwrap(),
            item,
        ));
    }

    #[test]
    fn late_release_effect_cannot_resurrect_a_consumed_item() {
        let mut world = World::new();
        let item = world.add_entity(PropHasRefs(false));
        world.delete_entity(item);

        assert!(!restore_live_entity_world_refs(&mut world, item));
        assert!(!world.borrow::<EntitiesView>().unwrap().is_alive(item));
    }
}

/// Apply one retail food/drink use against live state. The carried-source
/// validation, bounded heal, and consumption decision occur in one effect
/// pass, so duplicate queued uses of the same object cannot heal twice.
fn apply_comestible_use(
    world: &World,
    entity_id: EntityId,
    hit_points: i32,
) -> ComestibleUseOutcome {
    if hit_points <= 0
        || !world
            .borrow::<EntitiesView>()
            .is_ok_and(|entities| entities.is_alive(entity_id))
        || !crate::scripts::script_util::player_carried_items(world).contains(&entity_id)
    {
        return ComestibleUseOutcome::NotUsed;
    }

    let player = world.borrow::<UniqueView<PlayerInfo>>().unwrap().entity_id;
    let maximum = world
        .borrow::<View<dark::properties::PropMaxHitPoints>>()
        .ok()
        .and_then(|maximums| maximums.get(player).ok().map(|hp| hp.hit_points))
        .map(|maximum| maximum.min(i32::MAX as u32) as i32);
    let Some(maximum) = maximum else {
        return ComestibleUseOutcome::NotUsed;
    };

    let mut current = world
        .borrow::<ViewMut<dark::properties::PropHitPoints>>()
        .unwrap();
    let Ok(current) = (&mut current).get(player) else {
        return ComestibleUseOutcome::NotUsed;
    };
    current.hit_points = current
        .hit_points
        .saturating_add(hit_points)
        .clamp(0, maximum.max(0));
    ComestibleUseOutcome::Consumed
}

/// Raise only the live maximum HP pool. Buying Endurance is not healing: the
/// original stat promises more maximum hit points, while Tank's separate O/S
/// effect deliberately raises both current and maximum HP.
fn increase_player_max_hit_points(world: &World, player: EntityId, amount: u32) -> bool {
    let mut maximums = world
        .borrow::<ViewMut<dark::properties::PropMaxHitPoints>>()
        .unwrap();
    if let Ok(maximum) = (&mut maximums).get(player) {
        maximum.hit_points = maximum.hit_points.saturating_add(amount);
        true
    } else {
        false
    }
}

#[cfg(test)]
mod endurance_upgrade_tests {
    use super::*;

    #[test]
    fn endurance_raises_maximum_hp_without_healing() {
        let mut world = World::new();
        let player = world.add_entity((
            PropHitPoints { hit_points: 25 },
            dark::properties::PropMaxHitPoints { hit_points: 30 },
        ));

        assert!(increase_player_max_hit_points(
            &world,
            player,
            crate::scripts::gui::ENDURANCE_HP_PER_LEVEL,
        ));
        assert_eq!(
            world
                .borrow::<View<dark::properties::PropMaxHitPoints>>()
                .unwrap()
                .get(player)
                .unwrap()
                .hit_points,
            35
        );
        assert_eq!(
            world
                .borrow::<View<PropHitPoints>>()
                .unwrap()
                .get(player)
                .unwrap()
                .hit_points,
            25,
            "an Endurance upgrade must not double as a heal"
        );
    }
}

/// First-person player-melee idle clip (motiondb ActorType 1, `+plyrmelee:0`),
/// used to pose the flat melee viewmodel in its ready stance.
const MELEE_IDLE_CLIP: &str = "ph212203";
/// First-person player-melee swing clip (motiondb `+plyrmelee:2 +plyrmeleeswing`),
/// played once on a melee attack then auto-returns to the idle. The shipped game
/// always uses the medium-left swing.
const MELEE_SWING_CLIP: &str = "leftswing";

/// Velocity (world units/s) a radius blast adds per point of stim intensity to
/// a dynamic body at its center (falling off linearly to zero at the radius).
/// Tuned so a barrel blast (intensity 15) gives nearby props a solid toss.
const BLAST_PUSH_SPEED_PER_INTENSITY: f32 = 0.5;

/// Debug options accessible from scripts via UniqueView
#[derive(Unique, Clone, Default)]
pub struct DebugOptions {
    pub debug_ai: bool,
}

/// The active presentation mode, accessible from scripts via UniqueView -
/// e.g. the held-entity viewmodel swap is flat-only.
#[derive(Unique, Clone, Copy)]
pub struct GlobalPresentationMode(pub crate::PresentationMode);

/// Pathfinding service accessible from scripts (steering strategies) via
/// UniqueView. None when the mission has no AIPATH data (e.g. debug scenes).
#[derive(Unique, Clone)]
pub struct GlobalPathfinding(pub Option<Arc<PathfindingService>>);

/// Off-thread path query worker (see pathfinding::async_queries): steering
/// submits queries here and adopts results on later frames, so A* runs in
/// parallel with simulation + rendering instead of inside the frame. None
/// when the mission has no AIPATH data.
#[derive(Unique, Clone)]
pub struct GlobalAsyncPathfinding(
    pub Option<Arc<crate::pathfinding::async_queries::AsyncPathfinding>>,
);

#[derive(Unique, Clone, Default)]
pub struct EffectQueue {
    effects: Vec<Effect>,
}

pub struct DebugLine {
    pub start: Point3<f32>,
    pub end: Point3<f32>,
    pub color: Vector3<f32>,
    pub remaining_life_in_seconds: f32,
}

#[derive(Clone)]
pub struct EntityMetadata {
    pub template_id: i32,
    pub obj_icon: Option<String>,
    pub obj_short_name: Option<String>,
    #[allow(dead_code)]
    pub obj_name: Option<String>,
}

#[derive(Unique, Clone)]
pub struct GlobalEntityMetadata(pub HashMap<String, EntityMetadata>);

#[derive(Unique, Clone)]
pub struct GlobalTemplateIdMap(pub HashMap<i32, WrappedEntityId>);

/// Global template class tag mapping for script access
#[derive(Unique, Clone)]
pub struct GlobalTemplateClassTags(pub HashMap<i32, HashMap<String, String>>);

/// Global template id -> object-icon (`P$ObjIcon`) bitmap filename (e.g.
/// "STD_I.pcx"). Used to show a projectile/ammo's icon (e.g. the wielded
/// weapon's selected ammo type) without the projectile being an instantiated
/// entity. Derived from the template metadata hydrated at load.
#[derive(Unique, Clone)]
pub struct GlobalTemplateObjIcons(pub HashMap<i32, String>);

/// The synthetic player-owned entity hosting the automap panel (`MapGui`).
/// Created at mission init; `Effect::ToggleMap` opens/closes its panel in the
/// flat host. See `projects/flat-ui-panels.md` §5.
#[derive(Unique, Clone, Copy)]
pub struct MapPanelEntity(pub EntityId);

/// Nonserialized player-owned host for the audio-log reader. Physical log
/// discs belong to their source mission; this host is rebuilt on every mission
/// so a persisted `(deck, log)` can be localized and replayed anywhere.
#[derive(Unique, Clone, Copy)]
pub struct MediaPanelEntity(pub EntityId);

/// The automap location (`PropMapLoc`) of the mapped room the player most
/// recently entered - the player's *current* map location. The automap uses it
/// to draw that location bright (R-art) while other explored locations draw
/// dim (X-art), and to pick a per-frame `MapRef` marker when placing the
/// player pip (multi-story areas relocated into the page's inset boxes). Not
/// serialized: re-established by the room sensors as the player moves.
#[derive(Unique, Clone, Copy, Default)]
pub struct PlayerMapLocation(pub Option<i32>);

/// Global template inheritance hierarchy (template id -> MetaProp parents),
/// so scripts can answer class questions about entities at runtime - e.g.
/// picking a projectile's hit spang by whether the victim descends from the
/// archetype class a `HitSpang` link targets (Hybrids, Robots, ...).
#[derive(Unique, Clone)]
pub struct GlobalTemplateHierarchy(pub HashMap<i32, Vec<i32>>);

/// The trainer upgrade cost tables from the gamesys
/// (`STATCOST`/`WTECHCOST`/`WSKILLCOST`/`PSICOST` chunks), so the trainer
/// panel and the `TrainerPurchase` effect handler price upgrades from the
/// same authored data. `None` if the gamesys lacks the chunks.
#[derive(Unique, Clone)]
pub struct GlobalTrainerCosts(pub Option<dark::gamesys::TrainerCostTables>);

/// Retail HRM tuning from the gamesys `HRM` chunk, shared with hackable GUIs.
#[derive(Unique, Clone)]
pub struct GlobalHrmParams(pub Option<dark::gamesys::HrmParams>);

/// Retail research-speed tuning from `SKILLPARAM`.
#[derive(Unique, Clone)]
pub struct GlobalSkillParams(pub Option<dark::gamesys::SkillParams>);

impl GlobalTemplateHierarchy {
    /// Whether `template_id` is `class_template_id` or inherits from it.
    pub fn is_or_descends_from(&self, template_id: i32, class_template_id: i32) -> bool {
        template_id == class_template_id
            || dark::ss2_entity_info::get_ancestors(&self.0, &template_id)
                .contains(&class_template_id)
    }
}

impl EffectQueue {
    pub fn push(&mut self, effect: Effect) {
        self.effects.push(effect);
    }

    pub fn flush(&mut self) -> Vec<Effect> {
        let prev = self.effects.clone();
        self.effects = vec![];
        prev
    }
}

/// See `MissionCore::failed_animation_queries`.
struct FailedAnimationGuard {
    key: String,
    frames_left: u32,
    completion_owed: bool,
}

pub struct MissionCore {
    pub level_name: String,
    pub gui: GuiManager,
    pub hit_boxes: HitBoxManager,
    pub rag_doll_manager: RagDollManager,
    pub debug_lines: Vec<DebugLine>,
    pub entity_info: Arc<SystemShock2EntityInfo>,
    pub physics: PhysicsWorld,
    pub script_world: ScriptWorld,
    pub scene_objects: Vec<SceneObject>,
    pub animated_lightmaps: Option<dark::mission::AnimatedLightmapController>,
    pub id_to_animation_player: HashMap<EntityId, AnimationPlayer>,
    /// Failed-animation suppression state per entity: the failing key, a
    /// countdown (frames), and whether a suppressed request is owed a
    /// completion when the window expires. A failed query reports
    /// AnimationCompleted so the requester isn't left hanging, but the AI's
    /// completion handler re-queries - an unresolvable query would otherwise
    /// churn every frame (observed: a creature with no matching idle clips
    /// issuing ~70 queries/second forever). Repeats of the same failure
    /// inside the window get ONE deferred completion at expiry instead:
    /// the loop collapses to ~2 queries/second, while a scripted sequence
    /// legitimately repeating the same failing request still advances
    /// within half a second. A different query or a success resets the
    /// guard.
    failed_animation_queries: HashMap<EntityId, FailedAnimationGuard>,
    pub id_to_model: HashMap<EntityId, Model>,
    pub id_to_bitmap: HashMap<EntityId, Rc<BitmapAnimation>>,
    pub id_to_physics: HashMap<EntityId, RigidBodyHandle>,
    pub id_to_particle_system: HashMap<EntityId, ParticleSystem>,
    #[allow(dead_code)]
    pub template_to_entity_id: HashMap<i32, WrappedEntityId>,
    pub template_name_to_template_id: HashMap<String, EntityMetadata>,
    /// This mission's own objects (positive ids) that author a unique
    /// `P$SymName`, indexed by lowercased name - the fallback an ecology spawn
    /// uses when the gamesys has no archetype under the authored name.
    mission_object_name_to_id: HashMap<String, i32>,
    pub obj_map: HashMap<i32, String>,
    pub world: World,
    pub player_handle: PlayerHandle,
    pub spatial_data: Option<Box<dyn SpatialQueryEngine>>,
    /// Host template -> the particle-group archetypes authored to ride it
    /// (reverse of the archetype `ParticleAttachement` links). Runtime entity
    /// creation instantiates these attached (projectile trails, psi bolt
    /// visuals).
    template_to_particle_riders: HashMap<i32, Vec<(i32, dark::properties::ParticleAttachOptions)>>,
    template_to_particle_attachees: HashMap<i32, Vec<i32>>,
    interaction: Box<dyn PlayerInteraction>,
    pub visibility_engine: Box<dyn VisibilityEngine>,
    pub teleport_system: TeleportSystem,
    pub pending_entity_triggers: Vec<String>,
    pub path_database: Option<dark::mission::PathDatabase>,
    pub pathfinding_service: Option<Arc<PathfindingService>>,
    pub path_visualization: PathVisualizationSystem,
    pub pathfinding_test: crate::mission::pathfinding_test::PathfindingTest,
    /// Sequential index for `Effect::DebugCycleHitboxPose` so each trigger picks
    /// the next animation deterministically (debug hitbox inspection).
    pub debug_pose_index: u32,

    /// Sequential index for `Effect::DebugCycleWeapon` so each trigger wields the
    /// next weapon in `DEBUG_WEAPONS` (flat-mode aim/viewmodel testing).
    pub debug_weapon_index: usize,

    /// First-person animation for the flat melee viewmodel: the wielded melee
    /// entity and its motion player (loops the player-melee idle; a swing is
    /// queued on attack and auto-returns to idle). `None` for guns / no weapon.
    flat_melee_anim: Option<(EntityId, AnimationPlayer)>,

    /// "Use" (metagame) mode, toggled by `Effect::ToggleUseMode` (Tab on
    /// flat; left-controller X in VR). One mode, two presentations: flat
    /// shows the cursor + top-docked inventory strip on screen
    /// (`projects/flat-ui.md`); VR presents the same strip canvas on a
    /// head-anchored world panel - the "cyber interface" - with the world
    /// dimmed behind it and the wielded weapon safed. Unlike the pause menu,
    /// the world keeps simulating while it is up.
    pub use_mode: bool,

    /// Entry/exit feel for `use_mode`: one eased 0..1 ramp driving the rim
    /// vignette (both presentations), the VR comfort dim's strength, and the
    /// flat FOV pull - see [`crate::ui::entry_ramp`]. Advanced every update
    /// regardless of presentation or `use_mode` itself, so it keeps easing
    /// out after the panel has already been put away.
    use_mode_ramp: crate::ui::entry_ramp::EntryExitRamp,

    /// Where the VR cyber-interface panel hangs: placed once on entry from
    /// the tracked head pose, world-locked, lazily recentered (rule 3 of the
    /// vr-ui-design skill). Reset on every entry so the panel is placed from
    /// the pose the mode is opened with. Unused in flat presentation.
    vr_use_mode_anchor: crate::ui::FrontendPanelAnchor,

    /// The head pose from the latest update, in pawn space, for the comfort
    /// dim behind the VR use-mode panel (the dim follows the live gaze; see
    /// `crate::ui::world_dim`). Reset to untracked on entry so a stale pose
    /// can't hang the dim where the player stood last time.
    vr_use_mode_head: (Vector3<f32>, Quaternion<f32>),

    /// VR weapon-safe latch: while use mode is up the hands see zeroed
    /// triggers, and this stays set on exit until the player releases the
    /// trigger - so a trigger held across the exit cannot read as a fresh
    /// rising edge and fire the wielded weapon (the pause menu's
    /// `closed_under_a_held_press` rule, applied to the hands).
    vr_trigger_swallow: bool,

    /// The cyber interface's pointer for this frame: every tracked
    /// controller's ray against the panel, and which one the canvas is
    /// listening to. `None` outside the mode. Resolved once per frame by
    /// [`crate::ui::vr_frontend_pointer_pass`] - the same pass the frontend
    /// screens use - so the canvas hit-test, the hand arbitration and the
    /// drawn beam/dot can never disagree about where the player is pointing
    /// (rule 5 of the vr-ui-design skill).
    vr_use_mode_pointer: Option<crate::ui::FrontendPointerPass>,

    /// VR grab swallow, per hand (indexed by [`hand_slot`]): while the cyber
    /// interface masks a hand's squeeze, that hand keeps seeing a released
    /// squeeze until the player actually lets go. World grabbing is
    /// *level*-triggered (`VirtualHand` grabs whenever the squeeze is down), so
    /// without this a squeeze begun on the panel would grab whatever the ray
    /// hits the instant it slipped off the panel edge - or the moment the mode
    /// closed. Cleared as soon as the hand releases, or as soon as it is
    /// holding something: a hand that just took an item off the panel must see
    /// its own squeeze again, or the eventual release would not drop it.
    vr_squeeze_swallow: [bool; 2],

    /// Flat-mode MFD panel host: the object-bound panel opened on frob, its
    /// canvas rendering, and the pointer -> GUIHover input mapping. VR uses
    /// `GuiManager` for the corresponding object-bound world panel.
    /// See `projects/flat-ui.md` §5.2.
    pub flat_ui: crate::mission::flat_ui_host::FlatUiHost,

    /// Whether ordinary movement/hand/pointer input reaches the active player.
    /// Authored sequences such as eng2's Many ride temporarily suppress it.
    player_controls_enabled: bool,

    /// White transition cover shared by both flat and per-eye VR rendering.
    screen_fade_alpha: f32,
    screen_fade_texture: Rc<dyn TextureTrait>,
}

pub struct GlobalContext {
    /// Typed to the asset-path layer's boxed reader rather than `BufReader<File>`,
    /// because `dark::mission::read` ties the property definitions to the reader
    /// type - and on a 25AE install the gamesys and missions come out of a KPF,
    /// not off disk.
    pub properties: Vec<Box<dyn PropertyDefinition<Box<dyn ReadableAndSeekable>>>>,
    pub links: Vec<Box<dyn LinkDefinition>>,
    pub links_with_data: Vec<Box<dyn LinkDefinitionWithData>>,
    pub gamesys: Gamesys,
    pub motiondb: MotionDB,
}

// [Gate A / PR S3] Background level loading parses on a worker thread that borrows the
// shared `GlobalContext` (gamesys + property/link definitions), so it must be
// `Send + Sync`. This compile-time assertion guards that.
const _: fn() = || {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<GlobalContext>();
};

fn restore_saved_script_namespaces(
    script_world: &mut ScriptWorld,
    mission_states: &[scripts::SavedScriptState],
    mission_entity_ids: &HashMap<EntityId, EntityId>,
    held_states: &[scripts::SavedScriptState],
    held_entity_ids: &HashMap<EntityId, EntityId>,
) {
    script_world
        .restore_states(mission_states, mission_entity_ids)
        .unwrap_or_else(|error| panic!("unable to restore mission script state: {error}"));
    // A revisited mission and the carried inventory were serialized from
    // different ECS worlds, so their saved EntityId values may legitimately
    // overlap. Keep their remap domains separate while hydrating scripts.
    script_world
        .restore_states(held_states, held_entity_ids)
        .unwrap_or_else(|error| panic!("unable to restore held-item script state: {error}"));
}

/// Reinstall the interaction controller's held-item state from a save.
///
/// Restoring both hands through a flat controller wields the second entity and
/// produces effects that holster the displaced first entity. The caller must
/// apply those effects after `PlayerInfo` has installed the backpack entity.
fn restore_held_item_interaction(
    interaction: &mut dyn PlayerInteraction,
    world: &World,
    left_hand_entity: Option<EntityId>,
    right_hand_entity: Option<EntityId>,
) -> Vec<VirtualHandEffect> {
    let mut effects = Vec::new();
    if let Some(entity_id) = left_hand_entity {
        effects.extend(interaction.grab(world, entity_id, vr_config::Handedness::Left));
    }
    if let Some(entity_id) = right_hand_entity {
        effects.extend(interaction.grab(world, entity_id, vr_config::Handedness::Right));
    }
    effects
}

pub struct AbstractMission {
    pub scene_objects: Vec<SceneObject>,
    pub animated_lightmaps: Option<dark::mission::AnimatedLightmapController>,
    pub song_params: SongParams,
    pub room_db: RoomDatabase,
    pub map_params: dark::mission::MapParams,
    pub physics_geometry: Option<Collider>,
    pub spatial_data: Option<Box<dyn SpatialQueryEngine>>,
    pub entity_info: SystemShock2EntityInfo,
    pub obj_map: HashMap<i32, String>,
    pub visibility_engine: Box<dyn VisibilityEngine>,
    pub path_database: Option<dark::mission::PathDatabase>,
}

impl MissionCore {
    pub fn load(
        mission: String,
        abstract_mission: AbstractMission,
        asset_cache: &mut AssetCache,
        audio_context: &mut AudioContext<EntityId, String>,
        global_context: &GlobalContext,
        spawn_loc: SpawnLocation,
        quest_info: QuestInfo,
        entity_populator: Box<dyn EntityPopulator>,
        held_item_save_data: HeldItemSaveData,
        game_options: &GameOptions,
    ) -> MissionCore {
        engine::platform::service_events();
        let game_entity_info = &global_context.gamesys;
        let _motiondb = &global_context.motiondb;

        let mut world = World::new();
        let start = SystemTime::now();
        info!("starting level load");
        let scene = abstract_mission.scene_objects;
        let mut animated_lightmaps = abstract_mission.animated_lightmaps;
        let duration: Duration = start.elapsed().unwrap();
        info!("loading level took {}s", duration.as_secs_f32());

        let entity_info =
            ss2_entity_info::merge_with_gamesys(&abstract_mission.entity_info, game_entity_info);
        let entity_info_rc = Arc::new(entity_info);
        engine::platform::service_events();

        let speech_registry = SpeechVoiceRegistry::from_entity_info(&entity_info_rc);
        let screen_fade_texture: Rc<dyn TextureTrait> = Rc::new(init_from_memory2(
            RawTextureData {
                width: 1,
                height: 1,
                bytes: vec![255, 255, 255, 255],
                format: PixelFormat::RGBA,
            },
            &TextureOptions {
                wrap: false,
                ..Default::default()
            },
        ));

        let mut id_to_model = HashMap::new();
        let mut id_to_animation_player = HashMap::new();

        // Create player
        let player_entity = world.add_entity((PropLocalPlayer {}, RuntimePropDoNotSerialize {}));

        // Seed the player's psi pool and hit points from `The Player`
        // template, where the gamesys authors the starting/maximum values
        // (P$PsiState, P$HitPoints/P$MAX_HP). The HP pool is what psi
        // burnout (and later, real damage handling) drains; nothing yet
        // reacts to it reaching zero.
        {
            use crate::scripts::script_util::hydrate_template_component;
            if let Some(psi_state) = hydrate_template_component::<dark::properties::PropPsiState>(
                THE_PLAYER_TEMPLATE_ID,
                &entity_info_rc,
            ) {
                world.add_component(player_entity, psi_state);
            }
            if let Some(hp) = hydrate_template_component::<dark::properties::PropHitPoints>(
                THE_PLAYER_TEMPLATE_ID,
                &entity_info_rc,
            ) {
                world.add_component(player_entity, hp);
            }
            if let Some(max_hp) = hydrate_template_component::<dark::properties::PropMaxHitPoints>(
                THE_PLAYER_TEMPLATE_ID,
                &entity_info_rc,
            ) {
                world.add_component(player_entity, max_hp);
            }
        }

        // Create a map of template name (ie 'HE Explosion' to the template id).
        // This is important for creating entities based on template name
        let (template_name_to_template_id, unique_gamesys_template_names) =
            create_template_name_map(game_entity_info);
        let mission_object_name_to_id = create_mission_object_name_map(&entity_info_rc);

        world.add_unique(GlobalEntityMetadata(template_name_to_template_id.clone()));
        world.add_unique(Time::default());
        world.add_unique(speech_registry);
        world.add_unique(DebugOptions {
            debug_ai: game_options.debug_ai,
        });
        world.add_unique(GlobalPresentationMode(game_options.presentation_mode));
        let template_class_tags = create_template_class_tag_map(&entity_info_rc);
        world.add_unique(GlobalTemplateClassTags(template_class_tags));
        world.add_unique(
            crate::mission::reload::GlobalProjectileClips::from_entity_info(&entity_info_rc),
        );
        world.add_unique(
            crate::mission::stim_response::GlobalContactStims::from_entity_info(&entity_info_rc),
        );
        // Reuse the obj-icons already hydrated into the template metadata above
        // (keyed by template id) rather than rescanning every template.
        let template_obj_icons: HashMap<i32, String> = template_name_to_template_id
            .values()
            .filter_map(|m| m.obj_icon.clone().map(|icon| (m.template_id, icon)))
            .collect();
        world.add_unique(GlobalTemplateObjIcons(template_obj_icons));
        world.add_unique(GlobalTemplateHierarchy(
            ss2_entity_info::get_hierarchy(&entity_info_rc).clone(),
        ));
        world.add_unique(GlobalTrainerCosts(
            game_entity_info.trainer_costs().cloned(),
        ));
        world.add_unique(GlobalHrmParams(game_entity_info.hrm_params().cloned()));
        world.add_unique(GlobalSkillParams(game_entity_info.skill_params().cloned()));
        let (mut psi_powers, psi_selection) = crate::psi::build_psi_power_registry(&entity_info_rc);
        // Player-facing discipline names come from the psihelp string table;
        // a data install without it just keeps the gamesys symbolic names.
        if let Some(psi_strings) =
            asset_cache.get_opt(&dark::importers::STRINGS_IMPORTER, "psihelp.str")
        {
            crate::psi::apply_display_names(&mut psi_powers, &psi_strings);
        }
        // Debug scenes unlock every power (debug_psi exercises the whole
        // registry); real missions start with the player template's learned
        // bits plus the default OSA loadout (Cryokinesis).
        let mut known_powers = crate::psi::build_known_powers(
            &entity_info_rc,
            &psi_powers,
            THE_PLAYER_TEMPLATE_ID,
            mission.starts_with("debug_"),
        );

        // Apply the career (Marine/Navy/OSA) chosen at the station recruit deck.
        // The choice is persisted as a quest bit, so it re-applies on every
        // deployment; with no career selected the player template defaults
        // (30 HP, 40/50 psi) stand. Absolute values keep re-application
        // idempotent across level loads. These are the deliberate first-career
        // and legacy-save defaults; ordinary transitions and current-format
        // saves restore their exact persisted pools after mission construction.
        if let Some(career) = crate::career::Career::from_quest_info(&quest_info) {
            let loadout = career.loadout();
            world.run(
                |mut v_hp: ViewMut<dark::properties::PropHitPoints>,
                 mut v_max_hp: ViewMut<dark::properties::PropMaxHitPoints>,
                 mut v_psi: ViewMut<dark::properties::PropPsiState>| {
                    if let Ok(hp) = (&mut v_hp).get(player_entity) {
                        hp.hit_points = loadout.max_hit_points;
                    }
                    if let Ok(max_hp) = (&mut v_max_hp).get(player_entity) {
                        max_hp.hit_points = loadout.max_hit_points as u32;
                    }
                    if let Ok(psi) = (&mut v_psi).get(player_entity) {
                        psi.psi_points = loadout.max_psi_points;
                        psi.max_psi_points = loadout.max_psi_points;
                    }
                },
            );
            if let Some(power) = loadout.extra_psi_power {
                known_powers.0.insert(power);
            }
            info!("Applied {:?} career loadout to player", career);
        }

        // O/S trait bonuses re-derive on every load, like the career loadout
        // (the player entity is rebuilt each mission load; the traits
        // themselves persist on the character sheet in QuestInfo). Applied
        // after the career block so Tank adds on top of the career pool.
        if quest_info
            .player_stats()
            .has_os_trait(crate::scripts::gui::TRAIT_TANK)
        {
            use crate::scripts::gui::TANK_HP_BONUS;
            world.run(
                |mut v_hp: ViewMut<dark::properties::PropHitPoints>,
                 mut v_max_hp: ViewMut<dark::properties::PropMaxHitPoints>| {
                    if let Ok(hp) = (&mut v_hp).get(player_entity) {
                        hp.hit_points += TANK_HP_BONUS;
                    }
                    if let Ok(max_hp) = (&mut v_max_hp).get(player_entity) {
                        max_hp.hit_points += TANK_HP_BONUS as u32;
                    }
                },
            );
            info!("Applied Tank O/S trait: +{} max hit points", TANK_HP_BONUS);
        }

        world.add_unique(psi_powers);
        world.add_unique(psi_selection);
        world.add_unique(known_powers);
        world.add_unique(crate::psi::ActivePsiPowers::default());
        world.add_unique(crate::scripts::healing_item::ActiveHealing::default());

        // ** Entity creation

        let population = entity_populator.populate(
            &entity_info_rc,
            &abstract_mission.entity_info,
            &abstract_mission.obj_map,
            &mut world,
        );
        engine::platform::service_events();
        let template_to_entity_id = population.template_to_entity_id;
        let mission_script_entity_id_map = population.entity_id_map;
        let mission_saved_script_states = population.script_states;

        // Instantiate held items
        let mut interaction: Box<dyn PlayerInteraction> =
            if game_options.presentation_mode == crate::PresentationMode::Flat {
                Box::new(FlatInteraction::new())
            } else {
                Box::new(VrInteraction::new())
            };
        let held_instantiation = held_item_save_data
            .instantiate_with_legacy_template_names(&mut world, &unique_gamesys_template_names);
        let left_hand_entity = held_instantiation.left_hand_entity_id;
        let right_hand_entity = held_instantiation.right_hand_entity_id;
        let maybe_inventory_entity = held_instantiation.inventory_entity_id;
        let held_entities: Vec<_> = held_instantiation.entity_id_map.values().copied().collect();
        crate::research::backfill_legacy_held_research_components(
            &mut world,
            &held_entities,
            &entity_info_rc,
        );
        let held_script_entity_id_map = held_instantiation.entity_id_map;
        let held_saved_script_states = held_instantiation.script_states;

        // Instantiate inventory
        // TODO: This should be move into the held_item_save_data
        let inventory = if let Some(inv_entity) = maybe_inventory_entity {
            inv_entity
        } else {
            PlayerInventoryEntity::create(&mut world)
        };

        // HACK: Re-add runtime prop transform for inventory
        world.add_component(
            inventory,
            RuntimePropTransform(Matrix4::from_translation(vec3(0.0, 1.0, 0.0))),
        );
        world.add_component(inventory, PlayerInventoryEntity {});

        // The synthetic automap panel entity (projects/flat-ui-panels.md §5):
        // carries `MapGui` (script `internal_map`) plus the level's page data.
        // No world object opens the map - `Effect::ToggleMap` binds it to the
        // flat host as an unbound (sticky) panel. Never serialized: rebuilt
        // here on every load. Flat-only: in VR the panel cannot be opened
        // (ToggleMap is flat-gated), and under `--experimental gui` its
        // per-frame SetUI would otherwise materialize an undismissable world
        // quad at the origin.
        world.add_unique(PlayerMapLocation::default());
        if game_options.presentation_mode == crate::PresentationMode::Flat {
            let level_stem = mission.split('.').next().unwrap_or(&mission).to_uppercase();
            let (revealed_rects, explored_rects) =
                dark::map::MapChunkData::load_from_mission(asset_cache, &level_stem)
                    .map(|data| (data.revealed_rects, data.explored_rects))
                    .unwrap_or_default();
            let entity = world.add_entity((
                Links::empty(),
                PropScripts {
                    scripts: vec!["internal_map".to_owned()],
                    inherits: false,
                },
                dark::properties::PropTemplateId { template_id: -1 },
                PropPosition {
                    position: vec3(0.0, 0.0, 0.0),
                    rotation: Quaternion {
                        v: vec3(0.0, 0.0, 0.0),
                        s: 1.0,
                    },
                    cell: 0,
                },
                RuntimePropTransform(Matrix4::identity()),
                RuntimePropDoNotSerialize,
                crate::runtime_props::RuntimePropMapData {
                    mission: mission.clone(),
                    map_params: abstract_mission.map_params,
                    revealed_rects,
                    explored_rects,
                },
            ));
            world.add_unique(MapPanelEntity(entity));
        }

        // Both presentations share the exact MediaGui canvas. Flat docks this
        // synthetic host in its MFD slot; VR positions the same host in front
        // of the player's current gaze and routes it through GuiManager.
        let media_panel = world.add_entity((
            Links::empty(),
            PropScripts {
                scripts: vec!["internal_media".to_owned()],
                inherits: false,
            },
            dark::properties::PropTemplateId { template_id: -1 },
            PropSymName("Audio Log Reader".to_owned()),
            PropPosition {
                position: vec3(0.0, 0.0, 0.0),
                rotation: Quaternion::new(1.0, 0.0, 0.0, 0.0),
                cell: 0,
            },
            RuntimePropTransform(Matrix4::identity()),
            RuntimePropDoNotSerialize,
        ));
        world.add_unique(MediaPanelEntity(media_panel));

        world.add_unique(GlobalTemplateIdMap(template_to_entity_id.clone()));

        // Give the player the links `The Player` archetype authors - chiefly
        // its receptrons (inherited from `Human Vulnerability`), which are what
        // decide how hard an incoming stim hits. Without them the player is
        // inert to every act/react damage source in the gamesys.
        entity_creator::initialize_links_for_entity(
            THE_PLAYER_TEMPLATE_ID,
            player_entity,
            &entity_info_rc,
            &template_to_entity_id,
            &mut world,
        );

        // Start background music
        initialize_background_music(&abstract_mission.song_params, asset_cache, audio_context);

        let mut entities_to_instantiate = HashSet::new();

        // Create rooms
        create_room_entities(
            &abstract_mission.room_db,
            &template_to_entity_id,
            &mut world,
            &mut entities_to_instantiate,
        );

        // Containment at creation: anything a `Contains` link points at (loot
        // in corpses/containers, items carried by live AIs, backpack
        // contents) has no world presence until taken or dropped. Runs before
        // the instantiation loop below so neither models nor physics bodies
        // are created for contained items.
        entity_creator::suppress_contained_entity_world_presence(&mut world);

        // Get the set of entities with PropPosition to be materialized
        world.run(
            |v_pos: View<dark::properties::PropPosition>,
             v_template_id: View<dark::properties::PropTemplateId>| {
                for (id, (_pos, template_id)) in (&v_pos, &v_template_id).iter().with_id() {
                    entities_to_instantiate.insert((id, template_id.template_id));
                }
            },
        );

        let mut physics = PhysicsWorld::new();
        let mut id_to_physics = HashMap::new();
        let mut id_to_bitmap = HashMap::new();
        let mut script_world = ScriptWorld::new();
        // The player has no `P$Scripts` in the gamesys, so give it its damage
        // handler explicitly - otherwise Damage messages sent to the player
        // are dropped and nothing can hurt it.
        script_world.add_entity2(
            player_entity,
            Box::new(crate::scripts::player_script::PlayerScript),
        );

        let world_entity_id = world.add_entity(RuntimePropDoNotSerialize {});
        if let Some(collider) = abstract_mission.physics_geometry {
            physics.add_collider(world_entity_id, collider);
        }

        // Finally, instantiate these entities
        for (entity_index, (entity_id, template_id)) in
            entities_to_instantiate.into_iter().enumerate()
        {
            if entity_index % LOAD_EVENT_PUMP_ENTITY_INTERVAL == 0 {
                engine::platform::service_events();
            }
            let created_entity = entity_creator::initialize_entity(
                entity_id,
                template_id,
                &mut world,
                &mut physics,
                asset_cache,
                &mut script_world,
                &entity_info_rc,
                &abstract_mission.obj_map,
                &template_to_entity_id,
                CreateEntityOptions::default(),
            );

            Self::finish_instantiating_entity(
                &mut id_to_model,
                &mut id_to_bitmap,
                &mut id_to_physics,
                &mut id_to_animation_player,
                &mut physics,
                &mut world,
                &mut script_world,
                created_entity,
                Matrix4::identity(),
            );
        }
        engine::platform::service_events();

        restore_saved_script_namespaces(
            &mut script_world,
            &mission_saved_script_states,
            &mission_script_entity_id_map,
            &held_saved_script_states,
            &held_script_entity_id_map,
        );

        // Restore the interaction controller now, but defer its effects until
        // PlayerInfo below has installed the backpack they may store into.

        let held_restore_effects = restore_held_item_interaction(
            interaction.as_mut(),
            &world,
            left_hand_entity,
            right_hand_entity,
        );

        if let Some(entity_id) = left_hand_entity {
            restore_held_item_physics(&world, &mut id_to_physics, &mut physics, entity_id);
        };

        if let Some(entity_id) = right_hand_entity {
            restore_held_item_physics(&world, &mut id_to_physics, &mut physics, entity_id);
        };

        let (start_pos, start_rotation) = spawn_loc.calculate_start_position(
            &world,
            &abstract_mission.entity_info,
            &template_to_entity_id,
        );

        let player_handle = physics.create_player(start_pos, player_entity);

        world.add_unique(PlayerInfo {
            rotation: start_rotation,
            pos: start_pos,
            entity_id: player_entity,
            left_hand_entity_id: None,
            right_hand_entity_id: None,
            inventory_entity_id: inventory,
        });
        world.add_unique(PlayerLifeState::Alive);

        world.add_unique(quest_info);

        // Current saves record the width that encoded their backpack link
        // ordinals. Pre-#948 saves omit it and used the former fixed width 15;
        // migrate both shapes before any restored interaction can add items.
        let current_backpack_width = world
            .borrow::<UniqueView<QuestInfo>>()
            .map(|quests| crate::inventory::backpack_width(quests.player_stats()))
            .unwrap_or(crate::inventory::BACKPACK_GRID.0);
        let backpack_load_remap = held_item_save_data.remap_instantiated_backpack(
            &mut world,
            inventory,
            current_backpack_width,
        );

        crate::scripts::gui::restore_authored_computer_data(
            &mut world,
            &entity_info_rc,
            &template_to_entity_id,
        );

        // Preload the elevator floor labels (MISC.STR) + current mission so the
        // AssetCache-less ElevatorGui can label/gate floors at draw time.
        world.add_unique(crate::scripts::ElevatorContext::load(asset_cache, &mission));
        world.add_unique(crate::scripts::gui::TraitsContext::load(asset_cache));
        world.add_unique(crate::scripts::gui::ComputerContext::load(asset_cache));

        world.add_unique(EffectQueue {
            effects: Vec::new(),
        });

        // Initialize teleport system based on game options
        let teleport_system = if game_options.experimental_features.contains("teleport") {
            let teleport_config = crate::teleport::TeleportConfig {
                enabled: true,
                button_mapping: crate::teleport::TeleportButton::Trigger,
                trigger_threshold: 0.5,
                max_distance: 20.0,
                ..Default::default()
            };
            TeleportSystem::new(teleport_config)
        } else {
            let teleport_config = crate::teleport::TeleportConfig {
                enabled: false,
                ..Default::default()
            };
            TeleportSystem::new(teleport_config)
        };

        // Mission-placed particle entities follow the object their concrete
        // ParticleAttachement link names (steam rides its machinery): bolt
        // them with RuntimePropAttachment, preserving the authored relative
        // pose. Physical attachments are registered directly with Rapier
        // below; their bodies then feed the normal physics-to-render sync.
        {
            let mut attachments: Vec<(EntityId, EntityId, Matrix4<f32>)> = Vec::new();
            let mut physical_attachments: Vec<(EntityId, EntityId, Vector3<f32>)> = Vec::new();
            {
                let v_links = world.borrow::<View<Links>>().unwrap();
                let v_transform = world.borrow::<View<RuntimePropTransform>>().unwrap();
                for (id, links) in v_links.iter().with_id() {
                    for link in &links.to_links {
                        let Some(parent) = link.to_entity_id else {
                            continue;
                        };
                        if let dark::properties::Link::PhysAttach(options) = &link.link {
                            physical_attachments.push((id, parent.0, options.offset));
                            continue;
                        }
                        if !matches!(link.link, dark::properties::Link::ParticleAttachement(_)) {
                            continue;
                        }
                        let (Ok(child_xform), Ok(parent_xform)) =
                            (v_transform.get(id), v_transform.get(parent.0))
                        else {
                            continue;
                        };
                        if let Some(inv_parent) = parent_xform.0.invert() {
                            attachments.push((id, parent.0, inv_parent * child_xform.0));
                        }
                    }
                }
            }
            for (child, parent, local_transform) in attachments {
                world.add_component(
                    child,
                    crate::runtime_props::RuntimePropAttachment {
                        parent,
                        local_transform,
                    },
                );
            }
            for (child, parent, offset) in physical_attachments {
                if !physics.attach_kinematic(child, parent, offset) {
                    warn!(
                        "ignoring PhysAttach {:?} -> {:?}: both objects need kinematic physics bodies",
                        child, parent
                    );
                }
            }
        }

        // Reverse map of the archetype ParticleAttachement links: host template
        // -> the particle-group archetypes that ride it. Concrete (mission-
        // placed) links are excluded - those particle entities already exist
        // in the level and just follow their host (see the attachment pass
        // below).
        let mut template_to_particle_riders: HashMap<
            i32,
            Vec<(i32, dark::properties::ParticleAttachOptions)>,
        > = HashMap::new();
        // Forward map: particle archetype -> the archetypes its own
        // ParticleAttachement links point AT. Normally the attach target
        // exists first and the reverse map above spawns the particle riding
        // it (trails riding projectiles), but a transient impact FX is the
        // root of its own creation - nothing else ever creates its attach
        // target. The shipped data has exactly one such pair (the bullet
        // spang's ParticleAttachement to the "Bullet Hit" decal), and the
        // decal only appears if the spang's creation brings it along.
        let mut template_to_particle_attachees: HashMap<i32, Vec<i32>> = HashMap::new();
        for (src_template, links) in &entity_info_rc.template_to_links {
            if *src_template >= 0 {
                continue;
            }
            for link in &links.to_links {
                if let dark::properties::Link::ParticleAttachement(opts) = &link.link {
                    if link.to_template_id < 0 {
                        template_to_particle_riders
                            .entry(link.to_template_id)
                            .or_default()
                            .push((*src_template, *opts));
                        template_to_particle_attachees
                            .entry(*src_template)
                            .or_default()
                            .push(link.to_template_id);
                    }
                }
            }
        }

        let nav_bridges = game_options.experimental_features.contains("nav_bridges");
        // Vet nav crossings (island bridges, relaxed blocking-cell
        // traversal) against real geometry: a torso-height ray between the
        // two points must be clear, so routes can't run through railings,
        // thin walls (issue #481), or furniture spanning the corridor
        // (issue #489). Doors and creatures don't count as blockers - doors
        // open at runtime and creatures wander off - so the ray advances
        // past those hits (bounded).
        let nav_validator = |from: cgmath::Vector3<f32>, to: cgmath::Vector3<f32>| -> bool {
            let delta = to - from;
            let total = delta.magnitude();
            if total <= f32::EPSILON {
                return true;
            }
            let direction = delta / total;
            let mut origin = Point3::new(from.x, from.y, from.z);
            let mut remaining = total;
            // A crossing rarely stacks more than a couple of pass-through
            // entities; bail as blocked beyond that
            for _ in 0..4 {
                let Some(hit) = physics.ray_cast2_as_actor(
                    origin,
                    direction,
                    remaining,
                    crate::physics::InternalCollisionGroups::WORLD
                        | crate::physics::InternalCollisionGroups::ENTITIES,
                    None,
                    true,
                ) else {
                    return true;
                };
                let pass_through = hit
                    .maybe_entity_id
                    .map(|id| {
                        crate::scripts::ai::ai_util::is_entity_door(&world, id)
                            || world
                                .borrow::<View<PropCreature>>()
                                .map(|v| v.contains(id))
                                .unwrap_or(false)
                    })
                    .unwrap_or(false);
                if !pass_through {
                    return false;
                }
                let travelled = (hit.hit_point - origin).magnitude() + 0.05;
                if travelled >= remaining {
                    return true;
                }
                remaining -= travelled;
                origin += direction * travelled;
            }
            false
        };
        let pathfinding_service = abstract_mission.path_database.as_ref().map(|db| {
            Arc::new(PathfindingService::with_nav_options(
                Arc::new(db.clone()),
                nav_bridges,
                Some(&nav_validator),
            ))
        });
        // Steering strategies path through this unique; MissionCore keeps its
        // own handle for the interactive pathfinding test.
        world.add_unique(GlobalPathfinding(pathfinding_service.clone()));
        // Path queries run on a dedicated worker thread; the worker exits
        // when this world (and with it the unique) is dropped
        world.add_unique(GlobalAsyncPathfinding(pathfinding_service.as_ref().map(
            |service| {
                Arc::new(crate::pathfinding::async_queries::AsyncPathfinding::spawn(
                    service.clone(),
                ))
            },
        )));
        // Per-frame AI pathfind budget, refilled at the top of each update
        world.add_unique(crate::pathfinding::PathfindingFrameBudget::new());

        // The atlas is initially packed with static lightmaps only. Restore
        // every instantiated light's persisted value once at load, then later
        // script effects update just the rectangles touched by a state change.
        if let Some(controller) = &mut animated_lightmaps {
            world.run(|lights: View<PropAnimLight>| {
                for light in lights.iter() {
                    controller.set_light_intensity(light.light_number, light.initial_intensity());
                }
            });
            controller.flush();
        }

        let mut mission_core = MissionCore {
            interaction,
            level_name: mission,
            entity_info: entity_info_rc.clone(),
            template_to_particle_riders,
            template_to_particle_attachees,
            script_world,
            id_to_model,
            id_to_animation_player,
            failed_animation_queries: HashMap::new(),
            id_to_bitmap,
            id_to_particle_system: HashMap::new(),
            template_name_to_template_id,
            mission_object_name_to_id,
            scene_objects: scene,
            animated_lightmaps,
            physics,
            world,
            id_to_physics,
            template_to_entity_id,
            player_handle,
            spatial_data: abstract_mission.spatial_data,
            debug_lines: Vec::new(),
            gui: GuiManager::new(),
            hit_boxes: HitBoxManager::new(),
            rag_doll_manager: RagDollManager::new(),
            visibility_engine: abstract_mission.visibility_engine,
            teleport_system,
            pending_entity_triggers: Vec::new(),
            obj_map: abstract_mission.obj_map,
            path_database: abstract_mission.path_database.clone(),
            pathfinding_service,
            path_visualization: PathVisualizationSystem::new(),
            pathfinding_test: crate::mission::pathfinding_test::PathfindingTest::new(),
            debug_pose_index: 0,
            debug_weapon_index: 0,
            flat_melee_anim: None,
            use_mode: false,
            use_mode_ramp: crate::ui::entry_ramp::EntryExitRamp::new(),
            vr_use_mode_anchor: crate::ui::FrontendPanelAnchor::new(),
            vr_use_mode_head: crate::ui::world_dim::UNTRACKED_HEAD,
            vr_trigger_swallow: false,
            vr_use_mode_pointer: None,
            vr_squeeze_swallow: [false; 2],
            flat_ui: crate::mission::flat_ui_host::FlatUiHost::new(),
            player_controls_enabled: true,
            screen_fade_alpha: 0.0,
            screen_fade_texture,
        };
        for (index, entity_id) in backpack_load_remap.overflow.into_iter().enumerate() {
            mission_core.spill_backpack_overflow(entity_id, index);
        }
        mission_core.process_virtual_hand_effects(asset_cache, held_restore_effects);
        // Re-run each restored held item's Hold script. Only a fresh world
        // grab dispatches Hold (see `restore_held_item_physics` for the same
        // restore gap), so without this a save/load or level transition loses
        // whatever Hold set up: the save carries the swapped first-person
        // PropModelName, but the model was rebuilt through the plain importer
        // - spare baked hands back - and the cross-mode heal never ran. The
        // handlers are idempotent for an already-wielded item.
        for entity_id in [left_hand_entity, right_hand_entity].into_iter().flatten() {
            mission_core.script_world.dispatch(Message {
                payload: MessagePayload::Hold,
                to: entity_id,
            });
        }
        engine::platform::service_events();
        mission_core
    }

    fn player_is_alive(&self) -> bool {
        self.world
            .borrow::<UniqueView<PlayerLifeState>>()
            .map(|life| life.is_alive())
            .unwrap_or(true)
    }

    /// Enter the death state once. An active QBR is usable only when the
    /// player can atomically pay the retail 10-nanite reconstruction cost;
    /// otherwise death is terminal and runs the death sequence that ends in
    /// the game-over screen.
    ///
    /// Returns the death feedback to play (the authored death vocalization).
    fn begin_player_death(&mut self) -> Vec<Effect> {
        if !self.player_is_alive() {
            return Vec::new();
        }

        let paid_respawn = active_resurrection_target(&self.world).and_then(|target| {
            crate::scripts::script_util::debit_player_nanites(
                &self.world,
                PLAYER_RESPAWN_NANITE_COST,
            )
            .map(|exhausted| (target, exhausted))
        });

        let next_state = if let Some(((position, rotation), exhausted)) = paid_respawn {
            for entity_id in exhausted {
                self.destroy_entity(entity_id);
            }
            info!(
                "Player died: QBR reconstruction queued at {:?} (-{} nanites)",
                position, PLAYER_RESPAWN_NANITE_COST
            );
            PlayerLifeState::Respawning {
                elapsed_seconds: 0.0,
                position,
                rotation,
            }
        } else {
            info!("Player died: no activated and affordable QBR");
            PlayerLifeState::Dead {
                elapsed_seconds: 0.0,
            }
        };

        *self
            .world
            .borrow::<UniqueViewMut<PlayerLifeState>>()
            .unwrap() = next_state;

        vec![Effect::PlaySound {
            handle: AudioHandle::new(),
            source: None,
            name: PLAYER_DEATH_SCHEMAS
                .choose(&mut thread_rng())
                .expect("player death schemas are never empty")
                .to_string(),
            spatial: false,
        }]
    }

    /// Advance the death pause: perform the QBR reconstruction once its retail
    /// five seconds elapse, or hand a terminal death off to the game-over
    /// screen once its death sequence elapses.
    fn update_player_life_state(&mut self, elapsed_seconds: f32) -> Vec<Effect> {
        enum DeathPause {
            Pending,
            Reconstruct(Vector3<f32>, Quaternion<f32>),
            GameOver,
        }

        let pause = {
            let mut life = self
                .world
                .borrow::<UniqueViewMut<PlayerLifeState>>()
                .unwrap();
            match &mut *life {
                PlayerLifeState::Respawning {
                    elapsed_seconds: elapsed,
                    position,
                    rotation,
                } => {
                    *elapsed += elapsed_seconds;
                    if *elapsed + f32::EPSILON >= PLAYER_RESPAWN_DELAY_SECONDS {
                        DeathPause::Reconstruct(*position, *rotation)
                    } else {
                        DeathPause::Pending
                    }
                }
                PlayerLifeState::Dead {
                    elapsed_seconds: elapsed,
                } => {
                    *elapsed += elapsed_seconds;
                    if *elapsed + f32::EPSILON >= PLAYER_DEATH_SEQUENCE_SECONDS {
                        DeathPause::GameOver
                    } else {
                        DeathPause::Pending
                    }
                }
                PlayerLifeState::Alive | PlayerLifeState::GameOver => DeathPause::Pending,
            }
        };

        let (position, rotation) = match pause {
            DeathPause::Pending => return Vec::new(),
            DeathPause::GameOver => {
                info!("Player death is terminal: showing the game-over screen");
                *self
                    .world
                    .borrow::<UniqueViewMut<PlayerLifeState>>()
                    .unwrap() = PlayerLifeState::GameOver;
                return vec![Effect::GlobalEffect(GlobalEffect::GameOver)];
            }
            DeathPause::Reconstruct(position, rotation) => (position, rotation),
        };

        // The authored marker is used verbatim whenever the standing capsule
        // fits there. Reconstruction is the one relocation the player cannot
        // decline, so an obstructed marker would end the run outright: the
        // character controller cannot walk out of a pose it starts inside
        // (#801). Step aside in that case rather than strand them.
        let position = self
            .physics
            .set_player_translation_unobstructed(position, &mut self.player_handle);
        let player_entity = {
            let mut player = self.world.borrow::<UniqueViewMut<PlayerInfo>>().unwrap();
            player.pos = position;
            player.rotation = rotation;
            player.entity_id
        };
        self.world.add_component(
            player_entity,
            PropTeleported::with_source(TeleportSource::ScriptedTrap),
        );
        self.world.run(
            |mut hit_points: ViewMut<PropHitPoints>,
             max_hit_points: View<dark::properties::PropMaxHitPoints>| {
                let maximum = max_hit_points
                    .get(player_entity)
                    .map(|max| max.hit_points as i32)
                    .unwrap_or(1)
                    .max(1);
                if let Ok(hit_points) = (&mut hit_points).get(player_entity) {
                    hit_points.hit_points = (maximum / 2).max(1);
                }
            },
        );
        *self
            .world
            .borrow::<UniqueViewMut<PlayerLifeState>>()
            .unwrap() = PlayerLifeState::Alive;
        info!("Player reconstructed at QBR {:?}", position);
        Vec::new()
    }

    pub fn update(
        &mut self,
        time: &Time,
        asset_cache: &mut AssetCache,
        input_context: &input_context::InputContext,
        game_options: &GameOptions,
        command_effects: Vec<Effect>,
    ) -> Vec<Effect> {
        let _ = self.world.remove_unique::<Time>();
        self.world.add_unique(time.clone());

        // Old saves can legitimately restore a zero-HP player even though
        // PlayerLifeState itself is runtime-only. Enter death on the first
        // update in that case, then advance any pending QBR reconstruction.
        let player_health_depleted = {
            let player = self.world.borrow::<UniqueView<PlayerInfo>>().unwrap();
            self.world
                .borrow::<View<PropHitPoints>>()
                .ok()
                .and_then(|hit_points| {
                    hit_points
                        .get(player.entity_id)
                        .ok()
                        .map(|hit_points| hit_points.hit_points <= 0)
                })
                .unwrap_or(false)
        };
        let mut life_state_effects = Vec::new();
        if self.player_is_alive() && player_health_depleted {
            life_state_effects.append(&mut self.begin_player_death());
        }
        life_state_effects.append(&mut self.update_player_life_state(time.elapsed.as_secs_f32()));

        // A dead player's physical head can still look around in VR, but all
        // actionable movement, hand, trigger, crouch, and pointer channels are
        // neutral until reconstruction. Discrete quick-load remains available
        // because it arrives separately in `command_effects`.
        let suppressed_input = InputContext::default();
        let input_context = if self.player_is_alive() && self.player_controls_enabled {
            input_context
        } else {
            &suppressed_input
        };
        // Refill the per-frame AI pathfind budget - only on advancing frames,
        // so paused zero-dt ticks (debug runtime introspection) can't grant
        // extra query slots between stepped frames
        if time.elapsed.as_secs_f32() > 0.0 {
            if let Ok(budget) = self
                .world
                .borrow::<UniqueView<crate::pathfinding::PathfindingFrameBudget>>()
            {
                budget.reset();
            }
        }

        // Drop AI path records (and their debug visuals) for entities that
        // no longer exist OR are dead, so they don't ghost in
        // GET /v1/ai/paths. A killed monster keeps its entity (the corpse),
        // so liveness alone isn't enough: without the hit-point check a
        // corpse advertises its pre-death route - frozen mid-route, forever
        // - to tooling and tests (issue #481's dominant "frozen AI" class).
        if time.elapsed.as_secs_f32() > 0.0 {
            if let Some(service) = &self.pathfinding_service {
                if let (Ok(entities), Ok(v_hit_points)) = (
                    self.world.borrow::<shipyard::EntitiesView>(),
                    self.world.borrow::<View<PropHitPoints>>(),
                ) {
                    service.prune_ai_paths(|inner| {
                        shipyard::EntityId::from_inner(inner)
                            .map(|id| {
                                entities.is_alive(id)
                                    && v_hit_points
                                        .get(id)
                                        .map(|hp| hp.hit_points > 0)
                                        .unwrap_or(true)
                            })
                            .unwrap_or(false)
                    });
                }
            }
        }

        // Sync live door state into the pathfinding service: impassable
        // doors make their below-door cells unpathable, so A* routes around
        // them (or stops at them) instead of through them. Impassable means
        // locked-and-closed, a zero-travel TransDoor authored closed (e.g.
        // medsci1's space-shield membranes), OR a door this engine cannot
        // operate at all - no runtime entity or no TransDoor prop. Unlocked
        // closed translating doors with real travel stay pathable - pursuing
        // AIs open those on arrival.
        if time.elapsed.as_secs_f32() > 0.0 {
            if let Some(service) = &self.pathfinding_service {
                if let Ok(id_map) = self
                    .world
                    .borrow::<UniqueView<crate::mission::GlobalTemplateIdMap>>()
                {
                    let blocked: std::collections::HashSet<i32> = service
                        .path_database
                        .cell_doors
                        .iter()
                        .map(|cd| cd.door)
                        .filter(|door| {
                            let Some(ent) = id_map.0.get(door).map(|w| w.0) else {
                                // The gating door has no runtime entity:
                                // nothing can ever open it
                                return true;
                            };
                            crate::scripts::script_util::door_blocks_pathfinding(&self.world, ent)
                        })
                        .collect();
                    service.set_blocked_doors(blocked);
                }
            }
        }

        // Mirror each AI's active route into the path visualization (orange),
        // so debug-draw sessions show what every AI is following - the same
        // data GET /v1/ai/paths reports
        if game_options.debug_draw {
            if let Some(service) = &self.pathfinding_service {
                let stale: Vec<String> = self
                    .path_visualization
                    .paths
                    .keys()
                    .filter(|name| name.starts_with("ai_"))
                    .cloned()
                    .collect();
                for name in stale {
                    self.path_visualization.remove_path(&name);
                }
                for (entity, record) in service.ai_paths() {
                    if record.waypoints.len() >= 2 {
                        let name = format!("ai_{entity}");
                        self.path_visualization.set_path(
                            name.clone(),
                            crate::pathfinding::path_visualization::ComputedPath::new(
                                name,
                                record.waypoints,
                                crate::pathfinding::path_visualization::colors::AI_PATH,
                            ),
                        );
                    }
                }
            }
        }
        // Life-state effects go first so a scene-replacing GameOver cannot
        // clobber a same-frame quick-load arriving in `command_effects`.
        let mut effects = life_state_effects;
        if let Some(healing) = crate::scripts::healing_item::tick_player_healing(
            &self.world,
            time.elapsed.as_secs_f32(),
        ) {
            effects.push(healing);
        }
        effects.extend(command_effects);

        let player = {
            let player_info = self.world.borrow::<UniqueView<PlayerInfo>>().unwrap();
            player_info.clone()
        };

        // Player movement logic
        let delta_time = time.elapsed.as_secs_f32();

        // Update teleport system and add effects (only if experimental flag enabled)
        if game_options.experimental_features.contains("teleport") {
            let teleport_effects =
                self.teleport_system
                    .update(input_context, player.pos, player.rotation, delta_time);
            effects.extend(teleport_effects);
        }
        let rot_speed = 2.0;
        let additional_rotation = cgmath::Quaternion::from_axis_angle(
            cgmath::vec3(0.0, 1.0, 0.0),
            cgmath::Rad(input_context.left_hand.thumbstick.x * delta_time * rot_speed),
        );

        let new_rotation = player.rotation * additional_rotation;

        let dir = new_rotation * input_context.head.rotation;
        let facing = dir.rotate_vector(cgmath::vec3(0.0, 0.0, -1.0));
        let move_thumbstick_value = input_context.right_hand.thumbstick;
        let forward = dir.rotate_vector(cgmath::vec3(
            -delta_time * move_thumbstick_value.x * 25. / dark::SCALE_FACTOR,
            0.0,
            -delta_time * move_thumbstick_value.y * 25. / dark::SCALE_FACTOR,
        ));

        let up_value = input_context.left_hand.thumbstick.y / dark::SCALE_FACTOR;

        // Skip physics while time is frozen (the debug runtime's paused state
        // calls update with zero dt): the Rapier pipeline advances by a fixed
        // internal dt per call regardless of elapsed time, so stepping it here
        // would keep integrating bodies at wall-clock rate while scripts,
        // animations, and particles are frozen - creatures glide across the
        // floor and effects stick around. A paused sim must not move. The
        // player position is still read from the character body (not stepped)
        // so a teleport - which writes the body directly - is reflected in
        // PlayerInfo/introspection even before the next real step.
        let (new_character_pos, collision_events) = if time.elapsed.is_zero() {
            (
                self.physics.get_player_translation(&self.player_handle),
                Vec::new(),
            )
        } else {
            // Apply the crouch request before moving: swaps the capsule size
            // feet-planted; standing up is refused without headroom (the
            // actual state is read back via `player_is_crouched`).
            self.physics
                .set_player_crouch(input_context.crouch, &mut self.player_handle);
            profile!(
                "shock2.update.physics",
                self.physics.update_with_facing_and_jump(
                    forward + cgmath::vec3(0.0, up_value, 0.0),
                    facing,
                    input_context.jump,
                    &mut self.player_handle,
                )
            )
        };

        if !time.elapsed.is_zero() {
            let player_id = self
                .world
                .borrow::<UniqueView<PlayerInfo>>()
                .unwrap()
                .entity_id;
            let living_creatures = self.world.run(
                |v_creature: View<PropCreature>, v_hit_points: View<PropHitPoints>| {
                    (&v_creature, &v_hit_points)
                        .iter()
                        .with_id()
                        .filter_map(|(entity_id, (_, hit_points))| {
                            (entity_id != player_id && hit_points.hit_points > 0)
                                .then_some(entity_id)
                        })
                        .collect::<Vec<_>>()
                },
            );
            self.physics
                .recover_live_creatures_swept_off_support(&living_creatures);
        }

        // Clear one-shot forces - but only after a step actually consumed
        // them. A paused (zero-dt) frame skips physics.update above, and
        // clearing there would silently erase forces queued between frames
        // (e.g. a killing-blow impulse on a multibody link, which is applied
        // as a one-step force) before they ever acted.
        if !time.elapsed.is_zero() {
            self.physics.clear_forces();
        }
        // A poisoned (transform-diverged) rig is despawned by the manager;
        // also delete its otherwise-empty corpse entity so it doesn't leak.
        for poisoned_id in self.rag_doll_manager.update(&mut self.physics) {
            self.world.delete_entity(poisoned_id);
        }

        let (left_hand_entity_id, right_hand_entity_id) = self.interaction.held_entities();

        // Update player info
        let mut player_info = self.world.borrow::<UniqueViewMut<PlayerInfo>>().unwrap();
        player_info.pos = new_character_pos;
        player_info.rotation = new_rotation;
        player_info.left_hand_entity_id = left_hand_entity_id;
        player_info.right_hand_entity_id = right_hand_entity_id;
        drop(player_info);

        // Handle collision events
        for ce in collision_events {
            info!("event: {:?}", ce);

            match ce {
                physics::CollisionEvent::BeginIntersect {
                    sensor_id,
                    entity_id,
                } => {
                    self.script_world.dispatch(Message {
                        to: sensor_id,
                        payload: MessagePayload::SensorBeginIntersect { with: entity_id },
                    });
                }
                physics::CollisionEvent::EndIntersect {
                    sensor_id,
                    entity_id,
                } => {
                    self.script_world.dispatch(Message {
                        to: sensor_id,
                        payload: MessagePayload::SensorEndIntersect { with: entity_id },
                    });
                }
                physics::CollisionEvent::CollisionStarted {
                    entity1_id,
                    entity2_id,
                    contact,
                } => {
                    self.script_world.dispatch(Message {
                        to: entity1_id,
                        payload: MessagePayload::Collided {
                            with: entity2_id,
                            contact,
                        },
                    });
                    self.script_world.dispatch(Message {
                        to: entity2_id,
                        payload: MessagePayload::Collided {
                            with: entity1_id,
                            contact: contact.map(|contact| physics::CollisionContact {
                                point: contact.point,
                                normal: -contact.normal,
                            }),
                        },
                    });
                }
            }
        }

        // Update PropTeleported entities
        self.world.run(
            |mut v_teleported: ViewMut<dark::properties::PropTeleported>| {
                let mut ents_to_remove = Vec::new();
                for (id, door) in (&mut v_teleported).iter().with_id() {
                    // Load restoration is a one-script-update marker, not a
                    // wall-clock timer. It must survive even a slow first
                    // frame so initial sensor overlaps can be reconstructed.
                    if door.source == TeleportSource::LoadRestore {
                        continue;
                    }

                    door.countdown_timer -= time.elapsed.as_secs_f32();

                    if door.countdown_timer < 0.0 {
                        ents_to_remove.push(id);
                    }
                }

                for id in ents_to_remove {
                    v_teleported.remove(id);
                }
            },
        );

        // Tick the player's sustained psi powers down and expire them.
        self.world
            .run(|mut active: UniqueViewMut<crate::psi::ActivePsiPowers>| {
                let dt = time.elapsed.as_secs_f32();
                active.0.retain_mut(|power| {
                    power.remaining_secs -= dt;
                    if power.remaining_secs <= 0.0 {
                        game_log!(INFO, "Psi power expired: {}", power.name);
                        false
                    } else {
                        true
                    }
                });
            });

        effects.extend(update_research(&self.world, time.elapsed.as_secs_f32()));

        let (player_pos, player_rot) = {
            let player_info = self.world.borrow::<UniqueView<PlayerInfo>>().unwrap();
            (player_info.pos, player_info.rotation)
        };

        self.debug_lines.iter_mut().for_each(|p| {
            p.remaining_life_in_seconds -= time.elapsed.as_secs_f32();
        });

        self.debug_lines
            .retain(|p| p.remaining_life_in_seconds > 0.0);

        self.update_animations(time);

        self.hit_boxes.update(
            &mut self.world,
            &mut self.physics,
            &mut self.script_world,
            &self.id_to_model,
            &mut self.id_to_physics,
        );

        // Default VR has one object-bound world-panel slot. Keep its
        // transient proxy faithful to the original overlay lifecycle before
        // either hand raycasts: destroyed hosts and walk-away panels close,
        // while `--experimental gui` retains its legacy all-panels behavior.
        if game_options.presentation_mode == crate::PresentationMode::Vr
            && !game_options.experimental_features.contains("gui")
        {
            self.gui.maintain_active_panel(
                &mut self.world,
                &mut self.physics,
                &mut self.script_world,
                &mut self.id_to_physics,
            );
        }

        // Entry/exit ramp: advanced every update regardless of presentation
        // or `use_mode` itself, so the vignette/dim/FOV pull keeps easing out
        // after the panel has already been put away (see `render`'s
        // `is_settled_closed()` gate).
        self.use_mode_ramp.update(time.elapsed.as_secs_f32());

        // VR cyber interface (use mode): follow the head for the anchor's
        // lazy recenter and the comfort dim, resolve this frame's pointer, and
        // clear the weapon-safe latch once every trigger is released.
        self.vr_use_mode_pointer = None;
        if game_options.presentation_mode == crate::PresentationMode::Vr && self.use_mode {
            self.vr_use_mode_head = (input_context.head.position, input_context.head.rotation);
            let panel = self.vr_use_mode_anchor.update(
                input_context.head.position,
                input_context.head.rotation,
                time.elapsed,
            );
            // The VR ray plays the role of the mouse: one pass resolves where
            // each controller lands on the panel and which one owns it.
            self.vr_use_mode_pointer = Some(crate::ui::vr_pointer_pass(
                input_context,
                crate::mission::flat_ui_host::CANVAS_SIZE,
                &panel,
                // A squeeze claims the panel here, unlike on a frontend screen:
                // it is how a hand takes an item out of a slot.
                crate::ui::PointerEngagement::TriggerOrGrab,
            ));
        }
        if self.vr_trigger_swallow && !self.use_mode {
            let held = |hand: &crate::input_context::Hand| {
                hand.trigger_value > crate::ui::VR_TRIGGER_THRESHOLD
            };
            if !held(&input_context.left_hand) && !held(&input_context.right_hand) {
                self.vr_trigger_swallow = false;
            }
        }

        // Per-hand arbitration (cyber-interface plan, owner decision 4): a hand
        // whose ray is ON the panel is a UI pointer this frame; a hand off it
        // stays an ordinary `VirtualHand` for world grab/drop. Note this is
        // *every* hand on the panel, not just the one driving the point - an
        // idle hand resting on the panel is not the pointer, but its squeeze
        // must still not reach through the panel into the world.
        let (left_hand_held, right_hand_held) = self.interaction.held_entities();
        let hand_empty = [left_hand_held.is_none(), right_hand_held.is_none()];
        let mut on_panel = [false; 2];
        if let Some(pass) = self.vr_use_mode_pointer.as_ref() {
            for ray in pass.rays.iter().filter(|ray| ray.canvas_hit.is_some()) {
                on_panel[hand_slot(ray.handedness)] = true;
            }
        }
        let squeezing = [
            input_context.left_hand.squeeze_value > crate::ui::VR_TRIGGER_THRESHOLD,
            input_context.right_hand.squeeze_value > crate::ui::VR_TRIGGER_THRESHOLD,
        ];
        update_squeeze_swallow(
            &mut self.vr_squeeze_swallow,
            squeezing,
            hand_empty,
            [self.use_mode && on_panel[0], self.use_mode && on_panel[1]],
        );

        // VR weapon-safe: while the cyber interface is up (and until a
        // trigger held across its exit is released) the hands see zeroed
        // triggers, so the wielded weapon cannot fire - it stays wielded and
        // visible. The analog of the flat runtime's mouse-ownership swallow
        // latch. Squeezes are masked only per the latch above, so a squeeze on
        // an inventory slot pulls the item into that hand (the panel emits
        // `GrabEntity`) without also grabbing whatever the hand was aimed at.
        let safe_triggers = self.use_mode || self.vr_trigger_swallow;
        let weapon_safe_input = (game_options.presentation_mode == crate::PresentationMode::Vr
            && (safe_triggers || self.vr_squeeze_swallow.iter().any(|latched| *latched)))
        .then(|| {
            let mut safe = input_context.clone();
            if safe_triggers {
                safe.left_hand.trigger_value = 0.0;
                safe.right_hand.trigger_value = 0.0;
            }
            if self.vr_squeeze_swallow[hand_slot(crate::vr_config::Handedness::Left)] {
                safe.left_hand.squeeze_value = 0.0;
            }
            if self.vr_squeeze_swallow[hand_slot(crate::vr_config::Handedness::Right)] {
                safe.right_hand.squeeze_value = 0.0;
            }
            safe
        });
        let hands_input = weapon_safe_input.as_ref().unwrap_or(input_context);

        // VR drives two hands; flat drives a single first-person weapon
        // controller. Both feed the same effect-processing path.
        let interaction_msgs = self.interaction.update(&InteractionContext {
            physics: &self.physics,
            world: &self.world,
            input: hands_input,
            player_pos,
            player_rotation: player_rot,
            head_rotation: input_context.head.rotation,
            eye_height: crate::player_eye_height_for(self.player_handle.is_crouched()),
        });
        self.process_virtual_hand_effects(asset_cache, interaction_msgs);

        // Tag the wielded weapon with the flat camera/crosshair fire ray so its
        // firing scripts spawn projectiles along the crosshair (camera-origin
        // aim) rather than the offset barrel. Only the player's wielded weapon
        // gets this; AI/VR weapons are untouched.
        if let (Some((origin, forward)), Some(weapon)) = (
            self.interaction.flat_aim_ray(),
            self.interaction.viewmodel_entity(),
        ) {
            self.world
                .add_component(weapon, RuntimePropFlatAim { origin, forward });
        }

        // Advance the flat melee swing animation (returns to static idle on end).
        self.update_flat_melee_anim(time.elapsed);

        // Advance any in-progress reload (clears itself when complete).
        self.update_flat_reload_anim(time.elapsed);

        // Sync up the position of all the physics objects
        // The timing of this is important - things like the GUI rendering depend on an up-to-date position
        // from physics
        self.synchronize_physics_positions();

        // Re-anchor attached entities (e.g. a muzzle flash) to their parent's
        // now-current transform, so they track a moving parent rather than their
        // spawn pose. Runs after physics sync so parents' transforms are current.
        self.update_attached_entities();

        // Flat-mode MFD panel input: map the 2D pointer onto the active panel
        // and drive it through the same GUIHover contract the VR hand ray
        // uses. Dispatched before the script update so hovers/clicks are
        // processed this frame.
        let (ui_messages, ui_drag_actions) = match game_options.presentation_mode {
            crate::PresentationMode::Flat => {
                // Expose the AMMOFULL ammo-cycle button for hit-testing exactly
                // when the flat HUD draws it (the shared visibility predicate,
                // so the clickable rect never diverges from the rendered
                // button).
                let ammo_button =
                    if crate::hud::ammo_cycle_button_visible(&self.world, self.use_mode) {
                        Some(crate::hud::AMMO_CYCLE_BUTTON)
                    } else {
                        None
                    };
                self.flat_ui.set_ammo_cycle_button(ammo_button);
                self.flat_ui.update(&self.world, input_context.pointer)
            }
            // The cyber interface's pointer bridge: the controller ray's canvas
            // hit, the trigger as the click button and the squeeze as the grab,
            // fed to the same host the mouse drives. Everything after this - the
            // hover routing, the cursor-is-the-item drag, grab-to-hand - is the
            // one shared implementation.
            // Only while the interface is up: outside it the host has nothing
            // bound in VR, and running its walk-away / bare-view logic against
            // a pointer that does not exist would be a trap for whatever opens
            // a host slot in VR next.
            crate::PresentationMode::Vr if self.use_mode => {
                let pointer = self.vr_use_mode_pointer.as_ref().map(|pass| {
                    crate::mission::flat_ui_host::vr_canvas_pointer(pass, input_context)
                });
                self.flat_ui.update_canvas(&self.world, pointer)
            }
            crate::PresentationMode::Vr => (Vec::new(), Vec::new()),
        };
        for msg in ui_messages {
            self.script_world.dispatch(msg);
        }
        for action in ui_drag_actions {
            let drag_effects = self.apply_flat_drag_action(action);
            effects.extend(drag_effects);
        }

        // Update scripts
        let mut script_effects = profile!(
            scope: "game", level: DEBUG, "script_world.update",
            self.script_world.update(&self.world, &self.physics, time)
        );
        effects.append(&mut script_effects);

        // Any load-restored entity that did not overlap a tripwire still only
        // needs the marker for the first physics-backed script update. Paused
        // zero-time updates cannot emit initial overlaps, so they must retain
        // it. Clear after the first nonzero update so a genuine sensor entry on
        // a later frame is never suppressed.
        if !time.elapsed.is_zero() {
            self.world.run(|mut v_teleported: ViewMut<PropTeleported>| {
                let load_restores = (&v_teleported)
                    .iter()
                    .with_id()
                    .filter_map(|(id, marker)| {
                        (marker.source == TeleportSource::LoadRestore).then_some(id)
                    })
                    .collect::<Vec<_>>();

                for id in load_restores {
                    v_teleported.remove(id);
                }
            });
        }

        // Handle any pending entity triggers now that scripts are initialized
        if !self.pending_entity_triggers.is_empty() {
            println!(
                "Processing {} pending entity triggers after script initialization",
                self.pending_entity_triggers.len()
            );
            let pending_triggers = self.pending_entity_triggers.drain(..).collect::<Vec<_>>();
            for entity_name in pending_triggers {
                println!("Triggering delayed entity: {}", entity_name);
                let messages = self.trigger_entity_by_name_internal(entity_name);
                for message in messages {
                    println!("Dispatching message: {:?}", message);
                    self.script_world.dispatch(message);
                }
            }
        }

        self.world.run(run_tweq);
        self.world.run(run_bitmap_animation);

        self.gui.update();

        let mut current_effects = self.world.borrow::<UniqueViewMut<EffectQueue>>().unwrap();
        effects.append(&mut current_effects.flush());

        // Update particle systems
        let mut finished_particle_entities: Vec<EntityId> = Vec::new();
        self.world.run(
            |prop_particle_group: View<PropParticleGroup>,
             prop_particle_launch_info: View<PropParticleLaunchInfo>,
             v_transient_fx: View<crate::runtime_props::RuntimePropTransientFx>,
             transform: View<RuntimePropTransform>| {
                for (id, (pg, launch_info, transform)) in
                    (&prop_particle_group, &prop_particle_launch_info, &transform)
                        .iter()
                        .with_id()
                {
                    // Dormant groups (authored inactive) do not emit. There is
                    // no runtime activation toggle yet; when one exists this
                    // should flip the system on rather than skip it. A
                    // transient (fire-and-forget) entity that is dormant would
                    // never finish a burst, so reap it immediately instead of
                    // leaking an invisible entity.
                    if !pg.is_active {
                        if v_transient_fx.contains(id) {
                            finished_particle_entities.push(id);
                        }
                        continue;
                    }
                    let particle_system =
                        self.id_to_particle_system.entry(id).or_insert_with(|| {
                            let mut system = ParticleSystem::new()
                                .with_lifetime(launch_info.min_time, launch_info.max_time)
                                .with_velocity(
                                    launch_info.vel_min / SCALE_FACTOR,
                                    launch_info.vel_max / SCALE_FACTOR,
                                )
                                .with_acceleration(pg.gravity / SCALE_FACTOR)
                                .with_launch_bounding_box(
                                    launch_info.loc_min / SCALE_FACTOR,
                                    launch_info.loc_max / SCALE_FACTOR,
                                )
                                .with_particle_size(
                                    2.0 * pg.size / SCALE_FACTOR,
                                    2.0 * pg.size / SCALE_FACTOR,
                                )
                                .with_num_particles(pg.num as usize)
                                .with_launch_time(Duration::from_secs_f32(pg.launch_time))
                                .with_alpha(pg.a as f32 / 255.0)
                                .with_fade_time(pg.fade_time)
                                // pg.r is the primary color as an index into the
                                // game master palette (not a literal red channel);
                                // resolve it to RGB. cg/cb (pg.g/pg.b) drive the
                                // lifetime fade and are handled separately.
                                .with_color(crate::palette::index_to_rgb(pg.r))
                                // Animation type 0 = launch one shot: the burst
                                // fires once and the group dies with its last
                                // particle (impact spangs). Other types keep
                                // launching (steam vents etc.).
                                .with_one_shot(pg.animation_type == 0);
                            // Render type 5 (scaled bitmap) draws the authored
                            // sprite (res/bitmap/<name>.PCX) instead of the
                            // default glow disk; a missing bitmap falls back
                            // to the disk. The sprite carries its own colors,
                            // so the palette tint is left white.
                            const PRT_SCALED_BITMAP: u32 = 5;
                            if pg.render_type == PRT_SCALED_BITMAP && !pg.model_name.is_empty() {
                                let bitmap_name = format!("{}.PCX", pg.model_name);
                                if let Some(texture) = asset_cache.get_ext_opt(
                                    &TEXTURE_IMPORTER,
                                    &bitmap_name,
                                    &engine::texture::TextureOptions {
                                        wrap: false,
                                        // Dark sprites key transparency on
                                        // palette index 0 (the magenta color
                                        // key only covers some of them).
                                        transparent_index_0: true,
                                    },
                                ) {
                                    system = system
                                        .with_sprite_texture(texture)
                                        .with_color(cgmath::vec3(1.0, 1.0, 1.0));
                                } else {
                                    warn!("particle bitmap not found: {bitmap_name}");
                                }
                            }
                            system
                        });
                    particle_system.update(time.elapsed, transform.0);
                    // Only fire-and-forget effect entities (impact spangs) are
                    // destroyed when their burst expires; a level-authored
                    // one-shot group just goes dormant (the object persists,
                    // like the original engine).
                    if particle_system.is_done() && v_transient_fx.contains(id) {
                        finished_particle_entities.push(id);
                    }
                }
            },
        );
        // Transient one-shot effects (impact spangs) expire with their burst -
        // destroy the entity so spangs don't accumulate forever at every
        // bullet hole.
        for id in finished_particle_entities {
            effects.push(Effect::DestroyEntity { entity_id: id });
        }

        effects
    }

    ///
    /// synchronize_physics_positions
    ///
    /// Populate the PropPosition and RuntimePropTransform components,
    /// based on the current values in the physics engine
    fn synchronize_physics_positions(&mut self) {
        {
            let v_scale = self
                .world
                .borrow::<View<dark::properties::PropScale>>()
                .unwrap();
            let mut v_transform = self
                .world
                .borrow::<ViewMut<RuntimePropTransform>>()
                .unwrap();
            let mut v_prop_position = self.world.borrow::<ViewMut<PropPosition>>().unwrap();
            let v_entities = self.world.borrow::<EntitiesView>().unwrap();
            for (entity_id, handle) in &self.id_to_physics {
                let scale = v_scale
                    .get(*entity_id)
                    .map(|p| p.0)
                    .unwrap_or(vec3(1.0, 1.0, 1.0));
                let position = self.physics.get_position(*handle).unwrap();
                let rotation = self.physics.get_rotation(*handle).unwrap();
                let scale_xform =
                    Matrix4::from_nonuniform_scale(scale.x.abs(), scale.y.abs(), scale.z.abs());
                let translation_xform = Matrix4::from_translation(position);
                let rotation_xform = Matrix4::from(rotation);
                let xform = translation_xform * rotation_xform * scale_xform;
                v_entities.add_component(
                    *entity_id,
                    &mut v_prop_position,
                    PropPosition {
                        position,
                        rotation,
                        cell: 0,
                    },
                );
                v_entities.add_component(*entity_id, &mut v_transform, RuntimePropTransform(xform));
            }
        };
    }

    ///
    /// update_attached_entities
    ///
    /// For every entity bolted to a parent (RuntimePropAttachment), recompute its
    /// RuntimePropTransform as `parent.transform * local_transform`, so it tracks
    /// the parent each frame (e.g. a muzzle flash following the first-person
    /// weapon). Entities whose parent has gone away keep their last transform.
    fn update_attached_entities(&mut self) {
        self.world.run(run_attachment_update);
    }

    /// Resolve tag-based motion queries for `entity_id` (tried in order,
    /// first match wins) and hand the clip to `apply`
    /// (`AnimationPlayer::queue_animation` to push on the queue,
    /// `AnimationPlayer::play_animation` to interrupt and replace it). When
    /// no query matches, dispatches `AnimationCompleted` so the requesting
    /// script isn't left waiting on a clip that never started.
    fn apply_animation_by_schema(
        &mut self,
        global_context: &GlobalContext,
        asset_cache: &mut AssetCache,
        entity_id: EntityId,
        motion_queries: Vec<Vec<MotionQueryItem>>,
        selection_strategy: MotionQuerySelectionStrategy,
        apply: fn(&AnimationPlayer, Rc<AnimationClip>) -> AnimationPlayer,
    ) {
        let is_death_query = motion_queries
            .iter()
            .flatten()
            .any(|item| item.tag_name() == "crumple");
        let mut resolved_death_pose = None;
        let maybe_player = self.id_to_animation_player.get_mut(&entity_id);
        if let Some(player) = maybe_player {
            let v_creature_type = self.world.borrow::<View<PropCreature>>().unwrap();

            let v_motion_actor_tag = self.world.borrow::<View<PropMotionActorTags>>().unwrap();

            if let (Ok(creature_type), Ok(motion_actor_tag)) = (
                v_creature_type.get(entity_id),
                v_motion_actor_tag.get(entity_id),
            ) {
                let actor_tags = motion_actor_tag
                    .tags
                    .iter()
                    .map(|tag| MotionQueryItem::new(tag).optional())
                    .collect::<Vec<MotionQueryItem>>();

                let creature_definition = get_creature_definition(creature_type.0).unwrap();

                let actor_type = creature_definition.actor_type.to_u32().unwrap();

                let mut tried_queries = Vec::new();
                let maybe_next_animation = motion_queries.into_iter().find_map(|items| {
                    let mut query_items = items;
                    query_items.extend(actor_tags.iter().cloned());
                    let query = MotionQuery::new(actor_type, query_items)
                        .with_selection_strategy(selection_strategy.clone());
                    let result = if is_death_query {
                        let options = global_context
                            .motiondb
                            .query_all(query.clone())
                            .into_iter()
                            .filter(|name| {
                                is_realtime_crumple(
                                    global_context
                                        .motiondb
                                        .get_mps_motions(name.clone())
                                        .frame_count,
                                )
                            })
                            .collect::<Vec<_>>();
                        select_motion_option(&options, &selection_strategy)
                    } else {
                        global_context.motiondb.query(query.clone())
                    };
                    tried_queries.push(query);
                    result
                });

                // Animation failures are otherwise invisible (the scoped
                // game_log is off by default) and have repeatedly hidden
                // broken sequences - keep a tracing breadcrumb of every
                // resolution.
                tracing::debug!(
                    "animation queries for {:?}: {:?} -> {:?}",
                    entity_id,
                    tried_queries.iter().map(|q| &q.items).collect::<Vec<_>>(),
                    maybe_next_animation
                );
                if let Some(next_animation) = maybe_next_animation {
                    let maybe_clip = asset_cache
                        .get_opt(&ANIMATION_CLIP_IMPORTER, &format!("{}_.mc", next_animation));

                    if let Some(clip) = maybe_clip {
                        // The live pose is already registered against the
                        // walk surface. Preserve its lowest-joint depth as the
                        // creature-specific floor convention: CAL bind depth
                        // is measurably higher for real human/hybrid assets.
                        let death_floor_depth = is_death_query
                            .then(|| {
                                self.id_to_model
                                    .get(&entity_id)
                                    .and_then(Model::skeleton)
                                    .and_then(|skeleton| {
                                        dark::ss2_skeleton::lowest_joint_y(
                                            skeleton,
                                            &player.get_transforms(skeleton),
                                        )
                                    })
                            })
                            .flatten();
                        let clip = match (
                            is_death_query,
                            self.id_to_model.get(&entity_id).and_then(Model::skeleton),
                            death_floor_depth,
                        ) {
                            (true, Some(skeleton), Some(floor_depth)) => {
                                Rc::new(dark::ss2_skeleton::ground_terminal_pose_to_floor(
                                    skeleton,
                                    &clip,
                                    floor_depth,
                                ))
                            }
                            _ => clip,
                        };
                        self.failed_animation_queries.remove(&entity_id);
                        *player = apply(player, clip);

                        // The motion query is random, so its resolved clip name
                        // is the only durable identity of the corpse pose.
                        // EntitySaveData explicitly persists this generated
                        // runtime fact separately from authored P$CretPose.
                        let is_killed = self
                            .world
                            .borrow::<View<PropHitPoints>>()
                            .ok()
                            .and_then(|view| view.get(entity_id).ok().map(|hp| hp.hit_points <= 0))
                            .unwrap_or(false);
                        if is_death_query && is_killed {
                            resolved_death_pose =
                                Some(RuntimePropDeathPose::new(next_animation, death_floor_depth));
                        }
                    } else {
                        // Report completion just like the query-miss branch
                        // below, so anything waiting on this animation
                        // (scripted Play actions) is never left hanging -
                        // but rate-limited per distinct failure, or the
                        // completion handler's re-query loops every frame.
                        let failure = format!("clip:{next_animation}");
                        if Self::report_animation_failure(
                            &mut self.failed_animation_queries,
                            &mut self.script_world,
                            entity_id,
                            failure,
                        ) {
                            game_log!(
                                WARN,
                                "Unable to load animation clip: {:?}_.mc",
                                next_animation
                            );
                        }
                    }
                } else {
                    // Key on the query items only - the selection strategy
                    // carries a per-request counter that would defeat the
                    // dedupe (every retry would look like a new query).
                    let failure = format!(
                        "query:{:?}",
                        tried_queries.iter().map(|q| &q.items).collect::<Vec<_>>()
                    );
                    if Self::report_animation_failure(
                        &mut self.failed_animation_queries,
                        &mut self.script_world,
                        entity_id,
                        failure,
                    ) {
                        game_log!(
                            WARN,
                            "Unable to find animation for queries: {:?}",
                            &tried_queries
                        );
                    }
                }
            }
        }

        if let Some(death_pose) = resolved_death_pose {
            self.world.add_component(entity_id, death_pose);
        }
    }

    /// Report a failed animation resolution: dispatches AnimationCompleted
    /// (so the requester isn't left hanging) unless the SAME failure for
    /// this entity is still inside its suppression window. Returns whether
    /// the failure was reported (callers log only then).
    fn report_animation_failure(
        failed_animation_queries: &mut HashMap<EntityId, FailedAnimationGuard>,
        script_world: &mut ScriptWorld,
        entity_id: EntityId,
        failure: String,
    ) -> bool {
        // ~0.5s at the fixed 60Hz step: long enough to collapse the
        // per-frame re-query loop, short enough that a deferred completion
        // (below) arrives well inside a scripted action's timeout.
        const SUPPRESS_FRAMES: u32 = 30;
        if let Some(guard) = failed_animation_queries.get_mut(&entity_id) {
            if guard.key == failure && guard.frames_left > 0 {
                // Owe the requester its completion at window expiry rather
                // than dropping it - a repeat can be a genuine new request
                // (a scripted sequence replaying the same failing action).
                guard.completion_owed = true;
                return false;
            }
        }
        failed_animation_queries.insert(
            entity_id,
            FailedAnimationGuard {
                key: failure,
                frames_left: SUPPRESS_FRAMES,
                completion_owed: false,
            },
        );
        script_world.dispatch(Message {
            payload: MessagePayload::AnimationCompleted,
            to: entity_id,
        });
        true
    }

    fn update_animations(&mut self, time: &Time) {
        // Tick down the failed-query suppression windows; on expiry, pay any
        // completion owed to a request suppressed inside the window (see
        // report_animation_failure).
        let mut owed_completions = Vec::new();
        self.failed_animation_queries.retain(|entity_id, guard| {
            guard.frames_left = guard.frames_left.saturating_sub(1);
            if guard.frames_left == 0 {
                if guard.completion_owed {
                    owed_completions.push(*entity_id);
                }
                false
            } else {
                true
            }
        });
        for entity_id in owed_completions {
            self.script_world.dispatch(Message {
                payload: MessagePayload::AnimationCompleted,
                to: entity_id,
            });
        }

        for (id, player) in self.id_to_animation_player.iter_mut() {
            // self.id_to_animation_player.entry(*id).and_modify(|player| {
            //     *player = AnimationPlayer::update(player, time.elapsed);
            // });
            let (new_player, flags, events, velocity) =
                AnimationPlayer::update(player, time.elapsed);
            *player = new_player;

            if let Some(model) = self.id_to_model.get(id) {
                let joint_transforms = model.get_joint_transforms(player);
                self.world
                    .add_component(*id, RuntimePropJointTransforms(joint_transforms));
            }

            let v_transform = self.world.borrow::<View<RuntimePropTransform>>().unwrap();
            let maybe_transform = v_transform.get(*id);
            let curr_velocity = self
                .physics
                .get_velocity(*id)
                .unwrap_or(vec3(0.0, 0.0, 0.0));
            if let Ok(transform) = maybe_transform {
                // AI steering publishes a locomotion scale (heading-error /
                // arrival coupling); scale the horizontal root velocity so a
                // turning AI slows instead of arcing at full stride. Vertical
                // velocity (gravity) is untouched. Consume-on-read: the
                // component is removed after applying so a stale value can't
                // slow non-steering animations (death, attack clips) - the
                // steering republishes it every frame it runs.
                let scale = self
                    .world
                    .run(
                        |mut v_scale: ViewMut<crate::runtime_props::RuntimePropLocomotionScale>| {
                            v_scale.remove(*id).map(|s| s.0)
                        },
                    )
                    .unwrap_or(1.0);
                let adj_velocity =
                    transform
                        .0
                        .transform_vector(vec3(velocity.z, curr_velocity.y, -velocity.x));
                let mut scaled = vec3(
                    adj_velocity.x * scale,
                    adj_velocity.y,
                    adj_velocity.z * scale,
                );
                scaled += self.physics.take_player_push_velocity(*id);
                // A restored terminal death player has no queued motion, but
                // even writing zero velocity wakes its deliberately sleeping
                // dynamic corpse body. Leave physics ownership untouched:
                // it stays stable at rest and remains wakeable by contact or
                // an impulse.
                let holds_terminal_death_pose = player.is_queue_empty()
                    && self
                        .world
                        .borrow::<View<RuntimePropDeathPose>>()
                        .unwrap()
                        .contains(*id);
                let launched_with_no_root_motion = velocity.magnitude2() <= f32::EPSILON
                    && self
                        .world
                        .borrow::<View<RuntimePropLaunchedProjectile>>()
                        .unwrap()
                        .contains(*id);
                if !holds_terminal_death_pose && !launched_with_no_root_motion {
                    self.physics.set_velocity(*id, scaled);
                }
            }

            if !flags.is_empty() {
                self.script_world.dispatch(Message {
                    to: *id,
                    payload: MessagePayload::AnimationFlagTriggered {
                        motion_flags: flags,
                    },
                })
            }

            for event in events {
                match event {
                    AnimationEvent::Completed => self.script_world.dispatch(Message {
                        to: *id,
                        payload: MessagePayload::AnimationCompleted,
                    }),
                    AnimationEvent::DirectionChanged(ang) => {
                        game_log!(DEBUG, "Animation direction changed: {:?}", ang);
                        // The events now arrive as small per-tick increments
                        // ramping across the clip (they used to snap once at
                        // completion); the -0.5 scale predates recorded
                        // history and is preserved so clips leave entities
                        // facing exactly where they always have.
                        let maybe_current_rotation = self.physics.get_rotation2(*id);
                        if let Some(current_rotation) = maybe_current_rotation {
                            let new_rotation =
                                current_rotation * Quaternion::from_angle_y(ang * -0.5);
                            self.physics.set_rotation2(*id, new_rotation);
                        }
                    }
                    AnimationEvent::VelocityChanged(_velocity) => (),
                }
            }
        }
    }

    pub fn slay_entity(&mut self, entity_id: EntityId, asset_cache: &mut AssetCache) -> bool {
        let world = &self.world;
        let flinderize_links = get_all_links_with_template(world, entity_id, |link| match link {
            Link::Flinderize(opts) => Some(*opts),
            _ => None,
        });

        let corpse_links = get_all_links_with_template(world, entity_id, |link| match link {
            Link::Corpse(opts) => Some(*opts),
            _ => None,
        });

        let did_slay = true;

        if let Some(handle) = &self.id_to_physics.get(&entity_id) {
            let position = self.physics.get_position(**handle).unwrap();
            let rotation = self.physics.get_rotation(**handle).unwrap();
            let aabb = self.physics.get_aabb2(entity_id);
            let mut rng = thread_rng();

            for (template_id, flinderize_options) in flinderize_links {
                // Real gamesys data carries count >= 1 (e.g. Breakable Windows:
                // 4 links, count 1 each); max(1) guards links with zeroed data
                // so they still yield one flinder, like the pre-options behavior.
                for _ in 0..flinderize_options.count.max(1) {
                    // scatter spawns the flinder at a random point within the
                    // object's bounds (world AABB - over-covers rotated objects,
                    // close enough for gibs); otherwise at the object-relative
                    // offset.
                    let spawn_position = match (flinderize_options.scatter, &aabb) {
                        (true, Some(aabb)) => Point3::new(
                            rng.gen_range(aabb.min.x..=aabb.max.x),
                            rng.gen_range(aabb.min.y..=aabb.max.y),
                            rng.gen_range(aabb.min.z..=aabb.max.z),
                        ),
                        _ => vec3_to_point3(position + rotation * flinderize_options.offset),
                    };

                    let spawn_rotation = Quaternion::from_angle_y(cgmath::Rad(
                        rng.gen_range(0.0..std::f32::consts::TAU),
                    )) * Quaternion::from_angle_x(cgmath::Rad(
                        rng.gen_range(0.0..std::f32::consts::TAU),
                    )) * Quaternion::from_angle_z(cgmath::Rad(
                        rng.gen_range(0.0..std::f32::consts::TAU),
                    ));

                    let created = self.create_entity_with_position(
                        asset_cache,
                        template_id,
                        spawn_position,
                        spawn_rotation,
                        Matrix4::identity(),
                        CreateEntityOptions {
                            flinderize_debris: true,
                            ..CreateEntityOptions::default()
                        },
                    );

                    if created.rigid_body.is_some() {
                        let speed = flinderize_options.impulse / SCALE_FACTOR;
                        self.physics
                            .set_velocity(created.entity_id, random_unit_vector(&mut rng) * speed);
                    }
                }
            }

            for (template_id, _corpse_options) in corpse_links {
                self.create_entity_with_position(
                    asset_cache,
                    template_id,
                    vec3_to_point3(position),
                    rotation,
                    Matrix4::identity(),
                    CreateEntityOptions::default(),
                );
            }
        }

        did_slay
    }

    /// Apply a radius stim blast (Effect::RadiusBlast): every entity with hit
    /// points in range receives the stim at linear-falloff intensity, and its
    /// receptrons decide the damage (no receptron for the stim = no response -
    /// the type-effectiveness mechanism: EMP does nothing to organics).
    /// Dynamic bodies in range are shoved outward regardless.
    fn radius_blast(
        &mut self,
        center: Vector3<f32>,
        radius: f32,
        intensity: f32,
        stim_template_id: i32,
    ) {
        let mut in_range = Vec::new();
        {
            let v_hit_points = self
                .world
                .borrow::<View<dark::properties::PropHitPoints>>()
                .unwrap();
            let v_transform = self.world.borrow::<View<RuntimePropTransform>>().unwrap();
            for (entity_id, (_hit_points, transform)) in
                (&v_hit_points, &v_transform).iter().with_id()
            {
                let position = transform.0.transform_point(cgmath::point3(0.0, 0.0, 0.0));
                let distance = (crate::util::point3_to_vec3(position) - center).magnitude();
                if distance < radius {
                    let falloff = 1.0 - distance / radius;
                    in_range.push((entity_id, intensity * falloff));
                }
            }
        }

        // The player's pawn is physics-driven: its position lives in
        // PlayerInfo and it never gets a RuntimePropTransform, so the sweep
        // above cannot see it - without this, no blast has ever reached the
        // player. Its receptrons (from `The Player` archetype) resolve the
        // stim exactly like any other victim's.
        {
            let player = self.world.borrow::<UniqueView<PlayerInfo>>().unwrap();
            let distance = (player.pos - center).magnitude();
            if distance < radius && !in_range.iter().any(|(id, _)| *id == player.entity_id) {
                in_range.push((player.entity_id, intensity * (1.0 - distance / radius)));
            }
        }

        for (entity_id, felt_intensity) in in_range {
            let receptrons =
                get_all_links_with_template(&self.world, entity_id, |link| match link {
                    Link::Receptron(options) => Some(options.clone()),
                    _ => None,
                });
            let maybe_damage = crate::mission::stim_response::resolve_stim_damage(
                &receptrons,
                stim_template_id,
                felt_intensity,
            );
            // Explosions only ever deal damage: dispatch a Damage message only
            // for a positive result. A zero amount (fully shielded) would still
            // read as "took damage" and aggro AI; a negative one (a heal
            // receptron) has no meaning through the damage path.
            if let Some(amount) = maybe_damage {
                if amount > 0.0 {
                    self.script_world.dispatch(Message {
                        to: entity_id,
                        // No impact vector: the blast's physical push is
                        // applied radially to bodies by radius_blast itself.
                        payload: MessagePayload::Damage {
                            amount,
                            impact: None,
                        },
                    });
                }
            }
        }

        self.physics.apply_radial_impulse(
            center,
            radius,
            intensity * BLAST_PUSH_SPEED_PER_INTENSITY,
        );
    }

    /// Propagate a noise (Effect::RaiseNoise): every creature within `radius`
    /// of `origin` hears it and gets a HeardNoise message, so it can alert and
    /// investigate the source. A plain Euclidean radius - walls don't
    /// attenuate it yet (a path-distance model is a follow-up). Deaf AIs
    /// (hearing acuity 0, e.g. the `Deaf` metaproperty on medsci1's
    /// card-slot-watching OG-Pipe, obj 596 - the corridor hybrids hear
    /// normally) are filtered out here so no listener has to re-check.
    fn raise_noise(&mut self, origin: Vector3<f32>, radius: f32) {
        let heard: Vec<EntityId> = {
            let v_creature = self
                .world
                .borrow::<View<dark::properties::PropCreature>>()
                .unwrap();
            let v_transform = self.world.borrow::<View<RuntimePropTransform>>().unwrap();
            let v_hearing = self
                .world
                .borrow::<View<dark::properties::PropAIHearing>>()
                .unwrap();
            (&v_creature, &v_transform)
                .iter()
                .with_id()
                .filter_map(|(entity_id, (_creature, transform))| {
                    if v_hearing.get(entity_id).is_ok_and(|h| h.is_deaf()) {
                        return None;
                    }
                    let pos = transform.0.transform_point(cgmath::point3(0.0, 0.0, 0.0));
                    let distance = (crate::util::point3_to_vec3(pos) - origin).magnitude();
                    (distance < radius).then_some(entity_id)
                })
                .collect()
        };
        for entity_id in heard {
            self.script_world.dispatch(Message {
                to: entity_id,
                payload: MessagePayload::HeardNoise { origin },
            });
        }
    }

    pub fn create_entity_by_template_name(
        &mut self,
        asset_cache: &mut AssetCache,
        template_name: &str,
        position: Point3<f32>,
        orientation: Quaternion<f32>,
    ) -> Option<EntityCreationInfo> {
        self.create_entity_by_template_name_with_options(
            asset_cache,
            template_name,
            position,
            orientation,
            CreateEntityOptions::default(),
        )
    }

    fn create_entity_by_template_name_with_options(
        &mut self,
        asset_cache: &mut AssetCache,
        template_name: &str,
        position: Point3<f32>,
        orientation: Quaternion<f32>,
        options: CreateEntityOptions,
    ) -> Option<EntityCreationInfo> {
        let template_name_lowercase = template_name.to_ascii_lowercase();
        let maybe_template_id = self
            .template_name_to_template_id
            .get(&template_name_lowercase)
            .cloned();

        if let Some(template_id) = maybe_template_id {
            Some(self.create_entity_with_position(
                asset_cache,
                template_id.template_id,
                position,
                orientation,
                Matrix4::identity(),
                options,
            ))
        } else {
            None
        }
    }

    fn spawn_ecology_entity(
        &mut self,
        asset_cache: &mut AssetCache,
        template_name: &str,
        spawn_point: EntityId,
        ecology_type: Option<i32>,
        goto_player: bool,
    ) {
        let (position, orientation, patrol) = {
            let positions = self.world.borrow::<View<PropPosition>>().unwrap();
            let Ok(position) = positions.get(spawn_point) else {
                warn!("TrapSpawn marker {:?} has no position", spawn_point);
                return;
            };
            let patrol = self
                .world
                .borrow::<View<dark::properties::PropAIPatrol>>()
                .ok()
                .and_then(|patrols| patrols.get(spawn_point).ok().map(|patrol| patrol.0))
                .unwrap_or(false);
            (position.position, position.rotation, patrol)
        };
        // Spawn markers name their archetype by object name, and that name
        // often belongs to a concrete object placed in the mission and parked
        // off-map (earth.mis's "DopeyDroid", the training-droid ecologies)
        // rather than to a gamesys template - so fall back to this mission's
        // own named objects. Gamesys archetypes still win, leaving every other
        // by-name creation path untouched.
        let name_lowercase = template_name.to_ascii_lowercase();
        let archetype = self
            .template_name_to_template_id
            .get(&name_lowercase)
            .map(|metadata| metadata.template_id)
            .or_else(|| self.mission_object_name_to_id.get(&name_lowercase).copied());
        let Some(archetype) = archetype else {
            warn!("TrapSpawn could not resolve archetype {template_name}");
            return;
        };
        if archetype >= 0 {
            // Cloning a concrete object also clones its authored links, which
            // resolve to the *original's* live partners - log which object was
            // picked so a bad resolution is diagnosable.
            info!("TrapSpawn resolved archetype {template_name} to mission object {archetype}");
        }
        let created = self.create_entity_with_position(
            asset_cache,
            archetype,
            Point3::new(position.x, position.y, position.z),
            orientation,
            Matrix4::identity(),
            CreateEntityOptions::default(),
        );

        if let Some(ecology_type) = ecology_type {
            self.world.add_component(
                created.entity_id,
                dark::properties::PropEcoType(ecology_type),
            );
        }
        if patrol {
            self.world
                .add_component(created.entity_id, dark::properties::PropAIPatrol(true));
        }

        let created_template_id = self
            .world
            .borrow::<View<dark::properties::PropTemplateId>>()
            .ok()
            .and_then(|templates| {
                templates
                    .get(created.entity_id)
                    .ok()
                    .map(|template| template.template_id)
            })
            .unwrap_or_default();
        let mut links = self.world.borrow::<ViewMut<Links>>().unwrap();
        if let Ok(marker_links) = (&mut links).get(spawn_point) {
            marker_links.to_links.push(ToLink {
                to_template_id: created_template_id,
                to_entity_id: Some(WrappedEntityId(created.entity_id)),
                link: Link::Spawned,
            });
        }
        drop(links);

        // The original TrapSpawn creates the authored materialization effect at
        // the marker alongside the child.
        self.create_entity_by_template_name(
            asset_cache,
            "SpawnSFX",
            Point3::new(position.x, position.y, position.z),
            orientation,
        );

        if goto_player {
            // Dark's GotoLoc asks the fresh AI to run directly to the player,
            // even without ordinary sight awareness. Pinned combat awareness
            // is the runtime's equivalent: it tracks the live player and
            // naturally transitions from pursuit to attack at range.
            self.script_world.dispatch(Message {
                to: created.entity_id,
                payload: MessagePayload::SetAlertness {
                    level: dark::properties::AIAlertLevel::High,
                    pin: true,
                },
            });
        }
    }

    pub fn make_un_physical(&mut self, entity_id: EntityId) {
        let current_entity = self.id_to_physics.get(&entity_id);
        if current_entity.is_none() {
            return;
        }

        self.physics.remove(entity_id);
        self.id_to_physics.remove(&entity_id);
    }

    /// Move `dropped_entity_id` into `container_entity_id` (e.g. the player's
    /// inventory): drop any prior `Contains` links to it, add a fresh one from
    /// the container, mark it referenced, and remove it from the physical world.
    /// Returns false if the container entity has no `Links` (so nothing was
    /// added). Shared by the `DropEntityInfo` effect and the debug give lever.
    pub fn drop_entity_into_container(
        &mut self,
        container_entity_id: EntityId,
        dropped_entity_id: EntityId,
    ) -> bool {
        let moved = move_live_entity_into_container(
            &mut self.world,
            container_entity_id,
            dropped_entity_id,
        );
        if moved {
            self.make_un_physical(dropped_entity_id);
        }
        moved
    }

    /// Re-encode the backpack's stored cells after effective Strength changes.
    /// Items that cannot fit even after deterministic reflow are spilled into
    /// the world, matching retail's `ShockInvResize` -> `ShockInvAddObj` path.
    fn resize_player_backpack(&mut self, old_width: usize, new_width: usize) {
        if old_width == new_width {
            return;
        }
        let inventory_entity = match self.world.borrow::<UniqueView<PlayerInfo>>() {
            Ok(player) => player.inventory_entity_id,
            Err(_) => return,
        };
        let outcome = crate::inventory::remap_container_width(
            &mut self.world,
            inventory_entity,
            (old_width, crate::inventory::BACKPACK_GRID.1),
            (new_width, crate::inventory::BACKPACK_GRID.1),
        );
        for (index, entity_id) in outcome.overflow.into_iter().enumerate() {
            self.spill_backpack_overflow(entity_id, index);
        }
    }

    /// Give one full-pack resize overflow item ordinary world presence beside
    /// the player. Physicalize before detaching, as the manual throw path does:
    /// a modelless item remains contained instead of being silently lost.
    fn spill_backpack_overflow(&mut self, entity_id: EntityId, index: usize) {
        let player = match self.world.borrow::<UniqueView<PlayerInfo>>() {
            Ok(player) => player.clone(),
            Err(_) => return,
        };
        self.make_physical(entity_id);
        if !self.id_to_physics.contains_key(&entity_id) {
            warn!(
                "Backpack resize could not spill modelless overflow entity {:?}; preserving its containment link",
                entity_id
            );
            return;
        }

        self.detach_from_containers(entity_id);
        let forward = player.rotation.rotate_vector(vec3(0.0, 0.0, -1.0));
        let right = player.rotation.rotate_vector(vec3(1.0, 0.0, 0.0));
        let lane = (index % 5) as f32 - 2.0;
        let row = (index / 5) as f32;
        let position = player.pos
            + forward * (crate::physics::PLAYER_STANDING_RADIUS_WORLD + 0.6 + row * 0.2)
            + right * lane * 0.25
            + vec3(0.0, 0.35 + row * 0.1, 0.0);
        self.set_entity_position_rotation(
            entity_id,
            position,
            Quaternion::new(1.0, 0.0, 0.0, 0.0),
            vec3(1.0, 1.0, 1.0),
        );
        self.physics.set_velocity(entity_id, forward * 1.5);
        self.world.add_component(entity_id, PropHasRefs(true));
        self.script_world.dispatch(Message {
            payload: MessagePayload::Drop,
            to: entity_id,
        });
    }

    /// When an entity is replaced (e.g. a dead power cell recharged into a live
    /// one), any container that held the old entity via a `Contains` link should
    /// keep holding the new one - otherwise the replacement is orphaned in the
    /// world and the container is left with a dangling link. Rewrites those
    /// incoming `Contains` links in place (preserving slot ordinal) and makes the
    /// new entity a proper contained item (un-physical, no refs). Returns whether
    /// the new entity was placed into a container. VR held items are tracked by
    /// the hand, not a `Contains` link, so this leaves the VR hold path untouched.
    fn transfer_containment(&mut self, old_entity_id: EntityId, new_entity_id: EntityId) -> bool {
        let mut transferred = false;
        {
            let mut v_links = self.world.borrow::<ViewMut<Links>>().unwrap();
            for (_id, links) in (&mut v_links).iter().with_id() {
                for link in links.to_links.iter_mut() {
                    if matches!(link.link, Link::Contains(_))
                        && link.to_entity_id.map(|e| e.0) == Some(old_entity_id)
                    {
                        link.to_entity_id = Some(WrappedEntityId(new_entity_id));
                        transferred = true;
                    }
                }
            }
        }
        if transferred {
            self.world.add_component(new_entity_id, PropHasRefs(false));
            self.make_un_physical(new_entity_id);
        }
        transferred
    }

    /// Drop any container's `Contains` link that points at `entity_id`. Called
    /// when an entity is destroyed so a consumed/used inventory item (e.g. a
    /// power cell inserted into an aux-power receptor) leaves no dangling link
    /// behind - a stale link would otherwise resolve to whatever entity later
    /// recycles the slot. Only invoked for contained items (see the destroy
    /// path), so the full scan is not run for world-present FX/projectiles.
    fn remove_incoming_contains_links(&mut self, entity_id: EntityId) {
        let mut v_links = self.world.borrow::<ViewMut<Links>>().unwrap();
        for (_id, links) in (&mut v_links).iter().with_id() {
            drop_contains_links_to(links, entity_id);
        }
    }

    /// Consume an audio-log pickup after copying its identity into the PDA.
    /// The ECS entity remains as inert mission state, while playback binds the
    /// shared MediaGui to the nonserialized player-owned reader host.
    ///
    /// `PropHasRefs(false)` persists through mission save/load, removing the
    /// model from rendering on both the current and reconstructed mission. The
    /// physics body and any container link must also go immediately so neither
    /// a world ray nor a corpse/locker loot panel can frob the disc again.
    fn consume_log_pickup(&mut self, entity_id: EntityId) {
        self.remove_incoming_contains_links(entity_id);
        self.world.add_component(entity_id, PropHasRefs(false));
        self.make_un_physical(entity_id);
    }

    /// Resolve an audio log's reader strings from `level<deck>.str` and cache
    /// them on the supplied `MediaGui` host. Returns true only when a real
    /// transcript was found, so callers never mark a log read or play audio
    /// after opening an empty backdrop.
    ///
    /// `RuntimePropLogData` is runtime-only and deliberately not serialized, so
    /// this runs each time the reader is opened - otherwise a log collected
    /// before a save would open a blank backdrop after loading.
    fn attach_log_reader_strings(
        &mut self,
        entity_id: EntityId,
        deck: u32,
        log: u32,
        asset_cache: &mut AssetCache,
    ) -> bool {
        let level_file = format!("level{deck:02}.str");
        let Some(strings) = asset_cache.get_opt(&dark::importers::STRINGS_IMPORTER, &level_file)
        else {
            return false;
        };
        // The .str values carry literal backslash-n escapes ("AMANPOUR
        // 07.JUL.14\nre: New code\n"); unescape them into real line breaks so
        // the reader never draws "\n".
        let get = |prefix: &str| {
            strings
                .get(&format!("{prefix}{log}"))
                .map(|s| s.replace("\\n", "\n"))
        };
        let data = crate::runtime_props::RuntimePropLogData {
            name: get("logname"),
            text: get("logtext"),
            portrait: resolve_optional_log_art_texture(
                asset_cache,
                deck,
                log,
                "portrait",
                get("logportrait"),
            ),
            icon: resolve_optional_log_art_texture(
                asset_cache,
                deck,
                log,
                "deck icon",
                get("logicon"),
            ),
        };
        if data.text.as_deref().is_none_or(str::is_empty) {
            return false;
        }
        self.world.add_component(entity_id, data);
        true
    }

    pub fn make_physical(&mut self, entity_id: EntityId) {
        let current_entity = self.id_to_physics.get(&entity_id);
        if current_entity.is_some() {
            return;
        }

        let maybe_model = self.id_to_model.get(&entity_id);

        let maybe_phys_obj = entity_creator::create_physics_representation(
            &mut self.world,
            &mut self.physics,
            &maybe_model,
            entity_id,
        );

        if let Some(phys_obj) = maybe_phys_obj {
            self.id_to_physics.insert(entity_id, phys_obj);
        }
    }

    /// Apply a cursor-is-the-item drag action from the `FlatUiHost` (§1.5/§2.4).
    /// Lift/place/swap never reach here: a lifted item stays in the backpack
    /// container (the host just hides it from the strip), so only committing
    /// the drag touches the world. `Throw` detaches + launches; `Wield`
    /// produces the same effect a backpack click does (returned for the normal
    /// effect pipeline, which has `asset_cache` for the grab).
    fn apply_flat_drag_action(
        &mut self,
        action: crate::mission::flat_ui_host::FlatUiDragAction,
    ) -> Vec<Effect> {
        use crate::mission::flat_ui_host::FlatUiDragAction;
        match action {
            FlatUiDragAction::Throw(entity_id) => {
                self.throw_entity_into_world(entity_id);
                Vec::new()
            }
            // The AMMOFULL cycle button: advance the wielded weapon's ammo type
            // via the same effect as the CycleAmmo key/action.
            FlatUiDragAction::CycleAmmo => vec![Effect::CycleAmmo],
            // Double-click = equip/use, acting on the still-contained item -
            // identical to the ContainerGui backpack click (shared weapon test,
            // shared effects): a weapon (gun or melee) wields via `GrabEntity`
            // (which also clears its Contains link and holsters any displaced
            // weapon back to the grid), anything else gets a `Frob` (use).
            FlatUiDragAction::Wield(entity_id) => {
                if crate::virtual_hand::is_wieldable_weapon(&self.world, entity_id) {
                    vec![Effect::GrabEntity {
                        entity_id,
                        hand: crate::vr_config::Handedness::Right,
                        current_parent_id: None,
                    }]
                } else {
                    vec![Effect::Send {
                        msg: Message {
                            payload: MessagePayload::Frob,
                            to: entity_id,
                        },
                    }]
                }
            }
        }
    }

    /// Remove every incoming `Contains` link to `entity_id` (take it out of
    /// whatever container holds it) without giving it world presence.
    fn detach_from_containers(&mut self, entity_id: EntityId) {
        let mut v_links = self.world.borrow::<ViewMut<Links>>().unwrap();
        for links in (&mut v_links).iter() {
            links.to_links.retain(|link| {
                !(matches!(link.link, Link::Contains(_))
                    && link.to_entity_id.map(|w| w.0) == Some(entity_id))
            });
        }
    }

    /// Throw a cursor-held item into the world: give it physics presence just
    /// ahead of the camera and shove it along the flat view ray (the original's
    /// `ShockInterfaceClick` -> `ThrowObj`, §2.4). The item is still in the
    /// backpack when this runs; it is only detached once a physics body is
    /// confirmed, so an aborted throw (no view ray, or a modelless item with no
    /// physics representation) leaves it in the backpack rather than lost or
    /// frozen mid-air.
    fn throw_entity_into_world(&mut self, entity_id: EntityId) {
        /// How far ahead of the camera the item materializes (world units).
        /// The eye sits on the standing capsule's axis, so this must clear the
        /// capsule's radius or a downward throw spawns the item inside the
        /// player's own collider and gets ejected.
        const THROW_SPAWN_DISTANCE: f32 = crate::physics::PLAYER_STANDING_RADIUS_WORLD + 0.2;
        /// Launch speed along the view ray (world units/sec).
        const THROW_SPEED: f32 = 6.0;

        // Origin + direction: the flat aim ray (camera + view forward). Only
        // set in flat presentation; without it, leave the item in the backpack.
        let Some((origin, forward)) = self.interaction.flat_aim_ray() else {
            return;
        };

        self.make_physical(entity_id);
        if self.id_to_physics.get(&entity_id).is_none() {
            // No physics representation (e.g. a modelless item): don't strand a
            // static object in the air - leave it in the backpack (it reappears
            // in the strip once the cursor cleared).
            return;
        }

        self.detach_from_containers(entity_id);
        let throw_pos = vec3(
            origin.x + forward.x * THROW_SPAWN_DISTANCE,
            origin.y + forward.y * THROW_SPAWN_DISTANCE,
            origin.z + forward.z * THROW_SPAWN_DISTANCE,
        );
        self.set_entity_position_rotation(
            entity_id,
            throw_pos,
            Quaternion::new(1.0, 0.0, 0.0, 0.0),
            vec3(1.0, 1.0, 1.0),
        );
        self.physics.set_velocity(entity_id, forward * THROW_SPEED);
        // A thrown item is a normal referenced world object again.
        self.world.add_component(entity_id, PropHasRefs(true));
        self.script_world.dispatch(Message {
            payload: MessagePayload::Drop,
            to: entity_id,
        });
    }

    pub fn set_entity_position_rotation(
        &mut self,
        entity_id: EntityId,
        position: Vector3<f32>,
        rotation: Quaternion<f32>,
        scale: Vector3<f32>,
    ) {
        if let Some(rigid_body_handle) = self.id_to_physics.get(&entity_id) {
            self.physics
                .set_position_rotation(*rigid_body_handle, position, rotation);
        } else {
            let translation_matrix = Matrix4::from_translation(position);
            let rotation_matrix = Matrix4::<f32>::from(rotation);
            let scale_matrix = Matrix4::from_nonuniform_scale(scale.x, scale.y, scale.z);
            let xform = translation_matrix * rotation_matrix * scale_matrix;

            let v_entities = self.world.borrow::<EntitiesView>().unwrap();
            let mut v_transform = self
                .world
                .borrow::<ViewMut<RuntimePropTransform>>()
                .unwrap();

            let mut v_prop_position = self.world.borrow::<ViewMut<PropPosition>>().unwrap();

            v_entities.add_component(entity_id, &mut v_transform, RuntimePropTransform(xform));
            v_entities.add_component(
                entity_id,
                &mut v_prop_position,
                PropPosition {
                    position,
                    rotation,
                    cell: 0,
                },
            );
        }
    }

    pub fn create_entity_with_position(
        &mut self,
        asset_cache: &mut AssetCache,
        template_id: i32,
        position: Point3<f32>,
        orientation: Quaternion<f32>,
        root_transform: Matrix4<f32>,
        additional_options: CreateEntityOptions,
    ) -> EntityCreationInfo {
        self.create_entity_with_position_and_rider_depth(
            asset_cache,
            template_id,
            position,
            orientation,
            root_transform,
            additional_options,
            0,
            None,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn create_entity_with_position_and_rider_depth(
        &mut self,
        asset_cache: &mut AssetCache,
        template_id: i32,
        position: Point3<f32>,
        orientation: Quaternion<f32>,
        root_transform: Matrix4<f32>,
        additional_options: CreateEntityOptions,
        rider_depth: u32,
        // The template of the entity this creation is attached to, if it was
        // itself spawned off a ParticleAttachement link. The link that caused
        // this creation must not be walked again from the other end (a trail
        // riding a projectile must not re-instantiate the projectile; a decal
        // brought along by a spang must not host a second spang).
        exclude_template: Option<i32>,
    ) -> EntityCreationInfo {
        let transient_fx = additional_options.transient_fx;
        let created_entity = {
            entity_creator::create_entity_with_position(
                template_id,
                position,
                orientation,
                root_transform,
                &mut self.world,
                &mut self.physics,
                asset_cache,
                &mut self.script_world,
                &self.entity_info,
                &self.obj_map,
                &self.template_to_entity_id,
                additional_options,
            )
        };

        let info = Self::finish_instantiating_entity(
            &mut self.id_to_model,
            &mut self.id_to_bitmap,
            &mut self.id_to_physics,
            &mut self.id_to_animation_player,
            &mut self.physics,
            &mut self.world,
            &mut self.script_world,
            created_entity,
            root_transform,
        );

        // Instantiate the particle groups authored to ride this archetype
        // (`ParticleAttachement` links from particle archetypes to this
        // template or an ancestor) - projectile trails, psi bolt visuals.
        // Mission-placed hosts already have their particle entities placed in
        // the level, so this only runs for runtime creations; the recursion
        // also instantiates nested attachments (a spang's own rider). The
        // depth cap bounds authored cycles (shipped data nests 2 deep).
        const MAX_RIDER_DEPTH: u32 = 3;
        if rider_depth >= MAX_RIDER_DEPTH {
            return info;
        }
        let riders: Vec<i32> = {
            let hierarchy = ss2_entity_info::get_hierarchy(&self.entity_info);
            let mut ancestors = ss2_entity_info::get_ancestors(hierarchy, &template_id);
            ancestors.push(template_id);
            let mut riders: Vec<i32> = ancestors
                .iter()
                .flat_map(|t| {
                    self.template_to_particle_riders
                        .get(t)
                        .map(|riders| {
                            riders
                                .iter()
                                .map(|(rider, _opts)| *rider)
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default()
                })
                .collect();
            // A transient impact FX is the root of its own creation, so the
            // archetypes its own ParticleAttachement links point at (the
            // bullet spang's "Bullet Hit" decal) don't exist yet - bring them
            // along too. Only for transient FX: a live projectile's outgoing
            // link is cosmetic authoring for OTHER hosts (the player Fusion
            // Shot's link to the droid variant) and must not spawn here.
            if transient_fx {
                riders.extend(ancestors.iter().flat_map(|t| {
                    self.template_to_particle_attachees
                        .get(t)
                        .cloned()
                        .unwrap_or_default()
                }));
            }
            // Exclude the host template AND its ancestors: both lookups walk
            // the whole ancestor chain, so a link authored on an ancestor
            // would otherwise be walked back from the other end (spawning a
            // spurious duplicate of the host's ancestor archetype).
            let excluded: Vec<i32> = exclude_template
                .map(|t| {
                    let mut excluded = ss2_entity_info::get_ancestors(hierarchy, &t);
                    excluded.push(t);
                    excluded
                })
                .unwrap_or_default();
            riders
                .into_iter()
                .filter(|t| !excluded.contains(t))
                .collect()
        };
        for particle_template in riders {
            // vhot/joint offsets are not applied yet - the particle rides the
            // host origin (attach type "object" covers the shipped projectile
            // trails).
            let rider = self.create_entity_with_position_and_rider_depth(
                asset_cache,
                particle_template,
                position,
                orientation,
                root_transform,
                CreateEntityOptions {
                    attach_to: Some(info.entity_id),
                    transient_fx,
                    ..CreateEntityOptions::default()
                },
                rider_depth + 1,
                Some(template_id),
            );
            // Riders are pure visuals: strip any physics the template brought
            // (some riders are full projectile archetypes - e.g. the droid
            // fusion shot rides the player Fusion Shot for its looks - and
            // must not fly off / collide / slay as a second live projectile),
            // and keep them out of saves (they are recreated with their host
            // and would otherwise load back orphaned, since attachments are
            // runtime-only).
            self.make_un_physical(rider.entity_id);
            // ...and any gameplay scripts (a projectile-archetype rider must
            // not raycast/damage as a live projectile).
            self.script_world.remove_entity(rider.entity_id);
            self.world
                .add_component(rider.entity_id, RuntimePropDoNotSerialize {});
        }

        info
    }

    fn finish_instantiating_entity(
        id_to_model: &mut HashMap<EntityId, Model>,
        id_to_bitmap: &mut HashMap<EntityId, Rc<BitmapAnimation>>,
        id_to_physics: &mut HashMap<EntityId, RigidBodyHandle>,
        id_to_animation_player: &mut HashMap<EntityId, AnimationPlayer>,
        physics: &mut PhysicsWorld,
        world: &mut World,
        script_world: &mut ScriptWorld,
        created_entity: EntityCreationInfo,
        root_transform: Matrix4<f32>,
    ) -> EntityCreationInfo {
        let ret = created_entity.clone();

        if let Some((model, maybe_animation_player)) = created_entity.model {
            id_to_model.insert(created_entity.entity_id, model);

            if let Some(animation_player) = maybe_animation_player {
                id_to_animation_player.insert(created_entity.entity_id, animation_player);
            }
        }

        if let Some(bitmap_animation) = created_entity.bitmap_animation {
            id_to_bitmap.insert(created_entity.entity_id, bitmap_animation);
        }

        let v_initial_velocity = world.borrow::<View<PropPhysInitialVelocity>>().unwrap();
        if let Some(rigid_body) = created_entity.rigid_body {
            let initial_velocity = v_initial_velocity
                .get(created_entity.entity_id)
                // Not sure why the coordinate system is different for projectile launch?
                .map(|v| vec3(v.0.z, v.0.y, v.0.x))
                //.map(|v| vec3(v.0.z.abs(), v.0.y.abs(), v.0.x.abs()))
                .unwrap_or(vec3(0.0, 0.0, 0.0));

            let mag = initial_velocity.magnitude();
            let x_velocity = root_transform.transform_vector(vec3(0.0, 0.0, mag));
            if initial_velocity.magnitude() > 80.0 {
                // Use raycast strategy for fast moving objects
                script_world.add_entity2(
                    created_entity.entity_id,
                    Box::new(InternalFastProjectileScript::new(x_velocity)),
                );
                // HACK: Don't use physics for these entities...
                physics.remove(created_entity.entity_id);
            } else {
                id_to_physics.insert(created_entity.entity_id, rigid_body);
                physics.set_velocity(created_entity.entity_id, (x_velocity / SCALE_FACTOR) * 1.5);
            }
        };

        ret
    }

    /// Canonical destruction cleanup for one runtime entity. Keep interaction
    /// state and incoming inventory links in sync before the entity id can be
    /// recycled, then tear down its scripts, physics, render state, and
    /// attached children through [`Self::remove_entity`].
    fn destroy_entity(&mut self, entity_id: EntityId) {
        self.interaction.on_entity_destroyed(entity_id);
        self.flat_ui.on_entity_destroyed(entity_id);
        // `PlayerInfo` mirrors the interaction controller each update, but
        // later effects in this batch may inspect it before then.
        if let Ok(mut player) = self.world.borrow::<UniqueViewMut<PlayerInfo>>() {
            if player.left_hand_entity_id == Some(entity_id) {
                player.left_hand_entity_id = None;
            }
            if player.right_hand_entity_id == Some(entity_id) {
                player.right_hand_entity_id = None;
            }
        }
        // Only a contained item (no world presence, PropHasRefs false) can
        // leave a dangling inventory `Contains` link behind.
        if !crate::util::has_refs(&self.world, entity_id) {
            self.remove_incoming_contains_links(entity_id);
        }
        self.remove_entity(entity_id);
    }

    pub fn remove_entity(&mut self, entity_id: EntityId) {
        // Entities riding this one (attached particle trails/FX) die with it -
        // otherwise a projectile's trail would linger at its last transform
        // forever after the projectile is destroyed. Exception: a rider with
        // its own running delete tweq manages its own lifetime (e.g. the
        // "Bullet Hit" decal riding a bullet spang is authored to linger 10s,
        // long past the spang's ~0.8s burst) - it is detached instead, frozen
        // at its current transform, and its tweq destroys it at the authored
        // time. The transitive closure is collected iteratively with a visited
        // set so mission-authored attachment cycles can't recurse forever.
        let mut to_remove: Vec<EntityId> = vec![entity_id];
        let mut to_detach: Vec<EntityId> = Vec::new();
        let mut visited: HashSet<EntityId> = HashSet::from([entity_id]);
        let mut frontier = vec![entity_id];
        while let Some(parent) = frontier.pop() {
            let children: Vec<(EntityId, bool)> = match self.world.borrow::<(
                View<crate::runtime_props::RuntimePropAttachment>,
                View<PropTweqDeleteConfig>,
                View<PropTweqDeleteState>,
            )>() {
                Ok((v_attach, v_delete_config, v_delete_state)) => v_attach
                    .iter()
                    .with_id()
                    .filter(|(id, a)| a.parent == parent && !visited.contains(id))
                    .map(|(id, _)| {
                        // Only a *running* delete tweq counts - a config whose
                        // state is off would never fire, and the rider would
                        // leak forever if spared from the cascade.
                        let self_expiring = v_delete_config.contains(id)
                            && (&v_delete_state)
                                .get(id)
                                .map(|s| s.animation_state.contains(TweqAnimationState::ON))
                                .unwrap_or(false);
                        (id, self_expiring)
                    })
                    .collect(),
                Err(_) => Vec::new(),
            };
            for (child, self_expiring) in children {
                visited.insert(child);
                if self_expiring {
                    to_detach.push(child);
                } else {
                    to_remove.push(child);
                    frontier.push(child);
                }
            }
        }
        // Cut the attachment on survivors so they stop tracking the (about to
        // be deleted) host and hold their last transform. Their own riders
        // stay attached to them and die with them when the tweq fires.
        for id in to_detach {
            self.world
                .remove::<(crate::runtime_props::RuntimePropAttachment,)>(id);
        }
        // Children first, host last (reverse discovery order).
        for id in to_remove.into_iter().rev() {
            self.remove_entity_single(id);
        }
    }

    fn remove_entity_single(&mut self, entity_id: EntityId) {
        // Shipyard recycles entity ids, so stale per-entity animation state
        // must not outlive the entity (a recycled id would inherit it).
        self.id_to_animation_player.remove(&entity_id);
        self.failed_animation_queries.remove(&entity_id);
        self.gui.on_entity_destroyed(
            entity_id,
            &mut self.world,
            &mut self.physics,
            &mut self.script_world,
            &mut self.id_to_physics,
        );
        self.hit_boxes.remove_entity(
            entity_id,
            &mut self.world,
            &mut self.script_world,
            &mut self.physics,
            &mut self.id_to_physics,
        );
        self.rag_doll_manager
            .remove_entity(entity_id, &mut self.physics);

        self.script_world.remove_entity(entity_id);
        self.id_to_bitmap.remove(&entity_id);
        self.id_to_model.remove(&entity_id);
        self.id_to_physics.remove(&entity_id);
        // Also drop the entity's particle system - the render loop iterates
        // this map directly, so a stale entry would keep emitting at the
        // entity's last transform forever.
        self.id_to_particle_system.remove(&entity_id);
        self.physics.remove(entity_id);

        self.world.delete_entity(entity_id);
    }

    /// Spawn a ragdoll from a (dying) creature and remove the original.
    ///
    /// The ragdoll is created as a dedicated "corpse" entity seeded from the
    /// creature's *current* bone world transforms (no offset), then the original
    /// creature is removed. Using a separate entity id is important: the original
    /// creature is torn down via `remove_entity`, which also clears any ragdoll
    /// keyed by that id - so the corpse must live under its own id to survive.
    /// Replace a creature with a physics ragdoll of its current pose. `use_multibody`
    /// selects reduced-coordinate (multibody) joints over the legacy impulse joints.
    /// `crumpled_pose` marks rigs spawned from a finished death crumple (floor-
    /// lying, limb-overlapping): those spawn without limb self-collision (the
    /// pose's deep limb-limb contacts can explode the articulated solve - see
    /// `CollisionGroup::ragdoll_no_self`) and with a small vertical lift so the
    /// fitted colliders don't start inside the level trimesh. Standing/mid-
    /// animation spawns (instant slays, the debug scene) pass false and keep
    /// the full self-colliding rig with no lift.
    /// Returns the corpse entity id if a ragdoll was spawned (the creature is
    /// then removed) - the id keys the ragdoll in `rag_doll_manager` (e.g. for
    /// `apply_impact`).
    pub fn spawn_ragdoll(
        &mut self,
        entity_id: EntityId,
        use_multibody: bool,
        crumpled_pose: bool,
    ) -> Option<EntityId> {
        let (spawned, ragdoll_id) = {
            let model = match self.id_to_model.get(&entity_id) {
                Some(model) if model.can_create_rag_doll() => model,
                _ => return None,
            };

            let (root_transform, joint_transforms) = {
                let v_transform = self.world.borrow::<View<RuntimePropTransform>>().unwrap();
                let v_joint_transforms = self
                    .world
                    .borrow::<View<RuntimePropJointTransforms>>()
                    .unwrap();

                let root_transform = match v_transform.get(entity_id) {
                    Ok(transform) => transform.0,
                    Err(_) => return None,
                };
                let joint_transforms = match v_joint_transforms.get(entity_id) {
                    Ok(joints) => joints.0,
                    Err(_) => return None,
                };
                (root_transform, joint_transforms)
            };

            // Per-bone joint limits from the creature definition (empty for
            // creatures without a humanoid skeleton -> uniform cone fallback).
            let joint_limits = crate::creature::get_entity_creature(&self.world, entity_id)
                .map(|creature| creature.joint_limits.clone())
                .unwrap_or_else(|| std::sync::Arc::new(std::collections::HashMap::new()));

            // Dedicated corpse entity so removing the original creature below does
            // not tear down the ragdoll (the manager keys ragdolls by entity id).
            let ragdoll_id = self.world.add_entity(RuntimePropDoNotSerialize {});

            // A crumple pose ends lying ON the floor, so the fitted colliders
            // start interpenetrating the level trimesh - the deep-penetration
            // recovery through the articulated solver exploded ~half of
            // medsci1 handoffs to non-finite positions within a step. A few cm
            // of clearance lets the rig drop back down instead (imperceptible
            // at spawn). Standing spawns don't need it.
            let lift = if crumpled_pose {
                RAGDOLL_SPAWN_LIFT
            } else {
                0.0
            };
            // The creature's live capsule velocity IS the crumple's root
            // motion (animation drives the body through physics velocity), so
            // it seeds the rig's bulk momentum across the handoff.
            let seed_velocity = self
                .physics
                .get_velocity(entity_id)
                .unwrap_or_else(Vector3::zero);
            let spawned = self.rag_doll_manager.add_ragdoll(
                ragdoll_id,
                model,
                root_transform,
                &joint_transforms,
                vec3(0.0, lift, 0.0),
                &joint_limits,
                use_multibody,
                seed_velocity,
                !crumpled_pose,
                &mut self.physics,
            );
            (spawned, ragdoll_id)
        };

        if spawned {
            // Replace the creature with the corpse: remove its capsule, hitboxes,
            // AI scripts, and animated model so only the ragdoll remains.
            self.remove_entity(entity_id);
            println!("Spawned ragdoll and removed creature {:?}", entity_id);
            Some(ragdoll_id)
        } else {
            None
        }
    }

    /// Drop out of the flat "use" (metagame) mode: put the inventory strip
    /// away and let go of anything on the cursor. The cursor item was never
    /// removed from the backpack (the host only hid it from the strip), so
    /// clearing the cursor is enough - the item is already reachable there
    /// (projects/flat-ui.md §1.5), and a save or transition mid-drag serializes
    /// it correctly with no orphan.
    ///
    /// Shared by `ToggleUseMode` and `CloseUseMode` so the two exits from use
    /// mode cannot drift apart.
    fn leave_use_mode(&mut self) -> Effect {
        self.use_mode = false;
        self.flat_ui.take_cursor_item();
        self.flat_ui.set_strip(None);
        // VR weapon-safe exit: a trigger still held from inside the mode must
        // be released before the hands see it again, or leaving the cyber
        // interface would fire the wielded weapon on a stale press. Inert in
        // flat (the consumption site is VR-gated) and self-clearing once
        // nothing is pressed.
        self.vr_trigger_swallow = true;
        // The panel disappears immediately; the ramp keeps easing the
        // vignette/dim/FOV pull back to nothing on its own release timing
        // (see `render`'s `use_mode_ramp.is_settled_closed()` gate).
        self.use_mode_ramp.close();
        Effect::PlaySound {
            handle: AudioHandle::new(),
            source: None,
            name: USE_MODE_CLOSE_SOUND.to_owned(),
            spatial: false,
        }
    }

    /// Enter "use" (metagame) mode: bind the top-docked inventory strip to
    /// the player's `internal_inventory` entity (whose GuiScript already
    /// emits SetUI every frame). Shared by both presentations - the strip
    /// canvas is the mode; only where it is presented differs.
    fn enter_use_mode(&mut self) -> Effect {
        self.use_mode = true;
        let strip_entity = self
            .world
            .borrow::<UniqueView<PlayerInfo>>()
            .ok()
            .map(|player| player.inventory_entity_id);
        self.flat_ui.set_strip(strip_entity);
        // Entered "already pressed": the button that opened the mode may still
        // be down (in VR the interface can be opened with the trigger held), and
        // a held press must never read as a click on whatever the pointer first
        // crosses (rule 6 of the vr-ui-design skill).
        self.flat_ui.guard_held_press();
        self.use_mode_ramp
            .open(crate::ui::entry_ramp::DEFAULT_ENTRY_EXIT);
        Effect::PlaySound {
            handle: AudioHandle::new(),
            source: None,
            name: USE_MODE_OPEN_SOUND.to_owned(),
            spatial: false,
        }
    }

    pub fn handle_effects(
        &mut self,
        effects: Vec<Effect>,
        global_context: &GlobalContext,
        game_options: &GameOptions,
        asset_cache: &mut AssetCache,
        audio_context: &mut AudioContext<EntityId, String>,
    ) -> Vec<GlobalEffect> {
        let mut global_effects = Vec::new();
        let player_entity = {
            let player_info = self.world.borrow::<UniqueView<PlayerInfo>>().unwrap();
            player_info.entity_id
        };

        // At most one player hurt grunt per drain - see the `AdjustHitPoints`
        // handler.
        let mut player_grunted = false;
        let mut effects = VecDeque::from(effects);
        while let Some(effect) = effects.pop_front() {
            match effect {
                Effect::AcquireKeyCard { key_card } => {
                    let mut quests = self.world.borrow::<UniqueViewMut<QuestInfo>>().unwrap();
                    quests.add_key_card(key_card);
                    drop(quests);
                }

                Effect::AdjustHitPoints { entity_id, delta } => {
                    // Once death has started, stray queued damage/healing must
                    // not churn the terminal pool or revive the player outside
                    // the reconstruction path.
                    if entity_id == player_entity && !self.player_is_alive() {
                        continue;
                    }
                    let mut v_hit_points = self
                        .world
                        .borrow::<ViewMut<dark::properties::PropHitPoints>>()
                        .unwrap();

                    if let Ok(hit_points) = (&mut v_hit_points).get(entity_id) {
                        let previous = hit_points.hit_points;
                        hit_points.hit_points = hit_points.hit_points.saturating_add(delta).max(0);
                        // Every HP change flows through here (weapon, stim,
                        // collision damage) - trace it with the resulting
                        // total so a mysterious death is attributable.
                        let hp = hit_points.hit_points;
                        drop(v_hit_points);
                        tracing::debug!(
                            "hp: {} {:+} -> {}",
                            debug_entity(&self.world, entity_id),
                            delta,
                            hp
                        );
                        // Tell the player they were hit. This is the one hook
                        // for it: every source of damage - AI melee,
                        // projectiles, psi, falls - lands here, and what it
                        // reads is the loss that was actually *applied*, so
                        // armour or a resistance that soaked a hit makes the
                        // feedback smaller rather than lying about it.
                        // Deliberately after the death check's `previous > 0`
                        // guard reads `previous`, and skipped for the killing
                        // blow, which the death path owns.
                        if entity_id == player_entity && hp < previous {
                            let damage = (previous - hp) as f32;
                            // The tint fires for the killing blow too - that
                            // is the hit the player most needs to see land.
                            global_effects.push(GlobalEffect::PlayerHit { damage });
                            // The grunt does not: `begin_player_death` plays
                            // the death vocalization, and two player voices at
                            // once is a mess. Nor does a second grunt for a
                            // second hit drained in the same pass - several
                            // damage effects can land together (a blast plus a
                            // follow-up), and the tint coalesces them where
                            // stacked audio would just be noise.
                            if hp > 0 && !player_grunted {
                                player_grunted = true;
                                effects.push_back(Effect::PlaySound {
                                    handle: AudioHandle::new(),
                                    name: crate::hit_feedback::hurt_schema(damage).to_owned(),
                                    source: Some(player_entity),
                                    // The player's own voice: at the ears, not
                                    // at a point in the world they stand on.
                                    spatial: false,
                                });
                            }
                        }
                        if entity_id == player_entity && previous > 0 && hp == 0 {
                            for death_effect in self.begin_player_death() {
                                effects.push_back(death_effect);
                            }
                        }
                    }
                }

                Effect::AdjustStackCount { entity_id, delta } => {
                    let mut stacks = self
                        .world
                        .borrow::<ViewMut<dark::properties::PropStackCount>>()
                        .unwrap();
                    if let Ok(stack) = (&mut stacks).get(entity_id) {
                        stack.0 = (stack.0 + delta).max(0);
                    }
                }

                Effect::AdjustAmmo { entity_id, delta } => {
                    let mut v_gun_state = self
                        .world
                        .borrow::<ViewMut<dark::properties::PropGunState>>()
                        .unwrap();

                    if let Ok(gun_state) = (&mut v_gun_state).get(entity_id) {
                        gun_state.ammo = (gun_state.ammo + delta).max(0);
                    }
                }

                Effect::RechargeAmmo {
                    entity_id,
                    capacity,
                } => {
                    let mut v_gun_state = self
                        .world
                        .borrow::<ViewMut<dark::properties::PropGunState>>()
                        .unwrap();

                    if let Ok(gun_state) = (&mut v_gun_state).get(entity_id) {
                        crate::scripts::effect::recharge_ammo_to_capacity(gun_state, capacity);
                    }
                }

                Effect::RadiusBlast {
                    center,
                    radius,
                    intensity,
                    stim_template_id,
                } => {
                    self.radius_blast(center, radius, intensity, stim_template_id);
                }

                Effect::RaiseNoise { origin, radius } => {
                    self.raise_noise(origin, radius);
                }

                Effect::ReloadWeapon => {
                    // Whichever hand holds the gun - the input action is not
                    // hand-specific (see `crate::wielded_weapon`).
                    if let Some(weapon) = crate::wielded_weapon::wielded_weapon(&self.world) {
                        self.begin_reload(weapon);
                    }
                }

                Effect::CycleAmmo => {
                    if let Some(weapon) = crate::wielded_weapon::wielded_weapon(&self.world) {
                        self.cycle_ammo(weapon);
                    }
                }

                Effect::ToggleUseMode => {
                    match game_options.presentation_mode {
                        crate::PresentationMode::Flat => {
                            // Tab dismisses an open overlay first, as the
                            // original does - reading a log (or looting a
                            // container) and pressing Tab should put the panel
                            // away, not drop the player into shooter mode with
                            // it still up.
                            if self.flat_ui.active_panel().is_some() {
                                self.flat_ui.close();
                            } else if self.use_mode {
                                effects.push_front(self.leave_use_mode());
                            } else {
                                effects.push_front(self.enter_use_mode());
                            }
                        }
                        crate::PresentationMode::Vr => {
                            // The cyber interface: the same use-mode canvas,
                            // presented on a head-anchored world panel with the
                            // world dimmed behind it and the weapon safed. The
                            // world keeps simulating - unlike the pause menu,
                            // this is a mode of play, not a suspension of it.
                            if self.use_mode {
                                effects.push_front(self.leave_use_mode());
                            } else if self.player_is_alive() {
                                // Edge policy: no cyber interface over the
                                // death sequence - that moment belongs to the
                                // game-over flow (same rule as the pause
                                // menu's `pause_is_allowed`). The other modal
                                // owners can't reach this handler at all: the
                                // pause menu clears triggered actions while
                                // open, and a level transition replaces this
                                // scene (taking the mission-owned mode state
                                // with it).
                                effects.push_front(self.enter_use_mode());
                                // Place the panel from the pose of *this*
                                // entry, not wherever the player stood last
                                // time - and let the anchor wait out an
                                // untracked (zero-quaternion) head rather than
                                // locking in a garbage placement.
                                self.vr_use_mode_anchor = crate::ui::FrontendPanelAnchor::new();
                                self.vr_use_mode_head = crate::ui::world_dim::UNTRACKED_HEAD;
                            }
                        }
                    }
                }

                Effect::CloseUseMode => {
                    // Idempotent counterpart to `ToggleUseMode`, used when
                    // something outside the mission (the pause menu) takes over
                    // the screen: put the overlay away and drop back to shooter
                    // mode, dropping any cursor item back into the backpack it
                    // was never actually removed from. Applies in both
                    // presentations - the pause menu must not open over a live
                    // cyber interface.
                    self.flat_ui.close();
                    if self.use_mode {
                        effects.push_front(self.leave_use_mode());
                    }
                }

                Effect::OpenPanel { entity } => {
                    // Bind the presentation's single object-panel slot to the
                    // frobbed entity (the original's frob-script -> overlay
                    // flow). Flat docks it in the MFD; default VR creates a
                    // world quad beside the object. The explicit experimental
                    // mode retains its historical all-panels presentation.
                    match game_options.presentation_mode {
                        crate::PresentationMode::Flat => self.flat_ui.open(entity),
                        crate::PresentationMode::Vr => self.gui.open_panel(
                            entity,
                            game_options.experimental_features.contains("gui"),
                            &mut self.world,
                            &mut self.physics,
                            &mut self.script_world,
                            &mut self.id_to_physics,
                        ),
                    }
                    // Give the gui its per-open state (the reader's scroll
                    // reset) however the panel was reached.
                    self.script_world.dispatch(Message {
                        to: entity,
                        payload: MessagePayload::PanelOpened,
                    });
                }

                Effect::BeginResearch { entity_id } => {
                    let carried = crate::scripts::script_util::player_carried_items(&self.world);
                    if !carried.contains(&entity_id) {
                        continue;
                    }
                    let Some(template_id) = crate::scripts::script_util::entity_class_template_id(
                        &self.world,
                        entity_id,
                    ) else {
                        continue;
                    };
                    let required = self
                        .world
                        .borrow::<View<dark::properties::PropBaseTechDesc>>()
                        .unwrap()
                        .get(entity_id)
                        .map(|skills| skills.0.research().max(1))
                        .unwrap_or(1);
                    let mut quests = self.world.borrow::<UniqueViewMut<QuestInfo>>().unwrap();
                    let skill = quests
                        .player_stats()
                        .skill_level(crate::player_stats::Skill::Research);
                    let result = quests.research_mut().begin(template_id, required, skill);
                    drop(quests);
                    match result {
                        crate::research::BeginResearchResult::Started => {
                            game_log!(INFO, "Research started");
                        }
                        crate::research::BeginResearchResult::SkillRequired(required) => {
                            game_log!(INFO, "Research skill {} required", required);
                        }
                        crate::research::BeginResearchResult::AlreadyComplete => {
                            self.world.add_component(
                                entity_id,
                                PropObjState(dark::properties::ObjectState::Normal),
                            );
                        }
                    }
                }

                Effect::UseResearchChemical { entity_id } => {
                    if apply_research_chemical(&self.world, entity_id) {
                        let stack = self
                            .world
                            .borrow::<View<dark::properties::PropStackCount>>()
                            .ok()
                            .and_then(|stacks| stacks.get(entity_id).ok().map(|stack| stack.0));
                        if stack.unwrap_or(1) > 1 {
                            let mut stacks = self
                                .world
                                .borrow::<ViewMut<dark::properties::PropStackCount>>()
                                .unwrap();
                            if let Ok(stack) = (&mut stacks).get(entity_id) {
                                stack.0 -= 1;
                            }
                        } else {
                            self.destroy_entity(entity_id);
                        }
                        game_log!(INFO, "Research chemical consumed");
                    } else {
                        game_log!(INFO, "That chemical is not needed");
                    }
                }

                Effect::CollectLog {
                    entity_id,
                    deck,
                    log,
                } => {
                    // Retail pickup files the identity and acknowledges it; it
                    // does not play the log or open the reader. Quest now has
                    // a production Y binding for the same explicit playback
                    // action as flat presentation, so the old VR auto-play
                    // stopgap is no longer needed.
                    {
                        let mut quests = self.world.borrow::<UniqueViewMut<QuestInfo>>().unwrap();
                        quests.collect_log(deck, log);
                    }
                    // Retail copies the entry into the PDA and destroys the
                    // pickup. Retire it from rendered, physical and container
                    // presence; the player-owned reader carries presentation.
                    self.consume_log_pickup(entity_id);
                }

                Effect::ReadLastUnreadLog { head_rotation } => {
                    let panel_entity = self
                        .world
                        .borrow::<UniqueView<MediaPanelEntity>>()
                        .ok()
                        .map(|panel| panel.0);
                    let Some(panel_entity) = panel_entity else {
                        game_log!(WARN, "audio log reader host is unavailable");
                        continue;
                    };

                    // Y is also the production dismiss affordance in VR. The
                    // next press re-enters this path, re-resolves and replays
                    // the latest collected log even when it is already read.
                    if game_options.presentation_mode == crate::PresentationMode::Vr
                        && self.gui.active_panel() == Some(panel_entity)
                    {
                        self.gui.close_panel(
                            &mut self.world,
                            &mut self.physics,
                            &mut self.script_world,
                            &mut self.id_to_physics,
                        );
                        continue;
                    }

                    // Resolving a candidate's reader strings can fail (a log
                    // collected on an earlier deck, a missing transcript), so
                    // walk the preference-ordered candidates and open the first
                    // one that actually resolves - taking only the front entry
                    // would let one unresolvable log wedge the control forever
                    // (it is never marked read, so it stays at the front).
                    let candidates: Vec<(u32, u32)> = self
                        .world
                        .borrow::<UniqueView<QuestInfo>>()
                        .map(|quests| {
                            quests
                                .logs_for_reader()
                                .map(|entry| (entry.deck, entry.log))
                                .collect()
                        })
                        .unwrap_or_default();
                    if candidates.is_empty() {
                        game_log!(INFO, "no collected audio log is available");
                        continue;
                    }
                    let resolved = candidates.into_iter().find(|(deck, log)| {
                        self.attach_log_reader_strings(panel_entity, *deck, *log, asset_cache)
                    });
                    let Some((deck, log)) = resolved else {
                        game_log!(
                            WARN,
                            "unable to open audio log reader: no collected log has a localized transcript here"
                        );
                        continue;
                    };
                    self.world.add_component(
                        panel_entity,
                        dark::properties::PropLog {
                            deck,
                            email: 33,
                            log,
                            note: 0,
                            video: 0,
                        },
                    );
                    self.script_world.dispatch(Message {
                        to: panel_entity,
                        payload: MessagePayload::PanelOpened,
                    });

                    match game_options.presentation_mode {
                        crate::PresentationMode::Flat => self.flat_ui.open_unbound(panel_entity),
                        crate::PresentationMode::Vr => {
                            let (player_position, player_rotation) = {
                                let player = self.world.borrow::<UniqueView<PlayerInfo>>().unwrap();
                                (player.pos, player.rotation * head_rotation)
                            };
                            let offset = player_rotation
                                * vec3(
                                    0.0,
                                    VR_MEDIA_UP / SCALE_FACTOR,
                                    -VR_MEDIA_FORWARD / SCALE_FACTOR,
                                );
                            let panel_position = player_position + offset;
                            let panel_rotation =
                                Quaternion::from_angle_y(cgmath::Deg(180.0)) * player_rotation;
                            self.world.add_component(
                                panel_entity,
                                PropPosition {
                                    position: panel_position,
                                    rotation: panel_rotation,
                                    cell: 0,
                                },
                            );
                            self.world.add_component(
                                panel_entity,
                                RuntimePropTransform(
                                    Matrix4::from_translation(panel_position)
                                        * Matrix4::from(panel_rotation),
                                ),
                            );
                            self.gui.open_panel(
                                panel_entity,
                                false,
                                &mut self.world,
                                &mut self.physics,
                                &mut self.script_world,
                                &mut self.id_to_physics,
                            );
                        }
                    }

                    // Only after a localized reader is bound to the active
                    // presentation do read state and audio advance together.
                    if let Ok(mut quest_info) = self.world.borrow::<UniqueViewMut<QuestInfo>>() {
                        quest_info.mark_log_read(deck, log);
                    }
                    effects.push_front(Effect::PlaySound {
                        handle: AudioHandle::new(),
                        source: None,
                        name: format!("LOG{deck:02}{log:02}"),
                        spatial: false,
                    });
                }

                Effect::ToggleMap => {
                    // Flat-presentation only, like OpenPanel: the automap is a
                    // flat MFD; VR panels are world quads (out of scope here).
                    if game_options.presentation_mode == crate::PresentationMode::Flat {
                        if let Ok(map) = self.world.borrow::<UniqueView<MapPanelEntity>>() {
                            let entity = map.0;
                            drop(map);
                            if self.flat_ui.active_panel() == Some(entity) {
                                self.flat_ui.close();
                            } else {
                                // Unbound: no world object -> no walk-away close.
                                self.flat_ui.open_unbound(entity);
                            }
                        }
                    }
                }

                Effect::RevealMapLocation { location } => {
                    let mission = self.level_name.to_ascii_lowercase();
                    if let Ok(mut quests) = self.world.borrow::<UniqueViewMut<QuestInfo>>() {
                        quests.reveal_map_location(&mission, location);
                    }
                    // Entering a mapped room also makes it the player's
                    // *current* map location (bright automap art + per-frame
                    // pip placement).
                    if let Ok(mut current) = self.world.borrow::<UniqueViewMut<PlayerMapLocation>>()
                    {
                        current.0 = Some(location);
                    }
                }

                Effect::CyclePsiPower => {
                    let powers = self.world.borrow::<UniqueView<GlobalPsiPowers>>().unwrap();
                    let known = self
                        .world
                        .borrow::<UniqueView<PlayerPsiKnownPowers>>()
                        .unwrap();
                    let mut selection = self
                        .world
                        .borrow::<UniqueViewMut<PsiPowerSelection>>()
                        .unwrap();
                    if !powers.0.is_empty() {
                        // Advance to the next *trained* power, wrapping (at
                        // most one lap - with a single trained power the lap
                        // lands back on it, so the selection never moves onto
                        // an untrained power).
                        for step in 1..=powers.0.len() {
                            let index = (selection.index + step) % powers.0.len();
                            if known.0.contains(&powers.0[index].template_id) {
                                selection.index = index;
                                break;
                            }
                        }
                        let power = &powers.0[selection.index];
                        game_log!(
                            INFO,
                            "Selected psi power: {} (tier {})",
                            power.name,
                            power.power.psi_cost
                        );
                    }
                }

                Effect::GrantPsiPower { template_id } => {
                    let powers = self.world.borrow::<UniqueView<GlobalPsiPowers>>().unwrap();
                    let mut known = self
                        .world
                        .borrow::<UniqueViewMut<PlayerPsiKnownPowers>>()
                        .unwrap();
                    if known.0.insert(template_id) {
                        let name = powers
                            .0
                            .iter()
                            .find(|p| p.template_id == template_id)
                            .map(|p| p.name.as_str())
                            .unwrap_or("<unknown power>");
                        game_log!(INFO, "Psi power trained: {} ({})", name, template_id);
                    }
                }

                Effect::SetPsiCharge {
                    entity_id,
                    fraction,
                    phase,
                } => {
                    self.world.add_component(
                        entity_id,
                        crate::runtime_props::RuntimePropPsiCharge { fraction, phase },
                    );
                }

                Effect::ClearPsiCharge { entity_id } => {
                    let mut v_charge = self
                        .world
                        .borrow::<ViewMut<crate::runtime_props::RuntimePropPsiCharge>>()
                        .unwrap();
                    v_charge.remove(entity_id);
                }

                Effect::SpendPsiPoints { amount } => {
                    update_player_psi_points(&self.world, |current| current.saturating_sub(amount));
                }

                Effect::SetPsiPoints { points } => {
                    update_player_psi_points(&self.world, |_| points);
                }

                Effect::UsePsiKit { entity_id, amount } => {
                    // This mutation and the consumption below happen while
                    // processing one effect against live state. Two boosters
                    // used in the same update therefore add independently;
                    // a duplicate use of an already-destroyed booster no-ops.
                    let outcome = apply_psi_kit_use(&self.world, entity_id, amount);
                    if outcome == PsiKitUseOutcome::NotUsed {
                        continue;
                    }

                    let sound = crate::scripts::script_util::play_environmental_sound(
                        &self.world,
                        entity_id,
                        "activate",
                        vec![],
                        AudioHandle::new(),
                    );
                    if outcome == PsiKitUseOutcome::DestroyEntity {
                        self.destroy_entity(entity_id);
                    }
                    if !matches!(sound, Effect::NoEffect) {
                        effects.push_front(sound);
                    }
                }

                Effect::UseComestible {
                    entity_id,
                    hit_points,
                } => {
                    // Resolve the heal and source lifetime against the same
                    // live world snapshot. A duplicate effect for an already
                    // destroyed object safely no-ops; full health still
                    // consumes, matching the retail script.
                    if apply_comestible_use(&self.world, entity_id, hit_points)
                        == ComestibleUseOutcome::NotUsed
                    {
                        continue;
                    }

                    let sound = crate::scripts::script_util::play_environmental_sound(
                        &self.world,
                        entity_id,
                        "activate",
                        vec![],
                        AudioHandle::new(),
                    );
                    self.destroy_entity(entity_id);
                    if !matches!(sound, Effect::NoEffect) {
                        effects.push_front(sound);
                    }
                }

                Effect::UseHealingItem {
                    entity_id,
                    total,
                    pulse,
                    first_pulse_secs,
                    pulse_interval_secs,
                } => {
                    let outcome = apply_healing_item_use(
                        &self.world,
                        entity_id,
                        total,
                        pulse,
                        first_pulse_secs,
                        pulse_interval_secs,
                    );
                    if outcome == HealingItemUseOutcome::NotUsed {
                        continue;
                    }

                    let sound = crate::scripts::script_util::play_environmental_sound(
                        &self.world,
                        entity_id,
                        "activate",
                        vec![],
                        AudioHandle::new(),
                    );
                    if outcome == HealingItemUseOutcome::DestroyEntity {
                        self.destroy_entity(entity_id);
                    }
                    if !matches!(sound, Effect::NoEffect) {
                        effects.push_front(sound);
                    }
                }

                Effect::InstallSoftware {
                    entity_id,
                    software,
                    level,
                } => {
                    // Two frobs of the same soft can be queued before either
                    // effect runs (both VR hands in one frame); the second must
                    // not report the soft it already installed as redundant.
                    let is_alive = self
                        .world
                        .borrow::<EntitiesView>()
                        .is_ok_and(|entities| entities.is_alive(entity_id));
                    if !is_alive {
                        continue;
                    }
                    // Atomic compare-and-install against live state: the sheet
                    // keeps the higher version, so a soft picked up after a
                    // better one is redundant (retail MISC.STR `SoftUseless`).
                    // Either way the soft is consumed - it never occupies
                    // inventory.
                    let installed = self
                        .world
                        .borrow::<UniqueViewMut<QuestInfo>>()
                        .unwrap()
                        .player_stats_mut()
                        .install_software(software, level);
                    if installed {
                        // Retail reports MISC.STR `SoftUpgrade0..3` here and
                        // `SoftUseless` below; the port has no player-facing
                        // message facility yet, so both go to the game log.
                        game_log!(INFO, "{:?} software upgraded to level {}", software, level);
                        effects.push_front(Effect::PlaySound {
                            handle: AudioHandle::new(),
                            source: None,
                            name: "boot_sw".to_owned(),
                            spatial: false,
                        });
                    } else {
                        game_log!(INFO, "Redundant software not installed.");
                    }
                    self.destroy_entity(entity_id);
                }

                Effect::ActivatePsiPower {
                    template_id,
                    name,
                    duration_secs,
                } => {
                    let mut active = self
                        .world
                        .borrow::<UniqueViewMut<crate::psi::ActivePsiPowers>>()
                        .unwrap();
                    if let Some(existing) =
                        active.0.iter_mut().find(|p| p.template_id == template_id)
                    {
                        existing.remaining_secs = duration_secs;
                        game_log!(INFO, "Psi power refreshed: {} ({}s)", name, duration_secs);
                    } else {
                        active.0.push(crate::psi::ActivePsiPower {
                            template_id,
                            name: name.clone(),
                            remaining_secs: duration_secs,
                        });
                        game_log!(INFO, "Psi power active: {} ({}s)", name, duration_secs);
                    }
                }

                Effect::AwardXP { amount } => {
                    // Cyber modules are the game's upgrade currency (retail's
                    // "XP"). Persisted on the character sheet inside QuestInfo,
                    // so the balance survives level transitions + save/load.
                    let mut quests = self.world.borrow::<UniqueViewMut<QuestInfo>>().unwrap();
                    let balance = quests.player_stats_mut().award_cyber_modules(amount);
                    if amount > 0 {
                        info!("Awarded {} cyber modules (balance now {})", amount, balance);
                    }
                }

                Effect::DrawDebugLines { lines } => {
                    if game_options.debug_draw {
                        for line in lines {
                            self.debug_lines.push(DebugLine {
                                start: line.0,
                                end: line.1,
                                color: vec3(line.2.x, line.2.y, line.2.z),
                                remaining_life_in_seconds: 0.1,
                            })
                        }
                    }
                }

                Effect::FlatMeleeSwing { entity_id } => {
                    self.queue_flat_melee_swing(asset_cache, entity_id);
                }

                Effect::CreateEntityByTemplateName {
                    source_entity_id,
                    template_name,
                    position,
                    orientation,
                    initial_velocity,
                } => {
                    if let Some(created) = self.create_entity_by_template_name_with_options(
                        asset_cache,
                        &template_name,
                        position,
                        orientation,
                        CreateEntityOptions {
                            launch_projectile: true,
                            ..CreateEntityOptions::default()
                        },
                    ) {
                        self.physics
                            .set_velocity(created.entity_id, initial_velocity);
                    } else {
                        warn!(
                            "Tweq emitter {:?} could not resolve authored template {:?}",
                            source_entity_id, template_name
                        );
                    }
                }

                Effect::CreateEntity {
                    template_id,
                    position,
                    orientation,
                    root_transform,
                    options,
                } => {
                    self.create_entity_with_position(
                        asset_cache,
                        template_id,
                        position,
                        orientation,
                        root_transform,
                        options,
                    );
                }
                Effect::SpawnEcologyEntity {
                    template_name,
                    spawn_point,
                    ecology_type,
                    goto_player,
                } => {
                    self.spawn_ecology_entity(
                        asset_cache,
                        &template_name,
                        spawn_point,
                        ecology_type,
                        goto_player,
                    );
                }
                Effect::DropEntityInfo {
                    parent_entity_id,
                    dropped_entity_id,
                } => {
                    self.drop_entity_into_container(parent_entity_id, dropped_entity_id);
                }

                Effect::EquipCarriedWeapon { class_template_id } => {
                    let maybe_weapon = crate::virtual_hand::carried_weapon_by_class(
                        &self.world,
                        class_template_id,
                    );
                    if let Some(entity_id) = maybe_weapon
                        // Either hand: in VR the weapon may already be held in
                        // the right one. Asked of the interaction controller
                        // rather than `PlayerInfo`, which only mirrors it once
                        // per update and so can be stale mid-effect-batch.
                        .filter(|entity| !self.interaction.is_holding(*entity))
                    {
                        effects.push_front(Effect::GrabEntity {
                            entity_id,
                            hand: crate::vr_config::Handedness::Right,
                            current_parent_id: None,
                        });
                    }
                }

                Effect::GrabEntity {
                    entity_id,
                    hand,
                    current_parent_id: _,
                } => {
                    // A grab that displaces something (the flat wield swapping
                    // the viewmodel out) holsters it back into the backpack
                    // itself, via the `StoreItem` the controller returns. VR
                    // grabs into an occupied hand no-op, so nothing is
                    // displaced there.
                    let grab_effects = self.interaction.grab(&self.world, entity_id, hand);
                    self.process_virtual_hand_effects(asset_cache, grab_effects);

                    if self.interaction.is_holding(entity_id) {
                        // A grab routed through this effect (equip a carried
                        // weapon, a backpack double-click) never emits
                        // `HoldItem`, so establish the melee contact body here
                        // too - otherwise a weapon equipped this way is held
                        // without a collider and never reports contacts.
                        if self.is_vr_melee_weapon(entity_id) {
                            self.attach_held_melee_physics(entity_id);
                        }

                        // Let the scripts know we are now holding the item..
                        self.script_world.dispatch(Message {
                            payload: MessagePayload::Hold,
                            to: entity_id,
                        });

                        let mut v_has_refs = self.world.borrow::<ViewMut<PropHasRefs>>().unwrap();
                        if let Ok(has_refs) = (&mut v_has_refs).get(entity_id) {
                            has_refs.0 = true;
                        }

                        let mut v_links = self.world.borrow::<ViewMut<Links>>().unwrap();

                        for links in (&mut v_links).iter() {
                            links.to_links.retain(|link| {
                                let is_link_to_entity = matches!(link.link, Link::Contains(_))
                                    && link.to_entity_id.is_some()
                                    && link.to_entity_id.unwrap().0 == entity_id;

                                !is_link_to_entity
                            })
                        }
                    }
                }
                Effect::SetJointTransform {
                    entity_id,
                    joint_id,
                    transform,
                } => {
                    let maybe_player = self.id_to_animation_player.get_mut(&entity_id);
                    if let Some(player) = maybe_player {
                        *player = AnimationPlayer::set_additional_joint_transform(
                            player, joint_id, transform,
                        )
                    }
                }

                Effect::QueueAnimationBySchema {
                    entity_id,
                    motion_queries,
                    selection_strategy,
                } => {
                    self.apply_animation_by_schema(
                        global_context,
                        asset_cache,
                        entity_id,
                        motion_queries,
                        selection_strategy,
                        AnimationPlayer::queue_animation,
                    );
                }

                Effect::PlayAnimationBySchema {
                    entity_id,
                    motion_queries,
                    selection_strategy,
                } => {
                    self.apply_animation_by_schema(
                        global_context,
                        asset_cache,
                        entity_id,
                        motion_queries,
                        selection_strategy,
                        AnimationPlayer::play_animation,
                    );
                }

                Effect::DebugCycleHitboxPose => {
                    // Advance every creature to the next pose in a fixed playlist of
                    // distinct biped-human clips, so repeated triggers walk
                    // deterministically through clearly-different poses. Used by the
                    // `debug_hitbox` scene to inspect per-joint hitbox/ragdoll fit
                    // across poses. We load clips by name directly (as `dark_viewer`
                    // does) rather than via the tag-based motion query, which needs
                    // an AI intent we don't have here.
                    const DEBUG_POSE_CLIPS: &[&str] = &[
                        "bh111005", // stand / idle
                        "BH111001", // gesture
                        "BH112020", // locomotion
                        "bh114001", // action
                        "BH413001", // combat
                        "BH212oo8", // reach
                        // Regression pose (#508): this stock idle gesture is
                        // one of 23 clips whose unanimated joints are stored
                        // as all-zero quaternions; it used to parse to NaN
                        // joint matrices and poison the hitbox kinematic
                        // bodies. Cycling to it must pose (bind rotation on
                        // those joints) without a single non-finite value.
                        "bh114009", // idle gesture with zero-quat joint tracks
                    ];
                    let clip_name =
                        DEBUG_POSE_CLIPS[self.debug_pose_index as usize % DEBUG_POSE_CLIPS.len()];

                    let creature_ids: Vec<EntityId> = {
                        let v_creature = self.world.borrow::<View<PropCreature>>().unwrap();
                        self.id_to_animation_player
                            .keys()
                            .filter(|id| v_creature.get(**id).is_ok())
                            .copied()
                            .collect()
                    };

                    if let Some(clip) =
                        asset_cache.get_opt(&ANIMATION_CLIP_IMPORTER, &format!("{clip_name}_.mc"))
                    {
                        for entity_id in creature_ids {
                            if let Some(player) = self.id_to_animation_player.get_mut(&entity_id) {
                                *player = AnimationPlayer::queue_animation(player, clip.clone());
                            }
                        }
                        self.debug_pose_index = self.debug_pose_index.wrapping_add(1);
                        game_log!(INFO, "[debug_hitbox] cycled to pose '{}'", clip_name);
                    } else {
                        game_log!(WARN, "[debug_hitbox] missing pose clip '{}_.mc'", clip_name);
                    }
                }

                Effect::Send { msg } => {
                    println!("handling Effect::Send event: {:?}", msg);
                    self.script_world.dispatch(msg);
                }

                Effect::SetUI {
                    parent_entity,
                    handle,
                    world_offset,
                    world_size,
                    components,
                } => {
                    // The FlatUiHost's canvas slots: the flat MFD panel
                    // (deliberately NOT behind `--experimental gui` - the flat
                    // MFD is the #435 fix) and the use-mode inventory strip,
                    // which BOTH presentations stash here - VR presents the
                    // same strip canvas on the cyber-interface world panel.
                    // The host only keeps components addressed to its bound
                    // slots, so this is inert outside those modes. VR's
                    // object-bound world panel still accepts only the panel
                    // explicitly opened through `OpenPanel` (below); the
                    // experimental mode retains its legacy all-panels
                    // behavior.
                    self.flat_ui
                        .on_set_ui(&self.world, parent_entity, world_size, &components);
                    let update_world_panel = game_options.experimental_features.contains("gui")
                        || (game_options.presentation_mode == crate::PresentationMode::Vr
                            && self.gui.active_panel() == Some(parent_entity));
                    if update_world_panel {
                        // `internal_inventory` is a 15-column strip: keep its
                        // shared canvas/layout identical, but map that resolved
                        // canvas to a comfortable physical width at the VR
                        // presentation boundary. Ordinary object panels retain
                        // their authored world size.
                        let is_player_backpack = self
                            .world
                            .borrow::<UniqueView<PlayerInfo>>()
                            .is_ok_and(|player| player.inventory_entity_id == parent_entity);
                        let presentation_size = presentation_world_panel_size(
                            game_options.presentation_mode,
                            is_player_backpack,
                            world_size,
                        );
                        self.gui.update_ui(
                            &mut self.world,
                            &mut self.physics,
                            &mut self.script_world,
                            &mut self.id_to_physics,
                            handle,
                            parent_entity,
                            world_size / crate::gui::GUI_PIXEL_TO_WORLD_SIZE,
                            presentation_size,
                            world_offset,
                            components,
                        );
                    }
                }

                Effect::ReplaceEntity {
                    entity_id,
                    template_id,
                } => {
                    let (position, rotation) = {
                        if let Some(handle) = &self.id_to_physics.get(&entity_id) {
                            let position = self.physics.get_position(**handle).unwrap();
                            let rotation = self.physics.get_rotation(**handle).unwrap();
                            // let scale_xform =
                            //     Matrix4::from_nonuniform_scale(scale.x.abs(), scale.y.abs(), scale.z.abs());
                            (position, rotation)
                        } else {
                            (
                                vec3(0.0, 0.0, 0.0),
                                Quaternion {
                                    s: 1.0,
                                    v: vec3(0.0, 0.0, 0.0),
                                },
                            )
                        }
                    };

                    let new_entity_info = self.create_entity_with_position(
                        asset_cache,
                        template_id,
                        vec3_to_point3(position),
                        rotation,
                        Matrix4::identity(),
                        CreateEntityOptions::default(),
                    );

                    if new_entity_info.rigid_body.is_some() {
                        let rigid_body = new_entity_info.rigid_body.unwrap();
                        self.interaction.replace_entity(
                            entity_id,
                            new_entity_info.entity_id,
                            rigid_body,
                        );
                    }

                    // Keep a contained item (e.g. an inventory power cell
                    // recharged at a station) in its container after replacement.
                    self.transfer_containment(entity_id, new_entity_info.entity_id);

                    self.remove_entity(entity_id);
                }

                Effect::ChangeModel {
                    entity_id,
                    model_name,
                } => {
                    // A VR-wielded first-person model loads as authored,
                    // baked hands included (see `VrHeldModel`); the
                    // separate importer is the seam where per-wield mesh
                    // preparation lives.
                    let is_vr = game_options.presentation_mode == crate::PresentationMode::Vr;
                    let vr_held = is_vr && crate::vr_config::is_vr_view_model(&model_name);
                    // The PsiSword authors no world model at all (PropLimbModel
                    // only), so it has no id_to_model entry to take a transform
                    // from - a VR wield materializes its first model from the
                    // entity transform instead. Every other ChangeModel still
                    // requires an existing model, exactly as before.
                    let maybe_xform = self
                        .id_to_model
                        .get(&entity_id)
                        .map(|model| model.get_transform())
                        .or_else(|| {
                            if !vr_held {
                                return None;
                            }
                            self.world
                                .borrow::<View<RuntimePropTransform>>()
                                .ok()
                                .and_then(|v| v.get(entity_id).ok().map(|t| t.0))
                        });
                    if let Some(xform) = maybe_xform {
                        let _ext_name = model_name.clone();
                        // Any model swap invalidates a computed grip; only the
                        // melee `_h` branch below re-derives one. Without this
                        // a drop would leave the restored world model wearing
                        // the wield's contact offset.
                        self.world.remove::<RuntimePropVrGripOffset>(entity_id);
                        // Was the *outgoing* model a VR-wielded first-person
                        // model? (Read before PropModelName is overwritten
                        // below - identifies the drop-restore swap.)
                        let was_vr_held = is_vr
                            && self
                                .world
                                .borrow::<View<PropModelName>>()
                                .ok()
                                .and_then(|v_model_name| {
                                    v_model_name
                                        .get(entity_id)
                                        .ok()
                                        .map(|model| crate::vr_config::is_vr_view_model(&model.0))
                                })
                                .unwrap_or(false);
                        // A missing model must never take down the frame (or a
                        // load): keep the current model and complain loudly.
                        let (new_model, vr_held_source) = if vr_held {
                            match asset_cache.get_opt(
                                &dark::importers::VR_HELD_MODELS_IMPORTER,
                                &format!("{model_name}.BIN"),
                            ) {
                                Some(source) => (
                                    Some(Model::transform(&source.as_ref().model, xform)),
                                    Some(source),
                                ),
                                None => (None, None),
                            }
                        } else {
                            (
                                asset_cache
                                    .get_opt(&MODELS_IMPORTER, &format!("{model_name}.BIN"))
                                    .map(|m| Model::transform(m.as_ref(), xform)),
                                None,
                            )
                        };
                        let Some(mut new_model) = new_model else {
                            tracing::error!(
                                "ChangeModel: model '{model_name}.BIN' could not be loaded for entity {entity_id:?} - keeping current model"
                            );
                            continue;
                        };

                        let vhots = new_model.vhots();
                        // An articulated VR-wielded first-person model (hand +
                        // arm + gun as skeleton sub-objects) renders unposed
                        // without a player, so give it the empty bind pose (it
                        // emits no motion flags or completion events). The
                        // player must NOT outlive the wield: the animation
                        // loop writes every player's root velocity into
                        // physics each frame, and an empty player's is zero -
                        // left in place after the drop-restore to the static
                        // world model it would pin the dropped weapon's
                        // horizontal velocity to zero. Scoped to the VR wield
                        // swap so every other ChangeModel (both presentations)
                        // behaves exactly as before this path existed.
                        if vr_held && new_model.is_animated() {
                            // Melee _h models are LGMM skinned meshes: the
                            // empty player's rest pose leaves the arm splayed
                            // mid-swing (same defect the flat viewmodel path
                            // documents), so hold the player-melee idle's
                            // final frame instead - a static pose that emits
                            // no motion flags, events, or root velocity
                            // (`from_completed_animation`). The flat viewmodel
                            // plays the same clip from frame 0 because there
                            // the swing animates; VR's hold is a frozen ready
                            // stance the tracked hand moves, so it takes the
                            // clip's settled end pose. Root motion is
                            // cancelled for the same reason as flat: the
                            // entity transform is re-anchored (there to the
                            // camera, here to the tracked hand) every frame.
                            let is_melee = self
                                .world
                                .borrow::<View<PropLimbModel>>()
                                .ok()
                                .is_some_and(|v| v.get(entity_id).is_ok());
                            if is_melee {
                                let player = asset_cache
                                    .get_opt(
                                        &ANIMATION_CLIP_IMPORTER,
                                        &format!("{MELEE_IDLE_CLIP}_.mc"),
                                    )
                                    .map(|clip| {
                                        AnimationPlayer::with_root_motion_cancelled(
                                            &AnimationPlayer::from_completed_animation(clip),
                                        )
                                    })
                                    .unwrap_or_else(AnimationPlayer::empty);
                                // Seat the posed arm on the tracked hand. Two
                                // halves of one placement, both derived from
                                // this same posed arm so they cannot drift
                                // apart: the grip puts the entity/body origin
                                // on the rendered weapon head, and the
                                // model-space correction cancels that same
                                // offset so the fist still lands in the palm.
                                // The fitted child collider below extends back
                                // over the rendered weapon without moving that
                                // controller-driven body origin.
                                let arm = new_model.skeleton().and_then(|skeleton| {
                                    crate::vr_config::MeleePosedArm::from_joints(
                                        &player.get_transforms(skeleton),
                                    )
                                });
                                if let Some(arm) = arm {
                                    let correction =
                                        crate::vr_config::melee_wield_pose_correction(arm);
                                    new_model.apply_local_transform(correction);
                                    self.world.add_component(
                                        entity_id,
                                        RuntimePropVrGripOffset(
                                            crate::vr_config::melee_contact_offset(arm),
                                        ),
                                    );
                                    if self.interaction.is_holding(entity_id) {
                                        match vr_held_source.as_ref().and_then(|source| {
                                            source.posed_weapon_bounds(&player, correction)
                                        }) {
                                            Some(bounds) => self.physics.fit_held_melee_cuboid(
                                                entity_id,
                                                bounds.size,
                                                bounds.center,
                                            ),
                                            None => tracing::error!(
                                                "ChangeModel: '{model_name}' has no fittable weapon geometry - retaining its loose-prop collider"
                                            ),
                                        }
                                    }
                                } else {
                                    tracing::error!(
                                        "ChangeModel: '{model_name}' has no melee arm joints - wielding it unseated"
                                    );
                                }
                                self.id_to_animation_player.insert(entity_id, player);
                            } else {
                                self.id_to_animation_player
                                    .entry(entity_id)
                                    .or_insert_with(AnimationPlayer::empty);
                            }
                        } else if was_vr_held && !new_model.is_animated() {
                            self.id_to_animation_player.remove(&entity_id);
                        }
                        self.id_to_model.insert(entity_id, new_model);
                        self.world
                            .add_component(entity_id, PropModelName(model_name));

                        self.world.add_component(entity_id, RuntimePropVhots(vhots));
                    }
                }
                Effect::ClearModel { entity_id } => {
                    self.id_to_model.remove(&entity_id);
                    self.id_to_animation_player.remove(&entity_id);
                    self.world
                        .remove::<(PropModelName, RuntimePropVhots)>(entity_id);
                    self.world.remove::<RuntimePropVrGripOffset>(entity_id);
                }
                Effect::SetVhotsFromModel {
                    entity_id,
                    model_name,
                } => {
                    if let Some(model) = self.id_to_model.get(&entity_id) {
                        let xform = model.get_transform();
                        // Same hazard as ChangeModel above: a missing donor
                        // model must not panic the frame.
                        let Some(donor_model) =
                            asset_cache.get_opt(&MODELS_IMPORTER, &format!("{model_name}.BIN"))
                        else {
                            tracing::error!(
                                "SetVhotsFromModel: model '{model_name}.BIN' could not be loaded for entity {entity_id:?} - keeping current vhots"
                            );
                            continue;
                        };
                        let vhots = Model::transform(donor_model.as_ref(), xform).vhots();
                        self.world.add_component(entity_id, RuntimePropVhots(vhots));
                    }
                }
                Effect::PlayEmail { deck, email, force } => {
                    let email_file = get_email_sound_file(deck, email);
                    let mut quests = self.world.borrow::<UniqueViewMut<QuestInfo>>().unwrap();
                    let has_read = quests.has_played_email(&email_file);
                    if !has_read || force {
                        quests.mark_email_as_played(&email_file);
                        let audio_clip =
                            asset_cache.get(&AUDIO_IMPORTER, &format!("{email_file}.wav"));
                        let duration = audio_clip.total_duration();
                        let handle = AudioHandle::new();
                        let preempted = engine::audio::play_audio(
                            audio_context,
                            handle.clone(),
                            Some(AudioChannel::new("email".to_owned())),
                            audio_clip,
                        );
                        // Observability: record the email so the headless e2e
                        // can assert it played (and only once). Anything this
                        // play cut short (the email channel is single-slot) is
                        // marked stopped first, so `still_playing` stays honest.
                        crate::audio_log::record_stops(&preempted);
                        crate::audio_log::record(crate::audio_log::SoundRecord {
                            sample: &email_file,
                            volume_millibels: None,
                            gain: 1.0,
                            pan_millibels: None,
                            pan_applied: false,
                            tags: vec![("kind".to_string(), "email".to_string())],
                            position: [0.0, 0.0, 0.0],
                            duration,
                            source_entity: None,
                            handle: Some(handle.id()),
                        });
                    }
                    drop(quests);
                }
                Effect::PlaySound {
                    handle,
                    name,
                    source,
                    spatial,
                } => {
                    println!("Trying to play sound: {}", &name);
                    let (resolved, has_schema) = resolve_schema(global_context, &name);
                    let maybe_audio_clip = asset_cache
                        .get_opt(&AUDIO_IMPORTER, &format!("{}.wav", resolved.sample_name));

                    if let Some(audio_clip) = maybe_audio_clip {
                        info!("Playing clip: {} handle: {:?}", name, &handle);
                        let duration = audio_clip.total_duration();
                        let handle_id = handle.id();
                        let gain = resolved.linear_gain();
                        // Spatial emitters (TrapSound narrations anchored at
                        // their authored station) play at the source entity,
                        // like the original engine's object sounds. Everything
                        // else - UI feedback, audio logs, cutscene narration -
                        // stays non-spatial at the ears even when it carries a
                        // `source` for attribution.
                        let maybe_position = if spatial {
                            source.and_then(|id| get_entity_position(&self.world, id))
                        } else {
                            None
                        };
                        let preempted = if let Some(position) = maybe_position {
                            engine::audio::play_spatial_audio_with_gain(
                                audio_context,
                                position,
                                source,
                                handle,
                                None,
                                audio_clip,
                                gain,
                            )
                        } else {
                            engine::audio::play_audio_with_settings(
                                audio_context,
                                handle,
                                None,
                                audio_clip,
                                AudioPlaybackSettings::listener_relative(
                                    gain,
                                    resolved.channel_gains(),
                                ),
                            )
                        };
                        // Observability: record scripted one-shot sounds (audio
                        // logs, keypad beeps, ...) so headless tooling can assert
                        // a schema actually resolved and played.
                        let position = maybe_position.unwrap_or_else(|| vec3(0.0, 0.0, 0.0));
                        crate::audio_log::record_stops(&preempted);
                        crate::audio_log::record(crate::audio_log::SoundRecord {
                            sample: &resolved.sample_name,
                            volume_millibels: has_schema.then_some(resolved.volume_millibels),
                            gain,
                            pan_millibels: has_schema.then_some(resolved.pan_millibels),
                            pan_applied: has_schema && maybe_position.is_none(),
                            tags: vec![("kind".to_string(), "sound".to_string())],
                            position: [position.x, position.y, position.z],
                            duration,
                            source_entity: source.map(|id| source_entity(&self.world, id)),
                            handle: Some(handle_id),
                        });
                    } else {
                        warn!("Unable to load clip: {}", name)
                    }
                }
                Effect::PlaySpeech {
                    entity_id,
                    voice_index,
                    concept,
                    tags,
                } => {
                    if let Some(resolved) = resolve_speech_sample(
                        &global_context.gamesys,
                        voice_index,
                        concept.as_str(),
                        &tags,
                    ) {
                        let audio_path = format!("{}.wav", resolved.sample_name);
                        if let Some(audio_clip) = asset_cache.get_opt(&AUDIO_IMPORTER, &audio_path)
                        {
                            let handle = AudioHandle::new();
                            let duration = audio_clip.total_duration();
                            let handle_id = handle.id();
                            let gain = resolved.linear_gain();
                            let maybe_position = get_entity_position(&self.world, entity_id);
                            let preempted = if let Some(position) = maybe_position {
                                engine::audio::play_spatial_audio_with_gain(
                                    audio_context,
                                    position,
                                    Some(entity_id),
                                    handle,
                                    None,
                                    audio_clip,
                                    gain,
                                )
                            } else {
                                engine::audio::play_audio_with_settings(
                                    audio_context,
                                    handle,
                                    None,
                                    audio_clip,
                                    AudioPlaybackSettings::listener_relative(
                                        gain,
                                        resolved.channel_gains(),
                                    ),
                                )
                            };
                            crate::audio_log::record_stops(&preempted);
                            // Observability: speech is the loudest source of
                            // overlapping audio, so trace it like the rest.
                            let position = maybe_position.unwrap_or_else(|| vec3(0.0, 0.0, 0.0));
                            let mut sound_tags = vec![
                                ("kind".to_string(), "speech".to_string()),
                                ("concept".to_string(), concept.clone()),
                            ];
                            sound_tags.extend(tags.iter().cloned());
                            crate::audio_log::record(crate::audio_log::SoundRecord {
                                sample: &resolved.sample_name,
                                volume_millibels: Some(resolved.volume_millibels),
                                gain,
                                pan_millibels: Some(resolved.pan_millibels),
                                pan_applied: maybe_position.is_none(),
                                tags: sound_tags,
                                position: [position.x, position.y, position.z],
                                duration,
                                source_entity: Some(source_entity(&self.world, entity_id)),
                                handle: Some(handle_id),
                            });
                        } else {
                            warn!(
                                "Unable to load speech clip '{}' for concept '{}'",
                                audio_path, concept
                            );
                        }
                    } else {
                        warn!(
                            "Failed to resolve speech for voice {} concept '{}'",
                            voice_index, concept
                        );
                    }
                }
                Effect::PlayEnvironmentalSound {
                    query,
                    position,
                    audio_handle,
                } => {
                    play_environmental_sound(
                        &global_context.gamesys,
                        asset_cache,
                        audio_context,
                        query,
                        audio_handle,
                        position,
                    );
                }
                Effect::SlayEntity { entity_id } => {
                    let did_slay = self.slay_entity(entity_id, asset_cache);

                    if did_slay {
                        let maybe_env_sound_query =
                            get_environmental_sound_query(&self.world, entity_id, "death", vec![]);

                        if let (Some(handle), Some(env_sound_query)) =
                            (self.id_to_physics.get(&entity_id), maybe_env_sound_query)
                        {
                            let position = self.physics.get_position(*handle).unwrap();

                            play_environmental_sound(
                                &global_context.gamesys,
                                asset_cache,
                                audio_context,
                                env_sound_query,
                                AudioHandle::new(),
                                position,
                            )
                        }

                        // With the `ragdoll` experimental flag, replace the slain
                        // creature with a physics ragdoll of its death pose (the
                        // spawn removes the creature itself). The reduced-coordinate
                        // multibody rig is the default; `ragdoll_impulse` falls back
                        // to the legacy impulse-joint rig. Without the flag (or for
                        // non-ragdoll-able entities), fall back to the plain removal.
                        // An entity replaced by its own Corpse/Flinderize links
                        // (a droid's explosion and parts, an Overlord's gibs)
                        // never ragdolls: the spawned links are the body, and a
                        // ragdoll on top would leave a corpse the explosion was
                        // supposed to consume.
                        let spawned_ragdoll = !has_death_links(&self.world, entity_id)
                            && game_options.experimental_features.contains("ragdoll")
                            && self
                                .spawn_ragdoll(
                                    entity_id,
                                    !game_options
                                        .experimental_features
                                        .contains("ragdoll_impulse"),
                                    // Slain mid-animation, usually upright -
                                    // not a crumpled pose.
                                    false,
                                )
                                .is_some();
                        if !spawned_ragdoll {
                            self.remove_entity(entity_id);
                        }
                    }
                }
                Effect::SpawnCorpseRagdoll { entity_id, impact } => {
                    // Death-crumple handoff (AI deaths): shortly after the
                    // animation starts, replace the animated corpse with a
                    // physics ragdoll seeded from its current pose. Without
                    // the experimental flag, keep the animated corpse but
                    // narrow its capsule to world/selectable collision: it
                    // remains grounded and lootable without blocking the
                    // player.
                    // (Known experimental limitations: the ragdoll corpse is
                    // not serialized, so it vanishes on save/load; and removing
                    // the creature entity also removes its creaturecontainer
                    // loot target.)
                    let mut spawned_ragdoll = false;
                    if game_options.experimental_features.contains("ragdoll") {
                        let ragdoll_id = self.spawn_ragdoll(
                            entity_id,
                            !game_options
                                .experimental_features
                                .contains("ragdoll_impulse"),
                            // Finished death crumple: floor-lying and limb-
                            // overlapping.
                            true,
                        );
                        spawned_ragdoll = ragdoll_id.is_some();
                        // Seed the corpse with the killing blow: the struck
                        // limb gets a shove along the shot's direction, so the
                        // corpse reacts to HOW it died instead of collapsing
                        // in place.
                        if let (Some(ragdoll_id), Some(impact)) = (ragdoll_id, impact) {
                            self.rag_doll_manager.apply_impact(
                                ragdoll_id,
                                &impact,
                                &mut self.physics,
                            );
                        }
                    }
                    // A successful ragdoll spawn removes the original creature
                    // and its capsule. In the normal path (or when a model
                    // cannot make a ragdoll), retain the corpse entity but
                    // exclude its capsule from player/entity collision.
                    if !spawned_ragdoll {
                        self.physics
                            .set_collision_group(entity_id, CollisionGroup::corpse());
                    }
                }
                Effect::StopSound { handle } => {
                    // Observability: mark the matching play as stopped so
                    // `still_playing` in the audio log stops reporting it.
                    crate::audio_log::record_stop(handle.id());
                    engine::audio::stop_audio(audio_context, handle);
                }
                Effect::DestroyEntity { entity_id } => {
                    info!("!!!Destroying entity: {:?}", entity_id);
                    self.destroy_entity(entity_id);
                }
                Effect::ResetGravity { entity_id } => {
                    self.physics.set_gravity(entity_id, 1.0);
                }
                Effect::SetGravity {
                    entity_id,
                    gravity_percent,
                } => {
                    self.physics.set_gravity(entity_id, gravity_percent);
                }
                Effect::SetPlayerPosition {
                    position,
                    is_teleport,
                    source,
                } => {
                    self.physics
                        .set_player_translation(position, &mut self.player_handle);
                    if is_teleport {
                        self.world
                            .add_component(player_entity, PropTeleported::with_source(source))
                    }
                }
                Effect::SetPlayerRotation { rotation } => {
                    self.world
                        .borrow::<UniqueViewMut<PlayerInfo>>()
                        .unwrap()
                        .rotation = rotation;
                }
                Effect::SetPlayerControlsEnabled { enabled } => {
                    self.player_controls_enabled = enabled;
                }
                Effect::SetScreenFade { alpha } => {
                    self.screen_fade_alpha = alpha.clamp(0.0, 1.0);
                }
                Effect::ClearTeleportedMarker { entity_id } => {
                    self.world.run(
                        |mut v_teleported: ViewMut<dark::properties::PropTeleported>| {
                            v_teleported.remove(entity_id);
                        },
                    );
                }
                Effect::SetRenderAlpha { entity_id, alpha } => {
                    self.world.add_component(
                        entity_id,
                        dark::properties::PropRenderAlpha(alpha.clamp(0.0, 1.0)),
                    );
                }
                Effect::SetVisibility { entity_id, visible } => {
                    let is_alive = self
                        .world
                        .borrow::<shipyard::EntitiesView>()
                        .map(|entities| entities.is_alive(entity_id))
                        .unwrap_or(false);
                    if is_alive {
                        self.world.add_component(entity_id, PropHasRefs(visible));
                    }
                }
                Effect::SetObjectState { entity_id, state } => {
                    let is_alive = self
                        .world
                        .borrow::<shipyard::EntitiesView>()
                        .map(|entities| entities.is_alive(entity_id))
                        .unwrap_or(false);
                    if is_alive {
                        self.world.add_component(entity_id, PropObjState(state));
                    }
                }
                Effect::SetAnimatedLight {
                    entity_id,
                    intensity,
                    inactive,
                } => {
                    let light_number =
                        self.world
                            .run(|mut lights: ViewMut<PropAnimLight>| -> Option<i16> {
                                let light = (&mut lights).get(entity_id).ok()?;
                                light.inactive = inactive;
                                Some(light.light_number)
                            });
                    if let (Some(light_number), Some(controller)) =
                        (light_number, &mut self.animated_lightmaps)
                    {
                        controller.set_light_intensity(light_number, intensity);
                    }
                }
                Effect::SetReplicatorHackedContents {
                    entity_id,
                    contents,
                } => {
                    let is_alive = self
                        .world
                        .borrow::<shipyard::EntitiesView>()
                        .map(|entities| entities.is_alive(entity_id))
                        .unwrap_or(false);
                    if is_alive {
                        self.world.add_component(entity_id, contents);
                    }
                }
                Effect::SetEcologyState { entity_id, state } => {
                    let is_alive = self
                        .world
                        .borrow::<shipyard::EntitiesView>()
                        .map(|entities| entities.is_alive(entity_id))
                        .unwrap_or(false);
                    if is_alive {
                        self.world
                            .add_component(entity_id, dark::properties::PropEcoState(state));
                    }
                }
                Effect::SetLocked { entity_id, locked } => {
                    crate::scripts::script_util::set_entity_locked(
                        &mut self.world,
                        entity_id,
                        locked,
                    );
                }
                Effect::SetTranslatingDoorState {
                    entity_id,
                    state,
                    base_location,
                } => {
                    let mut v_trans_door = self
                        .world
                        .borrow::<ViewMut<dark::properties::PropTranslatingDoor>>()
                        .unwrap();
                    if let Ok(door) = (&mut v_trans_door).get(entity_id) {
                        door.state = state;
                        door.base_location = base_location;
                    }
                }
                Effect::SetQuestBit {
                    quest_bit_name,
                    quest_bit_value,
                } => {
                    let mut quests = self.world.borrow::<UniqueViewMut<QuestInfo>>().unwrap();
                    quests.set_quest_bit_value(&quest_bit_name, quest_bit_value);
                    drop(quests);

                    let quests_new = self.world.borrow::<UniqueView<QuestInfo>>().unwrap();
                    info!(
                        "Updated quest info for {}({:?}): {:?}",
                        quest_bit_name, quest_bit_value, quests_new
                    );
                }

                Effect::GrantTourReward { career, year, tour } => {
                    let mut quests = self.world.borrow::<UniqueViewMut<QuestInfo>>().unwrap();
                    let old_width = crate::inventory::backpack_width(quests.player_stats());
                    let applied = quests
                        .player_stats_mut()
                        .apply_tour_reward(career, year, tour);
                    let new_width = crate::inventory::backpack_width(quests.player_stats());
                    if applied {
                        info!(
                            "Applied training-tour reward ({:?} year {} tour {}): {:?}",
                            career,
                            year,
                            tour,
                            quests.player_stats()
                        );
                    }
                    drop(quests);
                    if applied {
                        self.resize_player_backpack(old_width, new_width);
                    }
                }

                Effect::TrainerPurchase { target } => {
                    // Authoritative validation + mutation (the panel only
                    // pre-validates for feedback): re-quote from the cost
                    // tables, spend atomically, then raise the target.
                    use crate::scripts::gui::{apply_purchase, upgrade_quote};
                    let endurance_target = matches!(
                        target,
                        crate::scripts::gui::TrainerTarget::Stat(
                            crate::player_stats::Stat::Endurance
                        )
                    );
                    if endurance_target
                        && self
                            .world
                            .borrow::<View<dark::properties::PropMaxHitPoints>>()
                            .map_or(true, |maximums| maximums.get(player_entity).is_err())
                    {
                        warn!(
                            "Trainer purchase {:?} refused: player has no maximum-HP pool",
                            target
                        );
                        continue;
                    }
                    let costs = self
                        .world
                        .borrow::<UniqueView<GlobalTrainerCosts>>()
                        .unwrap()
                        .0
                        .clone();
                    if let Some(costs) = costs {
                        let (purchased, old_width, new_width) = {
                            let mut quests =
                                self.world.borrow::<UniqueViewMut<QuestInfo>>().unwrap();
                            let stats = quests.player_stats_mut();
                            let old_width = crate::inventory::backpack_width(stats);
                            match upgrade_quote(&costs, stats, target) {
                                Some(cost) if stats.spend_cyber_modules(cost) => {
                                    apply_purchase(stats, target);
                                    info!(
                                        "Trainer purchase {:?} (-{} modules, balance {})",
                                        target, cost, stats.cyber_modules
                                    );
                                    (true, old_width, crate::inventory::backpack_width(stats))
                                }
                                Some(cost) => {
                                    info!(
                                        "Trainer purchase {:?} refused: costs {}, balance {}",
                                        target, cost, stats.cyber_modules
                                    );
                                    (false, old_width, old_width)
                                }
                                None => {
                                    info!(
                                        "Trainer purchase {:?} refused: maxed/locked/unavailable",
                                        target
                                    );
                                    (false, old_width, old_width)
                                }
                            }
                        };
                        if purchased {
                            self.resize_player_backpack(old_width, new_width);
                        }
                        if purchased && endurance_target {
                            increase_player_max_hit_points(
                                &self.world,
                                player_entity,
                                crate::scripts::gui::ENDURANCE_HP_PER_LEVEL,
                            );
                        }
                    } else {
                        warn!("TrainerPurchase dropped: gamesys has no cost tables");
                    }
                }

                Effect::ReplicatorPurchase {
                    cost,
                    template_name,
                    position,
                    orientation,
                } => {
                    use crate::scripts::script_util::debit_player_nanites;

                    let template_exists = self
                        .template_name_to_template_id
                        .contains_key(&template_name.to_ascii_lowercase());
                    if cost <= 0 || !template_exists {
                        info!(
                            "Replicator purchase refused: invalid request for {} at cost {}",
                            template_name, cost
                        );
                        effects.push_front(Effect::PlaySound {
                            handle: AudioHandle::new(),
                            source: None,
                            name: "repfail".to_owned(),
                            spatial: false,
                        });
                    } else if let Some(exhausted) = debit_player_nanites(&self.world, cost) {
                        for entity_id in exhausted {
                            self.destroy_entity(entity_id);
                        }
                        let created = self.create_entity_by_template_name(
                            asset_cache,
                            &template_name,
                            position,
                            orientation,
                        );
                        debug_assert!(
                            created.is_some(),
                            "prevalidated replicator template disappeared"
                        );
                        effects.push_front(Effect::PlaySound {
                            handle: AudioHandle::new(),
                            source: None,
                            name: "replic2e".to_owned(),
                            spatial: false,
                        });
                    } else {
                        info!(
                            "Replicator purchase refused: {} costs {} nanites",
                            template_name, cost
                        );
                        effects.push_front(Effect::PlaySound {
                            handle: AudioHandle::new(),
                            source: None,
                            name: "repfail".to_owned(),
                            spatial: false,
                        });
                    }
                }

                Effect::AcquireOsTrait { trait_id, machine } => {
                    use crate::scripts::gui::{
                        NATURALLY_ABLE_MODULES, TANK_HP_BONUS, TRAIT_NATURALLY_ABLE, TRAIT_TANK,
                        live_effect_note, trait_name, used_bit_name,
                    };
                    // The machine's stable mission object id keys its used bit.
                    let machine_template_id = self
                        .world
                        .borrow::<View<dark::properties::PropTemplateId>>()
                        .ok()
                        .and_then(|v| v.get(machine).ok().map(|t| t.template_id));
                    // Without a stable machine id the used state could never
                    // be recorded - refuse rather than vend repeatably.
                    let Some(machine_id) = machine_template_id else {
                        warn!("O/S trait {} refused: machine has no template id", trait_id);
                        continue;
                    };
                    let (acquired, old_width, new_width) = {
                        let mut quests = self.world.borrow::<UniqueViewMut<QuestInfo>>().unwrap();
                        let old_width = crate::inventory::backpack_width(quests.player_stats());
                        let used = quests
                            .read_quest_bit_value(&used_bit_name(machine_id))
                            .bits()
                            != 0;
                        if used {
                            info!("O/S trait {} refused: machine already used", trait_id);
                            (false, old_width, old_width)
                        } else if !quests.player_stats_mut().add_os_trait(trait_id) {
                            info!("O/S trait {} refused: owned or slots full", trait_id);
                            (false, old_width, old_width)
                        } else {
                            quests.set_quest_bit_value(
                                &used_bit_name(machine_id),
                                dark::properties::QuestBitValue::COMPLETE,
                            );
                            if trait_id == TRAIT_NATURALLY_ABLE {
                                quests
                                    .player_stats_mut()
                                    .award_cyber_modules(NATURALLY_ABLE_MODULES);
                            }
                            let new_width = crate::inventory::backpack_width(quests.player_stats());
                            (true, old_width, new_width)
                        }
                    };

                    if acquired {
                        self.resize_player_backpack(old_width, new_width);
                        if trait_id == TRAIT_TANK {
                            // Tank (Trait8): the original raises the ceiling
                            // AND current HP by the bonus. Loads first re-derive
                            // the trait-adjusted default, so this live grant
                            // cannot double-apply.
                            let player_entity = self
                                .world
                                .borrow::<UniqueView<PlayerInfo>>()
                                .unwrap()
                                .entity_id;
                            self.world.run(
                                |mut v_hp: ViewMut<dark::properties::PropHitPoints>,
                                 mut v_max: ViewMut<dark::properties::PropMaxHitPoints>| {
                                    let new_max = (&mut v_max)
                                        .get(player_entity)
                                        .map(|max| {
                                            max.hit_points += TANK_HP_BONUS as u32;
                                            max.hit_points
                                        })
                                        .ok();
                                    if let Ok(hp) = (&mut v_hp).get(player_entity) {
                                        hp.hit_points += TANK_HP_BONUS;
                                        if let Some(new_max) = new_max {
                                            hp.hit_points = hp.hit_points.min(new_max as i32);
                                        }
                                    }
                                },
                            );
                        }
                        info!(
                            "O/S trait acquired: {} ({}){}",
                            trait_id,
                            trait_name(trait_id),
                            live_effect_note(trait_id)
                                .map(|n| format!(" - {}", n))
                                .unwrap_or_default()
                        );
                    }
                }

                Effect::SetAIProperty { entity_id, update } => {
                    // The AI may have been destroyed earlier in this same
                    // effect batch (e.g. a scripted sequence whose final
                    // action frobs a slay trap, while the AI also published
                    // a behavior change this frame); adding a component to a
                    // dead entity panics.
                    let is_alive = self
                        .world
                        .borrow::<shipyard::EntitiesView>()
                        .map(|entities| entities.is_alive(entity_id))
                        .unwrap_or(false);
                    if is_alive {
                        match update {
                            AIPropertyUpdate::Alertness { level, peak } => {
                                self.world
                                    .add_component(entity_id, PropAIAlertness { level, peak });
                            }
                            AIPropertyUpdate::Mode { mode } => {
                                self.world.add_component(entity_id, PropAIMode { mode });
                            }
                            AIPropertyUpdate::Behavior { name } => {
                                self.world
                                    .add_component(entity_id, RuntimePropAIBehavior(name));
                            }
                            AIPropertyUpdate::LocomotionScale { scale } => {
                                self.world.add_component(
                                    entity_id,
                                    crate::runtime_props::RuntimePropLocomotionScale(scale),
                                );
                            }
                            AIPropertyUpdate::TargetAwareness {
                                last_known_pos,
                                has_line_of_sight,
                            } => {
                                self.world.add_component(
                                    entity_id,
                                    crate::runtime_props::RuntimePropAITargetAwareness {
                                        last_known_pos,
                                        has_line_of_sight,
                                    },
                                );
                            }
                            AIPropertyUpdate::ClearTargetAwareness => {
                                self.world.run(
                                    |mut v_awareness: ViewMut<
                                        crate::runtime_props::RuntimePropAITargetAwareness,
                                    >| {
                                        v_awareness.remove(entity_id);
                                    },
                                );
                            }
                            AIPropertyUpdate::PatrolEnabled { enabled } => {
                                self.world.add_component(
                                    entity_id,
                                    dark::properties::PropAIPatrol(enabled),
                                );
                            }
                        }
                    }
                }
                Effect::SetAICurrentPatrol { entity_id, target } => {
                    let is_alive = self
                        .world
                        .borrow::<shipyard::EntitiesView>()
                        .map(|entities| entities.is_alive(entity_id))
                        .unwrap_or(false);
                    if !is_alive {
                        continue;
                    }
                    let target_template_id = target.and_then(|target| {
                        self.world
                            .borrow::<View<dark::properties::PropTemplateId>>()
                            .ok()
                            .and_then(|templates| {
                                templates.get(target).ok().map(|value| value.template_id)
                            })
                    });
                    let mut needs_links = false;
                    self.world.run(|mut v_links: ViewMut<Links>| {
                        if let Ok(links) = (&mut v_links).get(entity_id) {
                            links
                                .to_links
                                .retain(|link| link.link != Link::AICurrentPatrol);
                            if let Some(target) = target {
                                links.to_links.push(ToLink {
                                    to_template_id: target_template_id.unwrap_or(0),
                                    to_entity_id: Some(WrappedEntityId(target)),
                                    link: Link::AICurrentPatrol,
                                });
                            }
                        } else {
                            needs_links = target.is_some();
                        }
                    });
                    if let Some(target) = target.filter(|_| needs_links) {
                        self.world.add_component(
                            entity_id,
                            Links {
                                to_links: vec![ToLink {
                                    to_template_id: target_template_id.unwrap_or(0),
                                    to_entity_id: Some(WrappedEntityId(target)),
                                    link: Link::AICurrentPatrol,
                                }],
                            },
                        );
                    }
                }
                Effect::SetAllAIAlertness { level, pin } => {
                    let creature_ids: Vec<EntityId> = {
                        let v_creature = self.world.borrow::<View<PropCreature>>().unwrap();
                        v_creature.iter().ids().collect()
                    };
                    for id in creature_ids {
                        self.script_world.dispatch(Message {
                            to: id,
                            payload: MessagePayload::SetAlertness { level, pin },
                        });
                    }
                }
                Effect::SetPositionRotation {
                    entity_id,
                    rotation,
                    position,
                } => {
                    // TODO: plumb scale through
                    self.set_entity_position_rotation(
                        entity_id,
                        position,
                        rotation,
                        vec3(1.0, 1.0, 1.0),
                    );
                }
                Effect::SetPosition {
                    entity_id,
                    position,
                } => {
                    self.id_to_model.entry(entity_id).and_modify(|model| {
                        let mut xform = model.get_transform();
                        xform.w.x = position.x;
                        xform.w.y = position.y;
                        xform.w.z = position.z;
                        *model = Model::transform(model, xform);
                    });

                    if let Some(rigid_body_handle) = self.id_to_physics.get(&entity_id) {
                        self.physics.set_translation(*rigid_body_handle, position);
                    };
                }
                Effect::SetRotation {
                    entity_id,
                    rotation,
                } => {
                    if let Some(rigid_body_handle) = self.id_to_physics.get(&entity_id) {
                        self.physics.set_rotation(*rigid_body_handle, rotation);
                    };
                }
                Effect::PositionInventory { position, rotation } => {
                    PlayerInventoryEntity::set_position_rotation(
                        &mut self.world,
                        position,
                        rotation,
                    )
                }
                Effect::SpawnInFrontOfPlayer {
                    template_id,
                    head_rotation,
                    auto_wield,
                } => {
                    let (pos, rot) = {
                        let player = self.world.borrow::<UniqueView<PlayerInfo>>().unwrap();
                        (vec3_to_point3(player.pos), player.rotation * head_rotation)
                    };
                    let forward = rot * vec3(0.0, 2.5 / SCALE_FACTOR, -10.0 / SCALE_FACTOR);
                    let target = pos + forward;
                    // Drop-to-floor: the naive forward offset can land inside or
                    // below the level geometry (e.g. on pitched-down aim), which
                    // drops the spawn out of the world. Cast straight down onto
                    // the floor at the target's XZ and rest the monster there.
                    // The ray starts a fixed height above the PLAYER's floor
                    // (`pos.y`), not above the naive target - a pitched target
                    // can land below the floor, and a ray starting there would
                    // miss the floor above it. If no floor is found within range,
                    // fall back to the player's own position (known-good ground)
                    // rather than the naive target - the naive point is exactly
                    // the below-floor case this fixes, so it must not be the
                    // fallback.
                    let ray_start = Point3::new(target.x, pos.y + 2.0, target.z);
                    let spawn_pos = match self.physics.ray_cast2(
                        ray_start,
                        vec3(0.0, -1.0, 0.0),
                        20.0,
                        crate::physics::InternalCollisionGroups::WORLD,
                        None,
                        true,
                    ) {
                        Some(hit) => {
                            Point3::new(hit.hit_point.x, hit.hit_point.y + 0.1, hit.hit_point.z)
                        }
                        None => pos,
                    };
                    let info = self.create_entity_with_position(
                        asset_cache,
                        template_id,
                        spawn_pos,
                        rot,
                        Matrix4::identity(),
                        CreateEntityOptions::default(),
                    );
                    // Flat presentation: auto-wield the spawned weapon as the
                    // first-person viewmodel for debug testing, but only when not
                    // already armed - extra spawns fall to the ground as world
                    // pickups (world model + physics) to be picked up.
                    if auto_wield
                        && game_options.presentation_mode == crate::PresentationMode::Flat
                        && !self.interaction.is_wielding()
                    {
                        let msgs = self.interaction.wield(info.entity_id);
                        self.process_virtual_hand_effects(asset_cache, msgs);
                    }
                }
                Effect::DebugCycleWeapon { head_rotation } => {
                    // The SS2 player-weapon roster (templates with PropPlayerGun),
                    // cycled for flat-mode aim/viewmodel testing.
                    const DEBUG_WEAPONS: &[i32] = &[
                        -17,  // Pistol
                        -18,  // Assault Rifle
                        -19,  // Shotgun
                        -22,  // Laser Pistol
                        -23,  // EMP Rifle
                        -21,  // Gren Launcher
                        -25,  // Stasis Field Generator
                        -26,  // Fusion Cannon
                        -27,  // Worm Launcher
                        -29,  // Viral Prolif
                        -247, // Psi Amp
                        // Melee weapons (PropLimbModel, no PropPlayerGun):
                        -928, // Wrench
                        -24,  // Electro Shock (rapier)
                        -28,  // Crystal Shard
                        -2291, // PsiSword
                              // NB: Hybrid Shotgun (-4073) is omitted - it has an
                              // unimplemented `trashedshotgun` script that panics on
                              // creation (scripts/mod.rs). It is an enemy weapon
                              // variant, not part of the player arsenal.
                    ];
                    let template_id = DEBUG_WEAPONS[self.debug_weapon_index % DEBUG_WEAPONS.len()];
                    self.debug_weapon_index = self.debug_weapon_index.wrapping_add(1);

                    let (pos, rot) = {
                        let player = self.world.borrow::<UniqueView<PlayerInfo>>().unwrap();
                        (vec3_to_point3(player.pos), player.rotation * head_rotation)
                    };
                    let forward = rot * vec3(0.0, 2.5 / SCALE_FACTOR, -10.0 / SCALE_FACTOR);
                    let info = self.create_entity_with_position(
                        asset_cache,
                        template_id,
                        pos + forward,
                        rot,
                        Matrix4::identity(),
                        CreateEntityOptions::default(),
                    );
                    // Force-wield (unlike SpawnDebugItem): `wield` holsters the
                    // previously held weapon into the backpack, so each cycle
                    // swaps the viewmodel. No-op in VR (wield returns nothing).
                    let msgs = self.interaction.wield(info.entity_id);
                    self.process_virtual_hand_effects(asset_cache, msgs);
                }
                Effect::TurnOffTweqs { entity_id } => {
                    self.world.run_with_data(turn_off_tweqs, entity_id);
                }
                Effect::TurnOnTweqs { entity_id } => {
                    self.world.run_with_data(turn_on_tweqs, entity_id);
                }
                Effect::PathfindingTest => {
                    let result = self.pathfinding_test_action("cycle");
                    info!("Pathfinding test: {}", result);
                }
                Effect::GlobalEffect(global_effect) => global_effects.push(global_effect),
                _ => {
                    game_log!(WARN, "Unhandled effect: {effect:?}");
                }
            }
        }

        if let Some(controller) = &mut self.animated_lightmaps {
            controller.flush();
        }

        global_effects
    }

    /// Advance the flat melee swing animation. Only an active swing uses the
    /// persistent player; the idle is the static head-up pose (rendered directly,
    /// since the looping idle clip carries root motion meant to be cancelled by
    /// the original engine's camSynch, which we don't implement). When the swing
    /// completes - or the weapon changes - we drop the player and fall back to
    /// the static idle.
    fn update_flat_melee_anim(&mut self, dt: std::time::Duration) {
        let Some((entity, player)) = self.flat_melee_anim.take() else {
            return;
        };
        if self.interaction.viewmodel_entity() != Some(entity) {
            return; // weapon changed; leave None -> static idle
        }
        let (next, flags, events, _disp) = AnimationPlayer::update(&player, dt);
        if !flags.is_empty() {
            self.script_world.dispatch(Message {
                to: entity,
                payload: MessagePayload::AnimationFlagTriggered {
                    motion_flags: flags,
                },
            });
        }
        let completed = events
            .iter()
            .any(|e| matches!(e, AnimationEvent::Completed));
        if !completed {
            self.flat_melee_anim = Some((entity, next));
        }
    }

    /// Start a one-shot swing on the flat melee viewmodel (it plays once, then
    /// `update_flat_melee_anim` drops it back to the static idle). Driven by
    /// `Effect::FlatMeleeSwing` on a melee attack.
    fn queue_flat_melee_swing(&mut self, asset_cache: &mut AssetCache, entity_id: EntityId) {
        // Do not let release/pull chatter restart the clip before its authored
        // hit frame (or manufacture extra hits). A new swing is accepted only
        // after the current one returns to idle.
        if self.flat_melee_anim.is_some() {
            return;
        }
        if let Some(clip) =
            asset_cache.get_opt(&ANIMATION_CLIP_IMPORTER, &format!("{MELEE_SWING_CLIP}_.mc"))
        {
            let player = AnimationPlayer::queue_animation(&AnimationPlayer::empty(), clip);
            self.flat_melee_anim = Some((entity_id, player));
        }
    }

    /// Begin a reload on `weapon`: consume compatible backpack reserve to refill
    /// its clip and start the reload animation (a `RuntimePropReloading` on the
    /// weapon). SS2's
    /// first-person reload tilts the gun down to a peak angle, holds while the
    /// clip is swapped, then raises it back up; the peak angle and pitch speed
    /// come from the weapon's own data (`PropPlayerGun`'s reload pitch/rate, as
    /// 16-bit angle units where 65536 = 360 deg) and the hold from its reload
    /// time (`PropBaseGunDesc.reload_time_ms`). No-op for non-guns (no
    /// `PropBaseGunDesc`), while a reload is already in progress, or when no
    /// compatible reserve rounds are available.
    fn begin_reload(&mut self, weapon: EntityId) {
        // Sensible fallbacks used only when a gun has no PropPlayerGun at all, so
        // the reload is still visible.
        const FALLBACK_PEAK_DEG: f32 = -45.0;
        const FALLBACK_RATE_DEG_PER_S: f32 = 180.0;
        // 16-bit angle units (65536 = 360 deg). The tilt target is a signed
        // shortest-path angle (e.g. the pistol's -67.5 deg); the rate is an
        // unsigned magnitude (deg/sec), so it must NOT be read as signed.
        fn ang16_signed_deg(a: u16) -> f32 {
            (a as i16 as f32) / 65536.0 * 360.0
        }
        fn ang16_unsigned_deg(a: u16) -> f32 {
            (a as f32) / 65536.0 * 360.0
        }

        // Only guns reload; bail (and don't restart an in-progress reload).
        let (clip, hold) = {
            let v_desc = self
                .world
                .borrow::<View<dark::properties::PropBaseGunDesc>>();
            match v_desc.as_ref().ok().and_then(|v| v.get(weapon).ok()) {
                Some(d) => (d.clip, d.reload_time_ms as f32 / 1000.0),
                None => return,
            }
        };
        if let Ok(v) = self.world.borrow::<View<RuntimePropReloading>>() {
            if v.get(weapon).is_ok_and(|r| !r.is_done()) {
                return;
            }
        }

        // Use the weapon's own pitch/rate when it has a PropPlayerGun (even a
        // near-zero pitch is honored - some psi weapons intentionally barely tilt
        // - so we only fall back when the data is genuinely absent).
        let (peak_deg, rate) = {
            let v_gun = self.world.borrow::<View<PropPlayerGun>>();
            match v_gun.as_ref().ok().and_then(|v| v.get(weapon).ok()) {
                Some(g) => (
                    ang16_signed_deg(g.reload_pitch),
                    ang16_unsigned_deg(g.reload_rate),
                ),
                None => (FALLBACK_PEAK_DEG, FALLBACK_RATE_DEG_PER_S),
            }
        };
        // Guard the divisor only (a zero rate would blow up `leg`).
        let rate = if rate < 1.0 {
            FALLBACK_RATE_DEG_PER_S
        } else {
            rate
        };
        let leg = peak_deg.abs() / rate; // tilt-down (and tilt-up) duration

        // Consume the selected projectile's authored Clip type from the
        // backpack. Multiple small stacks may contribute to one magazine.
        let reload = crate::mission::reload::load_from_reserve(&self.world, weapon, clip);
        if reload.rounds_loaded == 0 {
            return;
        }
        for item in reload.depleted_items {
            self.interaction.on_entity_destroyed(item);
            self.flat_ui.on_entity_destroyed(item);
            self.remove_incoming_contains_links(item);
            self.remove_entity(item);
        }

        self.world.add_component(
            weapon,
            RuntimePropReloading {
                elapsed: 0.0,
                down: leg,
                hold,
                up: leg,
                peak_deg,
            },
        );
    }

    /// Cycle `weapon` to its next ammo type (next `Projectile` link). No-op when
    /// the weapon has fewer than two projectile links or still has loaded
    /// rounds; until magazine-unload semantics exist, requiring an empty gun
    /// prevents standard rounds from turning into AP/HE rounds for free.
    fn cycle_ammo(&mut self, weapon: EntityId) {
        if !crate::scripts::script_util::can_cycle_ammo(&self.world, weapon) {
            return;
        }
        let count =
            crate::scripts::script_util::ordered_projectile_links(&self.world, weapon).len();
        let current = self
            .world
            .borrow::<View<RuntimePropSelectedAmmo>>()
            .ok()
            .and_then(|v| v.get(weapon).ok().map(|s| s.0))
            .unwrap_or(0);
        self.world
            .add_component(weapon, RuntimePropSelectedAmmo((current + 1) % count));
    }

    /// Advance any in-progress reload by `dt` and clear it when complete.
    fn update_flat_reload_anim(&self, dt: std::time::Duration) {
        let dt_s = dt.as_secs_f32();
        let mut done = Vec::new();
        {
            let mut v = self
                .world
                .borrow::<ViewMut<RuntimePropReloading>>()
                .unwrap();
            for (id, r) in (&mut v).iter().with_id() {
                r.elapsed += dt_s;
                if r.is_done() {
                    done.push(id);
                }
            }
        }
        if !done.is_empty() {
            let mut v = self
                .world
                .borrow::<ViewMut<RuntimePropReloading>>()
                .unwrap();
            for id in done {
                v.remove(id);
            }
        }
    }

    pub fn render_per_eye(
        &mut self,
        asset_cache: &mut AssetCache,
        view: Matrix4<f32>,
        projection: Matrix4<f32>,
        screen_size: Vector2<f32>,
        options: &crate::GameOptions,
    ) -> Vec<SceneObject> {
        let mut ret = vec![];
        for hit_entity in self.interaction.highlighted_entities() {
            ret.extend(draw_item_outline(
                asset_cache,
                &self.physics,
                hit_entity,
                view,
                projection,
                screen_size,
            ));

            ret.extend(draw_item_name(
                asset_cache,
                &self.physics,
                hit_entity,
                &self.world,
                view,
                projection,
                screen_size,
                options.debug_show_ids,
            ));
        }

        ret.extend(self.visibility_engine.debug_render(asset_cache));

        // Render debug skeletons with joint ID text overlays
        if options.debug_skeletons {
            let v_transform = self.world.borrow::<View<RuntimePropTransform>>().unwrap();
            let v_joint_transforms = self
                .world
                .borrow::<View<RuntimePropJointTransforms>>()
                .unwrap();

            for (entity_id, objs) in &self.id_to_model {
                if !has_refs(&self.world, *entity_id) {
                    continue;
                }

                if !self.visibility_engine.is_visible(*entity_id) {
                    continue;
                }

                if let Ok(xform) = v_transform.get(*entity_id).map(|p| p.0) {
                    if let Ok(joint_transforms) = v_joint_transforms.get(*entity_id) {
                        let world_joints: Vec<Matrix4<f32>> = joint_transforms
                            .0
                            .iter()
                            .map(|joint| xform * *joint)
                            .collect();
                        let mut debug_skeleton_with_text = objs.draw_debug_skeleton_with_text(
                            &world_joints,
                            asset_cache,
                            view,
                            projection,
                            screen_size,
                        );
                        ret.append(&mut debug_skeleton_with_text);
                    }
                }
            }
        }

        if options.presentation_mode == crate::PresentationMode::Flat {
            // First-person weapon viewmodel: draw the wielded weapon's model on
            // top of the world. It is skipped in the world pass and drawn here,
            // last; the depth buffer is cleared before its first object so it
            // renders over geometry while still depth-testing within itself.
            if let Some(weapon) = self.interaction.viewmodel_entity() {
                let maybe_xform = {
                    let v_transform = self.world.borrow::<View<RuntimePropTransform>>().unwrap();
                    v_transform.get(weapon).map(|p| p.0).ok()
                };
                // Flat first-person model: prefer the weapon's native
                // first-person mesh - PropPlayerGun.hand_model for guns,
                // PropLimbModel for melee weapons (wrench etc.) - loaded directly
                // so EVERY weapon gets its proper FP model, not just those listed
                // in the VR hand-model table. Other items fall back to the
                // entity's current model.
                let gun_model = self
                    .world
                    .borrow::<View<PropPlayerGun>>()
                    .ok()
                    .and_then(|v| v.get(weapon).ok().map(|g| g.hand_model.clone()));
                let limb_model = self
                    .world
                    .borrow::<View<PropLimbModel>>()
                    .ok()
                    .and_then(|v| v.get(weapon).ok().map(|m| m.0.clone()));
                let is_melee = limb_model.is_some();
                let fp_model_name = gun_model.or(limb_model);
                if let Some(xform) = maybe_xform {
                    // The reload tilt is part of the entity transform itself:
                    // the flat controller folds the reload pitch into the gun's
                    // camera-pivot pitch (see `FlatPlayerController::update`),
                    // so the gun dips around the eye like the original instead
                    // of spinning in place around its own origin.
                    let scene_objs = if let Some(name) = fp_model_name {
                        let model = asset_cache.get(&MODELS_IMPORTER, &format!("{name}.BIN"));
                        // FP meshes are articulated (hand + arm + weapon as
                        // skeleton sub-objects), so the unskinned `to_scene_objects`
                        // leaves them unposed (the wrench looked mid-swing). Pose
                        // them with an AnimationPlayer: a melee weapon mid-swing
                        // uses its swing player; otherwise melee holds the static
                        // player-melee idle (frame 0 = head-up ready stance); guns
                        // use the empty/bind pose. No-op for static meshes.
                        // Melee poses cancel the clip's root motion: the arm is
                        // anchored to the camera by the entity transform every
                        // frame (the original engine's camSynch virtual motion),
                        // so only the relative joints carry the gesture -
                        // otherwise the arm hangs low and its open cut end is
                        // visible.
                        let swing_player = self
                            .flat_melee_anim
                            .as_ref()
                            .filter(|(e, _)| *e == weapon)
                            .map(|(_, p)| p.clone());
                        let player = match (is_melee, swing_player) {
                            (_, Some(p)) => p,
                            (true, None) => asset_cache
                                .get_opt(
                                    &ANIMATION_CLIP_IMPORTER,
                                    &format!("{MELEE_IDLE_CLIP}_.mc"),
                                )
                                .map(AnimationPlayer::from_animation)
                                .unwrap_or_else(AnimationPlayer::empty),
                            (false, None) => AnimationPlayer::empty(),
                        };
                        let player = if is_melee {
                            AnimationPlayer::with_root_motion_cancelled(&player)
                        } else {
                            player
                        };
                        model.as_ref().to_animated_scene_objects(&player)
                    } else if let Some(model) = self.id_to_model.get(&weapon) {
                        match self.id_to_animation_player.get(&weapon) {
                            Some(player) => model.to_animated_scene_objects(player),
                            None => model.to_scene_objects().clone(),
                        }
                    } else {
                        Vec::new()
                    };
                    // The FP models are framed for the game's original, much
                    // wider field of view (90 deg horizontal / ~74 vertical at
                    // 4:3); under our narrower world projection the close-up
                    // viewmodel fills the screen. Instead of a second render
                    // pass with its own projection, scale the viewmodel toward
                    // the view axis in camera space by
                    // tan(world_fov/2) / tan(viewmodel_fov/2): every vertex
                    // lands on exactly the pixel the wider-FOV projection would
                    // put it on (depth is unchanged).
                    const VIEWMODEL_FOV_Y_DEG: f32 = 73.74; // 90 deg horizontal at 4:3
                    let tan_world = 1.0 / projection.y.y;
                    let tan_vm = (VIEWMODEL_FOV_Y_DEG / 2.0).to_radians().tan();
                    let s = tan_world / tan_vm;
                    let squish =
                        view.invert().unwrap() * Matrix4::from_nonuniform_scale(s, s, 1.0) * view;
                    for obj in scene_objs {
                        let mut o = obj.clone();
                        o.set_transform(squish * xform);
                        ret.push(o);
                    }

                    // Entities bolted to the viewmodel (muzzle flash etc.,
                    // skipped in the world pass) draw here too so they share
                    // the viewmodel-FOV scale - otherwise they keep their true
                    // camera-space offset while the weapon is scaled toward
                    // the view axis, and float away from the barrel.
                    let attached: Vec<(EntityId, Matrix4<f32>)> = {
                        let v_attach = self.world.borrow::<View<RuntimePropAttachment>>().unwrap();
                        let v_transform =
                            self.world.borrow::<View<RuntimePropTransform>>().unwrap();
                        v_attach
                            .iter()
                            .with_id()
                            .filter(|(_, a)| a.parent == weapon)
                            .filter_map(|(id, _)| v_transform.get(id).ok().map(|t| (id, t.0)))
                            .collect()
                    };
                    for (attached_id, attached_xform) in attached {
                        if let Some(model) = self.id_to_model.get(&attached_id) {
                            let objs = match self.id_to_animation_player.get(&attached_id) {
                                Some(player) => model.to_animated_scene_objects(player),
                                None => model.to_scene_objects().clone(),
                            };
                            for obj in objs {
                                let mut o = obj.clone();
                                o.set_transform(squish * attached_xform);
                                ret.push(o);
                            }
                        }
                    }
                }
            }

            // Flat 2D HUD (screen size is available here; the VR forearm HUD is
            // built in `render`). Drawn after the viewmodel so it stays on top.
            ret.extend(crate::hud::create_flat_hud(
                asset_cache,
                &self.world,
                screen_size,
                // The crosshair is a shooter-mode overlay; use mode replaces
                // it with the cursor (the original's ShockOverlayMouseMode
                // turns kOverlayCrosshair off while the cursor is up).
                !self.use_mode,
                // Use mode expands the compact readouts to BIOFULL/AMMOFULL.
                self.use_mode,
            ));

            // Flat MFD panel (keypad, container, ...) + cursor, drawn over
            // the HUD. Also records the render-target size the pointer ->
            // canvas mapping needs.
            self.flat_ui.set_screen_size(screen_size);
            ret.extend(self.flat_ui.render(asset_cache, screen_size));
        }

        // WhiteOut is a view transition, so it covers the world, viewmodel,
        // HUD, and flat UI and is rendered once per eye in VR.
        if self.screen_fade_alpha > 0.0 {
            let mut fade = SceneObject::screen_space_quad2(
                self.screen_fade_texture.clone(),
                vec2(0.0, 0.0),
                screen_size,
                self.screen_fade_alpha,
            );
            // A final system layer covers scene UI in both hosts. Its depth is
            // cleared once by the renderer at the explicit group boundary.
            fade.set_render_layer(RenderLayer::SystemOverlay);
            ret.push(fade);
        }

        ret
    }

    /// The VR cyber-interface overlay, in tracked pawn space: the comfort dim
    /// first (the system-overlay group's depth clear rides the first object),
    /// then the shared use-mode canvas on the anchor's panel. The caller
    /// assigns the layer and rebases into world coordinates.
    ///
    /// Placement comes from [`Self::vr_use_mode_anchor`] alone (placed on
    /// entry from the tracked head pose, world-locked, lazily recentered);
    /// the dim follows the live gaze, falling back to the panel while the
    /// head is untracked (see `crate::ui::world_dim::dim_pose`). Its strength
    /// is scaled by [`Self::use_mode_ramp`]'s eased progress, so it fades in
    /// on entry and keeps fading out after `use_mode` itself has already gone
    /// false (the caller keeps calling this for exactly that trailing window
    /// - see `render`'s `is_settled_closed()` gate). The canvas itself is
    /// empty once the strip has been cleared on exit, so only the dim
    /// actually lingers.
    /// The pointer (aim beams + hit dot) is drawn last, from the same pass the
    /// canvas was hit-tested with, so the dot can only ever mark the pixel the
    /// interface actually reacted to. Deliberately hand-less: unlike a frontend
    /// screen this is a mode of play, so the player's own hands are still being
    /// rendered by the interaction controller, and a second static glove would
    /// stack on top of them (the defect issue #1018 fixed for the pause menu).
    fn render_vr_use_mode(&self, asset_cache: &mut AssetCache) -> Vec<SceneObject> {
        use crate::ui::world_dim;
        let panel = self.vr_use_mode_anchor.panel();
        let (head_position, head_rotation) = self.vr_use_mode_head;
        let (dim_position, dim_forward) = world_dim::dim_pose(head_position, head_rotation, &panel);
        let mut objects = vec![world_dim::world_dim_layer_scaled(
            dim_position,
            dim_forward,
            world_dim::dim_distance(dim_position, &panel),
            self.use_mode_ramp.eased(),
            crate::util::render_source::USE_MODE_DIM,
        )];
        objects.extend(self.flat_ui.render_world_space(asset_cache, &panel));
        if let Some(pass) = self.vr_use_mode_pointer.as_ref() {
            let panel_layers = objects.len();
            objects.extend(crate::ui::pointer_beams(
                pass,
                crate::mission::flat_ui_host::CANVAS_SIZE,
                &panel,
                panel_layers,
            ));
        }
        objects
    }

    pub fn finish_render(
        &mut self,
        _asset_cache: &mut AssetCache,
        view: Matrix4<f32>,
        projection: Matrix4<f32>,
        screen_size: Vector2<f32>,
    ) {
        let culling_info = CullingInfo {
            view,
            projection,
            screen_size,
        };
        profile!(
            scope: "render", level: DEBUG, "visibility_engine.prepare",
            self.visibility_engine
                .prepare(self.spatial_data.as_deref(), &self.world, &culling_info)
        );
    }

    pub fn ambient_audio_state(&self) -> Option<AmbientAudioState> {
        let player_position = {
            let player_info = self.world.borrow::<UniqueView<PlayerInfo>>().ok()?;
            player_info.pos
        };

        let Ok((v_ambient_hacked, v_position)) = self
            .world
            .borrow::<(View<PropAmbientHacked>, View<PropPosition>)>()
        else {
            return Some(AmbientAudioState {
                player_position,
                music_cue: None,
                environmental_cue: None,
                ambient_emitters: Vec::new(),
            });
        };

        let mut music_cue = None;
        let mut environmental_cue = None;
        let mut emitter_candidates: Vec<(f32, EntityId, Vector3<f32>, String)> = Vec::new();

        for (id, (ambient_sound, position)) in (&v_ambient_hacked, &v_position).iter().with_id() {
            let dist_squared = (position.position - player_position).magnitude2();

            if dist_squared < ambient_sound.radius_squared {
                if ambient_sound.sound_flags.contains(AmbientSoundFlags::MUSIC) {
                    music_cue = Some(ambient_sound.schema.clone());
                } else if ambient_sound
                    .sound_flags
                    .contains(AmbientSoundFlags::ENVIRONMENTAL)
                {
                    environmental_cue = Some(ambient_sound.schema.clone());
                } else {
                    emitter_candidates.push((
                        dist_squared,
                        id,
                        position.position,
                        ambient_sound.schema.clone(),
                    ));
                }
            }
        }

        emitter_candidates.sort_by(|a, b| a.0.total_cmp(&b.0));

        let ambient_emitters = emitter_candidates
            .into_iter()
            .take(8)
            .map(|(_, id, position, sample_name)| (id, position, sample_name))
            .collect();

        Some(AmbientAudioState {
            player_position,
            music_cue,
            environmental_cue,
            ambient_emitters,
        })
    }

    pub fn render(
        &mut self,
        asset_cache: &mut AssetCache,
        options: &GameOptions,
    ) -> (Vec<SceneObject>, Vector3<f32>, Quaternion<f32>) {
        let _v_position = self.world.borrow::<View<PropPosition>>().unwrap();
        let v_transform = self.world.borrow::<View<RuntimePropTransform>>().unwrap();
        let v_frame_state = self.world.borrow::<View<PropFrameAnimState>>().unwrap();
        let v_render_type = self.world.borrow::<View<PropRenderType>>().unwrap();
        let v_render_alpha = self
            .world
            .borrow::<View<dark::properties::PropRenderAlpha>>()
            .unwrap();
        // Debug-only provenance, so tooling can report which entity/model each
        // rendered object came from.
        let v_model_name = self.world.borrow::<View<PropModelName>>().unwrap();
        let v_sym_name = self
            .world
            .borrow::<View<dark::properties::PropSymName>>()
            .unwrap();
        let v_joint_transforms = self
            .world
            .borrow::<View<RuntimePropJointTransforms>>()
            .unwrap();

        // Start with built in scene objects
        let mut scene = self.scene_objects.clone();

        let mut total_model_count = 0;
        let mut rendered_model_count = 0;

        // In flat mode the wielded weapon is drawn as a first-person viewmodel in
        // `render_per_eye` (on top, depth-test off), so skip it in the world
        // pass - along with anything attached to it (muzzle flash), which is
        // drawn in the same viewmodel pass so it shares the viewmodel-FOV
        // scale (in the world pass the flash would keep its true camera-space
        // offset while the weapon is scaled toward the view axis, and float
        // away from the barrel).
        let flat_viewmodel_skip: HashSet<EntityId> =
            if options.presentation_mode == crate::PresentationMode::Flat {
                self.interaction
                    .viewmodel_entity()
                    .map(|vm| {
                        let v_attach = self.world.borrow::<View<RuntimePropAttachment>>().unwrap();
                        let mut skip: HashSet<EntityId> = v_attach
                            .iter()
                            .with_id()
                            .filter(|(_, a)| a.parent == vm)
                            .map(|(id, _)| id)
                            .collect();
                        skip.insert(vm);
                        skip
                    })
                    .unwrap_or_default()
            } else {
                HashSet::new()
            };

        // Render models
        for (entity_id, objs) in &self.id_to_model {
            total_model_count += 1;
            if flat_viewmodel_skip.contains(entity_id) {
                continue;
            }
            if !has_refs(&self.world, *entity_id) {
                continue;
            }

            if v_render_type.contains(*entity_id) {
                let render_type = v_render_type.get(*entity_id).unwrap();
                if render_type.0 == RenderType::EditorOnly || render_type.0 == RenderType::NoRender
                {
                    continue;
                };
            }

            if !self.visibility_engine.is_visible(*entity_id) {
                continue;
            }

            rendered_model_count += 1;

            let scene_objs = {
                if let Some(player) = self.id_to_animation_player.get(entity_id) {
                    objs.to_animated_scene_objects(player)
                } else {
                    objs.to_scene_objects().clone()
                }
            };
            let is_animated_model = objs.is_animated();

            // Authored/scripted per-entity alpha (Renderer\Transparency (alpha):
            // 1.0 = opaque, 0.0 = invisible), e.g. the CS9 holo exhibits.
            let render_alpha = v_render_alpha
                .get(*entity_id)
                .ok()
                .map(|a| a.0.clamp(0.0, 1.0))
                .filter(|a| *a < 1.0);

            let debug_tag = Rc::new(engine::scene::SceneObjectDebugTag {
                entity_id: Some(entity_id.inner()),
                name: v_sym_name.get(*entity_id).ok().map(|n| n.0.clone()),
                model: v_model_name.get(*entity_id).ok().map(|m| m.0.clone()),
                source: Some("entity".to_owned()),
            });

            if let Ok(xform) = v_transform.get(*entity_id).map(|p| p.0) {
                for obj in scene_objs {
                    let mut xformed_obj = obj.clone();
                    xformed_obj.set_transform(xform);
                    xformed_obj.set_debug_tag(Some(debug_tag.clone()));
                    if options.debug_skeletons && is_animated_model {
                        xformed_obj.set_depth_write(false);
                        xformed_obj.set_skinned_transparency(Some(0.35));
                    } else if let Some(alpha) = render_alpha {
                        xformed_obj.set_depth_write(false);
                        xformed_obj.set_transparency(Some(1.0 - alpha));
                    } else {
                        xformed_obj.set_depth_write(true);
                        xformed_obj.set_skinned_transparency(None);
                    }
                    scene.push(xformed_obj);
                }

                if options.debug_skeletons {
                    if let Ok(joint_transforms) = v_joint_transforms.get(*entity_id) {
                        let world_joints: Vec<Matrix4<f32>> = joint_transforms
                            .0
                            .iter()
                            .map(|joint| xform * *joint)
                            .collect();
                        let mut debug_skeleton = objs.draw_debug_skeleton(&world_joints);
                        scene.append(&mut debug_skeleton);
                    }
                }
            }
        }
        game_log!(
            TRACE,
            "Rendered models: {} / {} total",
            rendered_model_count,
            total_model_count
        );

        scene.extend(self.rag_doll_manager.render_scene_objects());

        // Render bitmap_animation
        for (entity_id, objs) in &self.id_to_bitmap {
            if !self.visibility_engine.is_visible(*entity_id) {
                continue;
            }

            if let Ok(xform) = v_transform.get(*entity_id).map(|p| p.0) {
                let current_frame = v_frame_state
                    .get(*entity_id)
                    .map(|c| c.current_frame)
                    .unwrap_or(0);
                let texture: Rc<dyn TextureTrait> = objs
                    .get_frame(current_frame as usize, dark::FrameOptions::Wrap)
                    .unwrap();
                let mat = BillboardMaterial::create(texture, vec3(1.0, 1.0, 1.0), 1.0, 0.0, 1.0);
                let mut scene_obj = SceneObject::new(mat, Box::new(quad::create()));
                scene_obj.set_transform(xform);
                scene.push(scene_obj);
            }
        }
        // Render particle systems
        if options.render_particles {
            for (particle_entity_id, particle_system) in &self.id_to_particle_system {
                if !self.visibility_engine.is_visible(*particle_entity_id) {
                    continue;
                }

                let particle_systems = particle_system.render();
                scene.extend(particle_systems);
            }
        }

        // Render player
        let player = self.world.borrow::<UniqueView<PlayerInfo>>().unwrap();

        let player_mat = engine::scene::color_material::create(Vector3::new(0.0, 0.0, 1.0));
        let mut _player = SceneObject::new(player_mat, Box::new(engine::scene::cube::create()));
        _player.set_transform(Matrix4::from_translation(player.pos));

        // The interaction controller owns its own visuals: VR draws hand models
        // + forearm HUD panels; flat draws nothing here (its weapon viewmodel is
        // drawn on top in `render_per_eye`). They are labelled `PLAYER_HANDS_SOURCE`
        // so `Game` can drop them while the pause menu is up (issue #1018).
        scene.append(&mut self.interaction.render(asset_cache, &self.world));

        // The old synthetic blue inventory cube remains a desktop diagnostic.
        // VR presents the authored INVBACK canvas through GuiManager instead.
        if options.presentation_mode == crate::PresentationMode::Flat {
            let inventory_objs = PlayerInventoryEntity::render(&self.world);
            scene.extend(inventory_objs);
        }

        // Render teleport arc + landing indicator
        if options.experimental_features.contains("teleport")
            && self.teleport_system.get_config().enabled
        {
            let style = TeleportVisualStyle::default();
            let mut teleport_visuals = TeleportUI::build_visuals(
                self.teleport_system.get_left_hand_state(),
                self.teleport_system.get_right_hand_state(),
                &style,
            );
            scene.append(&mut teleport_visuals);
        }

        // Render debug physics
        if options.debug_physics {
            let debug_render = &self.physics.debug_render();
            scene.append(&mut debug_render.clone());
        }

        // Render debug pathfinding
        if options.debug_pathfinding {
            if let Some(ref path_database) = self.path_database {
                let mut pathfinding_visuals =
                    pathfinding_debug::render_pathfinding_debug(path_database);
                scene.append(&mut pathfinding_visuals);
            }
        }

        // Render computed paths (test paths and AI paths)
        let mut path_visuals = self.path_visualization.render();
        scene.append(&mut path_visuals);

        // self.world.run(
        //     |v_position: View<PropPosition>,
        //      v_sym_name: View<PropSymName>,
        //      v_runtime_transform: View<RuntimePropTransform>| {
        //         for (_id, (_position, sym_name, transform)) in
        //             (&v_position, &v_sym_name, &v_runtime_transform)
        //                 .iter()
        //                 .with_id()
        //         {
        //             let font = asset_cache.get(&FONT_IMPORTER, "mainfont.fon").clone();

        //             let position = transform.0.transform_point(point3(0.0, 0.0, 0.0));
        //             let mut text = SceneObject::world_space_text(&sym_name.0, font, 0.0);
        //             text.set_transform(
        //                 Matrix4::from_translation(point3_to_vec3(position))
        //                     * Matrix4::from(input_context.head.rotation.invert())
        //                     * Matrix4::from_angle_y(Rad(std::f32::consts::PI / -2.0)),
        //             );
        //             scene.push(text);
        //         }
        //     },
        // );

        // Render debug lines

        for line in &self.debug_lines {
            let start = line.start;
            let end = line.end;
            let color = line.color;
            let lines_mat =
                engine::scene::color_material::create(Vector3::new(color.x, color.y, color.z));
            let vertices = vec![
                VertexPosition {
                    position: start.to_vec(),
                },
                VertexPosition {
                    position: end.to_vec(),
                },
            ];
            let debug = SceneObject::new(
                lines_mat,
                Box::new(engine::scene::lines_mesh::create(vertices)),
            );
            scene.push(debug);
        }

        // Render world-space GUI. The explicit experiment preserves the old
        // all-panels mode; default VR renders only the gameplay panel opened
        // by frobbing its object.
        if options.experimental_features.contains("gui") {
            scene.extend(self.gui.render(asset_cache, &self.world));
        } else if options.presentation_mode == crate::PresentationMode::Vr {
            scene.extend(self.gui.render_active(asset_cache, &self.world));
        }

        // The VR cyber interface (use mode): the flat use-mode canvas on the
        // head-anchored panel, over a comfort dim - while the world behind it
        // keeps simulating. Everything is emitted in one ordered system-
        // overlay group (dim first, so the group's depth clear rides it, then
        // the canvas), built in the tracked pawn space and rebased into world
        // coordinates with the same pawn transform the runtime builds its
        // camera from.
        // Kept up (dim only, once `use_mode` itself has gone false) until the
        // exit ramp finishes releasing, so the comfort dim eases out instead
        // of vanishing the instant the panel does.
        if options.presentation_mode == crate::PresentationMode::Vr
            && (self.use_mode || !self.use_mode_ramp.is_settled_closed())
        {
            let pawn_to_world =
                Matrix4::from_translation(player.pos) * Matrix4::from(player.rotation);
            let mut use_mode_objects = self.render_vr_use_mode(asset_cache);
            for object in &mut use_mode_objects {
                object.set_render_layer(RenderLayer::SystemOverlay);
                object.set_transform(pawn_to_world * object.get_transform());
            }
            scene.extend(use_mode_objects);
        }

        // Note: Hand spotlights for enhanced lighting are now handled in the runtime
        // via get_hand_spotlights() method - they're added to the Scene's lighting system

        if options.debug_portals {
            if let Some(spatial_data) = &self.spatial_data {
                let (player_pos, _player_rot) = {
                    let player_info = self.world.borrow::<UniqueView<PlayerInfo>>().unwrap();
                    (player_info.pos, player_info.rotation)
                };
                let maybe_cell = spatial_data.get_cell_from_position(player_pos);
                if let Some(cell) = maybe_cell {
                    // println!(
                    //     "!! pos: {:?}  [cell-idx]: {:?} center: {:?} radius: {:?}",
                    //     player_pos, cell.idx, cell.center, cell.radius
                    // );
                    scene.extend(cell.debug_render());
                } else {
                    game_log!(WARN, "Unable to find cell at position: {:?}", player_pos);
                }
            }
        }

        (scene, player.pos, player.rotation)
    }

    /// Get hand spotlights for testing enhanced lighting system
    /// Returns a vector of SpotLight objects positioned at the player's hands
    pub fn get_hand_spotlights(&self, options: &GameOptions) -> Vec<SpotLight> {
        self.interaction.hand_spotlights(options)
    }

    /// Whether the runtime should show a 2D cursor instead of captured
    /// mouse-look: an MFD panel is open, or "use" mode is active
    /// (projects/flat-ui.md §5.2). Only the flat desktop runtime consults
    /// this; in VR (where use mode is the cyber interface) no runtime reads
    /// it, so the mode's truthy answer there is inert.
    pub fn wants_pointer(&self) -> bool {
        self.flat_ui.active_panel().is_some() || self.use_mode
    }

    /// See [`crate::game_scene::GameScene::fov_pull_deg`]. VR must not react
    /// to the cyber interface's ramp - OpenXR view FOVs are used as-is - so
    /// this is gated on presentation even though the ramp itself runs in
    /// both.
    pub fn fov_pull_deg(&self, game_options: &GameOptions) -> f32 {
        if game_options.presentation_mode == crate::PresentationMode::Vr {
            return 0.0;
        }
        self.use_mode_ramp.fov_pull_deg()
    }

    /// See [`crate::game_scene::GameScene::use_mode_vignette_intensity`].
    pub fn use_mode_vignette_intensity(&self) -> f32 {
        self.use_mode_ramp.vignette_intensity()
    }

    /// Actual crouch state of the player collider (stand-up can be refused
    /// for lack of headroom, so this can lag the crouch input).
    pub fn player_is_crouched(&self) -> bool {
        self.player_handle.is_crouched()
    }

    pub fn player_save_position(&self) -> Result<Vector3<f32>, PlayerSavePoseError> {
        if !self.player_is_alive() {
            return Err(PlayerSavePoseError::PlayerNotAlive);
        }
        match self
            .physics
            .get_player_save_translation(&self.player_handle)
        {
            Err(PlayerSavePoseError::UnsupportedPose) => {
                let position = self.physics.get_player_translation(&self.player_handle);
                let inside_authored_cell = self.spatial_data.as_ref().is_some_and(|spatial| {
                    (0..spatial.get_cell_count()).any(|index| {
                        spatial.get_cell_by_index(index).is_some_and(|cell| {
                            (position - cell.center).magnitude2() <= cell.radius * cell.radius
                        })
                    })
                });
                if inside_authored_cell {
                    Ok(position)
                } else {
                    Err(PlayerSavePoseError::UnsupportedPose)
                }
            }
            result => result,
        }
    }

    /// Re-apply a crouch recorded in save data. The save stores the
    /// standing-equivalent center and the player is created standing there;
    /// this drops the capsule back into the saved crouched pose before the
    /// first step (a standing capsule may not even fit, e.g. mid-crawlspace).
    pub fn restore_saved_crouch(&mut self) {
        self.physics
            .set_player_crouch(true, &mut self.player_handle);
    }

    /// Apply the effects produced by an interaction controller (the VR hands or
    /// the flat first-person controller). Shared so both presentations go
    /// through one path.
    fn process_virtual_hand_effects(
        &mut self,
        asset_cache: &mut AssetCache,
        msgs: Vec<VirtualHandEffect>,
    ) {
        for msg in msgs {
            match msg {
                VirtualHandEffect::OutMessage { message } => self.script_world.dispatch(message),
                VirtualHandEffect::ApplyForce {
                    entity_id,
                    force,
                    torque,
                } => {
                    if let Some(rigid_body_handle) = self.id_to_physics.get(&entity_id) {
                        self.physics.apply_torque(*rigid_body_handle, torque);
                        self.physics.apply_force(*rigid_body_handle, force)
                    };
                }
                VirtualHandEffect::SetPositionRotation {
                    entity_id,
                    position,
                    rotation,
                    scale,
                } => {
                    self.set_entity_position_rotation(entity_id, position, rotation, scale);
                }
                VirtualHandEffect::SpawnEntity {
                    template_id,
                    position,
                    rotation,
                } => {
                    self.create_entity_with_position(
                        asset_cache,
                        template_id,
                        vec3_to_point3(position),
                        rotation,
                        Matrix4::identity(),
                        CreateEntityOptions::default(),
                    );
                }
                VirtualHandEffect::HoldItem { entity_id } => {
                    if self.is_vr_melee_weapon(entity_id) {
                        // Held guns/items stay unphysical, but a VR melee
                        // weapon needs its authored collider to report genuine
                        // controller-driven contacts. Kinematic motion keeps it
                        // seated in the hand without gravity or solver drift.
                        self.attach_held_melee_physics(entity_id);
                    } else {
                        self.make_un_physical(entity_id);
                    }
                    self.script_world.dispatch(Message {
                        payload: MessagePayload::Hold,
                        to: entity_id,
                    });
                }
                VirtualHandEffect::StoreItem { entity_id } => {
                    let inventory_entity = self
                        .world
                        .borrow::<UniqueView<PlayerInfo>>()
                        .map(|player| player.inventory_entity_id)
                        .ok();
                    if let Some(inventory_entity) = inventory_entity {
                        self.drop_entity_into_container(inventory_entity, entity_id);
                    }
                }
                VirtualHandEffect::DropItem { entity_id } => {
                    if !restore_live_entity_world_refs(&mut self.world, entity_id) {
                        continue;
                    }
                    if self.is_vr_melee_weapon(entity_id) {
                        // Recreate from authored physics so the released item
                        // is an ordinary dynamic, harmless loose prop again.
                        self.make_un_physical(entity_id);
                    }
                    // After the body is gone (so this writes the entity's
                    // transform rather than pushing the outgoing kinematic
                    // body around) and before the loose prop is rebuilt from
                    // it.
                    self.return_held_entity_to_the_hand(entity_id);
                    self.make_physical(entity_id);

                    self.script_world.dispatch(Message {
                        payload: MessagePayload::Drop,
                        to: entity_id,
                    });
                }
            }
        }
    }

    fn is_vr_melee_weapon(&self, entity_id: EntityId) -> bool {
        is_vr_melee_weapon(&self.world, entity_id)
    }

    /// Undo a computed grip offset before a released item becomes a loose prop
    /// again.
    ///
    /// A melee `_h` wield deliberately parks the entity on the *weapon head* -
    /// its body is the contact collider, so that is where the damage volume
    /// belongs - which is up to 1.2 units out of the palm. Respawning the world
    /// prop there would materialize it over the player's head and drop it from
    /// height; every other release happens at the hand, so put it back there
    /// first. No-op for anything without a computed grip.
    fn return_held_entity_to_the_hand(&mut self, entity_id: EntityId) {
        use cgmath::Rotation;

        let Some(grip) = self
            .world
            .borrow::<View<RuntimePropVrGripOffset>>()
            .ok()
            .and_then(|view| view.get(entity_id).ok().map(|grip| grip.0))
        else {
            return;
        };

        let Some((position, rotation)) =
            self.world
                .borrow::<View<PropPosition>>()
                .ok()
                .and_then(|view| {
                    view.get(entity_id)
                        .ok()
                        .map(|pos| (pos.position, pos.rotation))
                })
        else {
            return;
        };
        // The grip is hand-local and the entity carries the hand's rotation
        // (a computed grip contributes none of its own).
        let hand = position - rotation.rotate_vector(grip);
        self.set_entity_position_rotation(entity_id, hand, rotation, vec3(1.0, 1.0, 1.0));
    }

    /// Give a held melee weapon the contact body the VR damage window needs.
    /// Idempotent: `make_physical` no-ops when the body already exists, so this
    /// is safe on both a fresh grab and a restore.
    fn attach_held_melee_physics(&mut self, entity_id: EntityId) {
        self.make_physical(entity_id);
        self.physics.set_held_melee(entity_id);
    }

    /// Queue an entity to be triggered after scripts are initialized
    pub fn queue_entity_trigger(&mut self, entity_name: String) {
        println!("Queueing entity trigger for: {}", entity_name);
        self.pending_entity_triggers.push(entity_name);
    }

    /// Interactive pathfinding test system
    pub fn pathfinding_test_action(&mut self, action: &str) -> String {
        let player_pos = self.player_position();
        self.pathfinding_test.handle_action(
            action,
            player_pos,
            &self.pathfinding_service,
            &mut self.path_visualization,
        )
    }

    /// Internal method to trigger an entity and return messages to dispatch
    fn trigger_entity_by_name_internal(&mut self, entity_name: String) -> Vec<Message> {
        let entities = scripts::script_util::get_entities_by_name(&self.world, &entity_name);
        println!(
            "Triggering {} entities with name: {}",
            entities.len(),
            entity_name
        );

        let mut messages = Vec::new();
        for entity_id in entities {
            // Get all switch links and create TurnOn messages for each target
            let switch_links = scripts::script_util::get_all_switch_links(&self.world, entity_id);
            println!(
                "Found {} switch links for entity {:?}",
                switch_links.len(),
                entity_id
            );

            for target_entity_id in switch_links {
                let message = Message {
                    payload: MessagePayload::TurnOn { from: entity_id },
                    to: target_entity_id,
                };
                println!(
                    "Creating TurnOn message from {:?} to {:?}",
                    entity_id, target_entity_id
                );
                messages.push(message);
            }
        }
        messages
    }
}

fn create_template_name_map(
    game_entity_info: &Gamesys,
) -> (HashMap<String, EntityMetadata>, HashMap<String, i32>) {
    let mut gamesys_world = World::new();
    game_entity_info.entity_info.initialize_world_with_entities(
        &mut gamesys_world,
        HashMap::new(),
        |_id| true,
    );

    let mut name_to_template_id = HashMap::new();
    let mut template_ids_by_exact_name: HashMap<String, Vec<i32>> = HashMap::new();

    gamesys_world.run(
        |v_sym_name: View<dark::properties::PropSymName>,
         v_obj_icon: View<dark::properties::PropObjIcon>,
         v_obj_short_name: View<dark::properties::PropObjShortName>,
         v_obj_name: View<dark::properties::PropObjName>,
         v_template_id: View<dark::properties::PropTemplateId>| {
            for (entity_id, (sym_name, template_id)) in
                (&v_sym_name, &v_template_id).iter().with_id()
            {
                name_to_template_id.insert(
                    sym_name.0.to_ascii_lowercase(),
                    EntityMetadata {
                        template_id: template_id.template_id,
                        obj_icon: v_obj_icon
                            .get(entity_id)
                            .map(|p| format!("{}.pcx", p.0))
                            .ok(),
                        obj_name: v_obj_name.get(entity_id).map(|p| p.0.clone()).ok(),
                        obj_short_name: v_obj_short_name.get(entity_id).map(|p| p.0.clone()).ok(),
                    },
                );
                if template_id.template_id < 0 {
                    template_ids_by_exact_name
                        .entry(sym_name.0.clone())
                        .or_default()
                        .push(template_id.template_id);
                }
            }
        },
    );

    let unique_template_names = template_ids_by_exact_name
        .into_iter()
        .filter_map(|(name, ids)| (ids.len() == 1).then_some((name, ids[0])))
        .collect();
    (name_to_template_id, unique_template_names)
}

/// Index this mission's own objects (the positive id space) that author their
/// own `P$SymName`. Retail spawn markers name their archetype by object name,
/// and that name often belongs to a concrete off-map object placed in the
/// mission (earth.mis parks "DopeyDroid" as the training-droid archetype)
/// rather than to a gamesys template. Only *own* `P$SymName`s are indexed - an
/// object that merely inherits its archetype's name, or that is named only by
/// the mission's `OBJ_MAP` (generic archetype names like "Marker", see
/// `entity_creator::initialize_sym_name_from_obj_map`), is not a distinct
/// archetype.
fn create_mission_object_name_map(entity_info: &SystemShock2EntityInfo) -> HashMap<String, i32> {
    let mut world = World::new();
    for (id, props) in &entity_info.entity_to_properties {
        if *id < 0 {
            continue;
        }
        let entity = world.add_entity(dark::properties::PropTemplateId { template_id: *id });
        for prop in props {
            prop.initialize(&mut world, entity);
        }
    }

    let mut ids_by_name: HashMap<String, Vec<i32>> = HashMap::new();
    world.run(
        |v_sym_name: View<dark::properties::PropSymName>,
         v_template_id: View<dark::properties::PropTemplateId>| {
            for (sym_name, template_id) in (&v_sym_name, &v_template_id).iter() {
                ids_by_name
                    .entry(sym_name.0.to_ascii_lowercase())
                    .or_default()
                    .push(template_id.template_id);
            }
        },
    );
    // Only unambiguous names resolve - picking one of several same-named
    // objects would spawn an arbitrary one (same reasoning as the gamesys
    // `unique_gamesys_template_names` map above).
    ids_by_name
        .into_iter()
        .filter_map(|(name, ids)| (ids.len() == 1).then_some((name, ids[0])))
        .collect()
}

/// Create a map of template IDs to their class tag data for script access
fn create_template_class_tag_map(
    entity_info: &Arc<SystemShock2EntityInfo>,
) -> HashMap<i32, HashMap<String, String>> {
    use crate::scripts::script_util::hydrate_template_component;

    let mut class_tag_map = HashMap::new();

    // Iterate through all template IDs to extract PropClassTag data
    for template_id in entity_info.entity_to_properties.keys() {
        if let Some(class_tag) =
            hydrate_template_component::<PropClassTag>(*template_id, entity_info)
        {
            let mut tag_map = HashMap::new();
            for (key, value) in class_tag.class_tags() {
                tag_map.insert(key.to_string(), value.to_string());
            }
            class_tag_map.insert(*template_id, tag_map);
        }
    }

    class_tag_map
}

///
/// initialize_background_music
///
/// Helper function to set up the music player for the level
fn initialize_background_music(
    song_params: &SongParams,
    asset_cache: &mut AssetCache,
    audio_context: &mut AudioContext<EntityId, String>,
) {
    let song_file_name = &song_params.song;
    info!("loading music for level: {}", song_file_name);
    if !song_file_name.is_empty() {
        let song = {
            asset_cache
                .get(&SONG_IMPORTER, &format!("{song_file_name}.snc"))
                .clone()
        };
        let background_music_player = SongPlayer::new(&song, asset_cache);
        audio_context.set_background_music(Box::new(background_music_player));
    } else {
        audio_context.stop_background_music();
    }
}

fn create_room_entities(
    room_db: &RoomDatabase,
    template_to_entity_id: &HashMap<i32, WrappedEntityId>,
    world: &mut World,
    entities_to_initialize: &mut HashSet<(EntityId, i32)>,
) {
    // HACK: The collision detection for entering / exiting rooms is different here
    // than in Dark. We fire the edge detection events on any intersection, whereas
    // Dark seems to use a stricter check. For now, we'll just offset the rooms
    // to give a similar effect....
    let vert_offset = vec3(0.0, 5.0 / SCALE_FACTOR, 0.0);

    for room in &room_db.rooms {
        let link = Links {
            to_links: option_to_vec(template_to_entity_id.get(&room.obj_id).map(|id| ToLink {
                link: Link::SwitchLink,
                to_entity_id: Some(*id),
                to_template_id: room.obj_id,
            })),
        };

        let _room = world.add_entity((
            RuntimePropDoNotSerialize,
            PropPosition {
                position: room.center + vert_offset,
                rotation: Quaternion {
                    v: Vector3::zero(),
                    s: 1.0,
                },
                cell: 0, // TODO - needed?
            },
            PropScripts {
                scripts: vec!["internal_room_trigger".to_owned()],
                inherits: false,
            },
            PropPhysDimensions {
                radius0: 0.0,
                radius1: 1.0,
                unk1: 0,
                unk2: 0,
                offset0: Vector3::zero(),
                offset1: Vector3::zero(),
                size: (room.bounding_box.max - room.bounding_box.min),
            },
            PropPhysType {
                is_special: false,
                num_submodels: 1,
                phys_type: PhysicsModelType::ORIENTED_BOUNDING_BOX,
                remove_on_sleep: false,
            },
            PropTripFlags {
                trip_flags: TripFlags::ENTER | TripFlags::EXIT | TripFlags::PLAYER,
            },
            PropPhysState {
                position: room.center + vert_offset,
                velocity: Vector3::zero(),
                rot_velocity: Vector3::zero(),
                rotation: Quaternion {
                    v: Vector3::zero(),
                    s: 1.0,
                },
            },
            link,
        ));
        entities_to_initialize.insert((_room, room.obj_id));
    }
}

fn random_unit_vector(rng: &mut impl Rng) -> Vector3<f32> {
    loop {
        let v = vec3(
            rng.gen_range(-1.0f32..=1.0),
            rng.gen_range(-1.0f32..=1.0),
            rng.gen_range(-1.0f32..=1.0),
        );
        let mag2 = v.magnitude2();
        if mag2 > 1e-4 && mag2 <= 1.0 {
            return v / mag2.sqrt();
        }
    }
}

fn option_to_vec<T>(option: Option<T>) -> Vec<T> {
    match option {
        None => vec![],
        Some(v) => vec![v],
    }
}

fn carried_research_entity(world: &World, template_id: i32) -> Option<EntityId> {
    crate::scripts::script_util::player_carried_items(world)
        .into_iter()
        .find(|entity| {
            crate::scripts::script_util::entity_class_template_id(world, *entity)
                == Some(template_id)
        })
}

/// Advance the one active campaign research project from its carried object's
/// live authored metadata. Keeping this central avoids multiplying progress by
/// the number of copies/scripts in the world.
fn update_research(world: &World, real_seconds: f32) -> Vec<Effect> {
    if real_seconds <= 0.0 {
        return Vec::new();
    }
    let active_template = world
        .borrow::<UniqueView<QuestInfo>>()
        .ok()
        .and_then(|quests| quests.research().active_template_id());
    let Some(template_id) = active_template else {
        return Vec::new();
    };
    let Some(entity_id) = carried_research_entity(world, template_id) else {
        if let Ok(mut quests) = world.borrow::<UniqueViewMut<QuestInfo>>() {
            quests.research_mut().suspend();
        }
        return Vec::new();
    };

    let total = world
        .borrow::<View<dark::properties::PropResearchTime>>()
        .ok()
        .and_then(|view| view.get(entity_id).ok().map(|time| time.0 as f32));
    let Some(total) = total else {
        return Vec::new();
    };
    let chemicals = world
        .borrow::<View<dark::properties::PropChemicalNeeded>>()
        .ok()
        .and_then(|view| view.get(entity_id).ok().cloned());
    let report_mask = world
        .borrow::<View<dark::properties::PropResearchReport>>()
        .ok()
        .and_then(|view| view.get(entity_id).ok().map(|report| report.0))
        .unwrap_or(0);
    let research_factor = world
        .borrow::<UniqueView<GlobalSkillParams>>()
        .ok()
        .and_then(|params| params.0.as_ref().map(|params| params.research_factor))
        .unwrap_or(1.0);

    let outcome = {
        let mut quests = world.borrow::<UniqueViewMut<QuestInfo>>().unwrap();
        let skill = quests
            .player_stats()
            .skill_level(crate::player_stats::Skill::Research);
        quests.research_mut().advance(
            template_id,
            real_seconds,
            skill,
            research_factor,
            total,
            chemicals.as_ref(),
            report_mask,
        )
    };

    if outcome != crate::research::AdvanceResearchResult::Completed {
        return Vec::new();
    }
    game_log!(INFO, "Research complete");
    // Research belongs to the archetype, not one inventory instance. A legacy
    // campaign can already carry multiple Toxin-A vials when the project
    // completes, so normalize every live copy immediately; newly created
    // copies are covered by ResearchableScript::initialize.
    let canonical = world
        .borrow::<View<crate::runtime_props::RuntimePropCanonicalTemplateId>>()
        .unwrap();
    let mut effects = (&canonical)
        .iter()
        .with_id()
        .filter(|(_, canonical)| canonical.0 == template_id)
        .map(|(entity_id, _)| Effect::SetObjectState {
            entity_id,
            state: dark::properties::ObjectState::Normal,
        })
        .collect::<Vec<_>>();
    if let Some(quest_bit) = crate::scripts::script_util::set_quest_bit_effect(world, entity_id) {
        effects.push(quest_bit);
    }
    effects
}

fn apply_research_chemical(world: &World, chemical_entity: EntityId) -> bool {
    if !crate::scripts::script_util::player_carried_items(world).contains(&chemical_entity) {
        return false;
    }
    let chemical_name = world
        .borrow::<View<dark::properties::PropSymName>>()
        .ok()
        .and_then(|names| names.get(chemical_entity).ok().map(|name| name.0.clone()));
    let Some(chemical_name) = chemical_name else {
        return false;
    };
    let active_template = world
        .borrow::<UniqueView<QuestInfo>>()
        .ok()
        .and_then(|quests| quests.research().active_template_id());
    let Some(active_entity) = active_template.and_then(|id| carried_research_entity(world, id))
    else {
        return false;
    };
    let needed = world
        .borrow::<View<dark::properties::PropChemicalNeeded>>()
        .ok()
        .and_then(|view| view.get(active_entity).ok().cloned());
    let Some(needed) = needed else {
        return false;
    };
    world
        .borrow::<UniqueViewMut<QuestInfo>>()
        .map(|mut quests| {
            quests
                .research_mut()
                .provide_chemical(&chemical_name, &needed)
        })
        .unwrap_or(false)
}

/// Remove every `Contains` link in `links` that points at `target` - used when
/// an item leaves a container (moved/dropped into another container, or
/// destroyed while contained), so no stale `Contains` link is left behind.
fn drop_contains_links_to(links: &mut Links, target: EntityId) {
    links.to_links.retain(|link| {
        !(matches!(link.link, Link::Contains(_)) && link.to_entity_id.map(|e| e.0) == Some(target))
    });
}

/// Is this a melee weapon whose VR contact damage needs a live collider while
/// held? Authored player melee weapons are marked by `PropLimbModel`
/// (gamesys-wide: Wrench -928, PsiSword -2291, Crystal Shard -28, Electro
/// Shock -24); the flat presentation swings by raycast and needs none of this.
pub fn is_vr_melee_weapon(world: &World, entity_id: EntityId) -> bool {
    world
        .borrow::<UniqueView<GlobalPresentationMode>>()
        .is_ok_and(|mode| mode.0 == crate::PresentationMode::Vr)
        && world
            .borrow::<View<PropLimbModel>>()
            .is_ok_and(|limb_models| limb_models.get(entity_id).is_ok())
}

/// Physics for an item restored into a hand by save/load or a level change.
///
/// The restore path cannot reuse `VirtualHandEffect::HoldItem` - only a fresh
/// world grab emits that, and `VrInteraction::grab` returns no effects - so
/// without this branch a carried melee weapon came back with its collider
/// stripped and silently stopped reporting contacts for the rest of the
/// session. Every other held item still goes unphysical, as before.
fn restore_held_item_physics(
    world: &World,
    id_to_physics: &mut HashMap<EntityId, RigidBodyHandle>,
    physics: &mut PhysicsWorld,
    entity_id: EntityId,
) {
    if is_vr_melee_weapon(world, entity_id) {
        physics.set_held_melee(entity_id);
    } else {
        make_un_physical2(id_to_physics, physics, entity_id);
    }
}

pub fn make_un_physical2(
    id_to_physics: &mut HashMap<EntityId, RigidBodyHandle>,
    physics: &mut PhysicsWorld,
    entity_id: EntityId,
) {
    let current_entity = id_to_physics.get(&entity_id);
    if current_entity.is_none() {
        return;
    }

    physics.remove(entity_id);
    id_to_physics.remove(&entity_id);
}

fn resolve_schema(global_context: &GlobalContext, name: &str) -> (dark::ResolvedSoundSchema, bool) {
    let sound_schema = &global_context.gamesys.sound_schema;
    if let Some(resolved) = sound_schema.resolve(name) {
        trace!(
            "resolved sound schema {} to {} at {} millibels, pan {}",
            name, resolved.sample_name, resolved.volume_millibels, resolved.pan_millibels
        );
        (resolved, true)
    } else {
        trace!("sound {} is a direct sample, not a schema", name);
        (
            dark::ResolvedSoundSchema {
                sample_name: name.to_owned(),
                volume_millibels: 0,
                pan_millibels: 0,
            },
            false,
        )
    }
}

fn resolve_speech_sample(
    gamesys: &Gamesys,
    voice_index: usize,
    concept: &str,
    tags: &[(String, String)],
) -> Option<dark::ResolvedSoundSchema> {
    let speech_db = gamesys.speech_db();
    if voice_index >= speech_db.voices.len() {
        return None;
    }

    let concept_key = concept.to_ascii_lowercase();
    let concept_idx = speech_db.concept_map.get_index(&concept_key)? as usize;

    let voice = &speech_db.voices[voice_index];
    if concept_idx >= voice.tag_maps.len() {
        return None;
    }

    let tag_db = &voice.tag_maps[concept_idx];

    let mut query_items = Vec::new();
    for (tag, value) in tags {
        let lowered_tag = tag.to_ascii_lowercase();
        let lowered_value = value.to_ascii_lowercase();
        let tag_idx = speech_db.tag_map.get_index(&lowered_tag);
        let value_idx = speech_db
            .value_map
            .get_index(&lowered_value)
            .map(|idx| idx as u8);

        if let (Some(tag_id), Some(value_id)) = (tag_idx, value_idx) {
            query_items.push(TagQueryItem::KeyWithEnumValue(tag_id, value_id, false));
        }
    }

    let schema_candidates = if query_items.is_empty() {
        tag_db.collect_all_data_ids()
    } else {
        let query = TagQuery::from_items(query_items);
        tag_db.query_match_all(&query)
    };

    if schema_candidates.is_empty() {
        return None;
    }

    let mut rng = thread_rng();
    let schema_id = *schema_candidates
        .choose(&mut rng)
        .unwrap_or(&schema_candidates[0]);

    let samples = gamesys.sound_schema.id_to_samples.get(&schema_id)?;
    if samples.is_empty() {
        return None;
    }

    let weights: Vec<f64> = samples
        .iter()
        .map(|sample| f64::from(sample.frequency.max(1)))
        .collect();

    let selected_index = WeightedIndex::new(weights)
        .map(|dist| dist.sample(&mut rng))
        .unwrap_or_else(|_| rng.gen_range(0..samples.len()));

    gamesys
        .sound_schema
        .resolve_sample(schema_id, samples[selected_index].sample_name.clone())
}

/// Stable identity of the entity a sound came from, for the audio log.
fn source_entity(world: &World, entity_id: EntityId) -> crate::audio_log::SourceEntity {
    let (name, template_id) = crate::util::entity_ident(world, entity_id);
    crate::audio_log::SourceEntity { name, template_id }
}

fn play_environmental_sound(
    gamesys: &Gamesys,
    asset_cache: &mut AssetCache,
    audio_context: &mut AudioContext<EntityId, String>,
    query: dark::EnvSoundQuery,
    audio_handle: AudioHandle,
    position: Vector3<f32>,
) {
    if let Some(resolved) = gamesys.get_random_environmental_sound(&query) {
        let audio_clip = asset_cache.get(&AUDIO_IMPORTER, &format!("{}.wav", resolved.sample_name));

        info!(
            "Playing clip: {} handle: {:?} position: {:?}",
            resolved.sample_name, &audio_handle, position
        );
        // Log the resolved play so headless tooling (debug runtime
        // /v1/audio/recent) can assert a schema actually played.
        let duration = audio_clip.total_duration();
        let handle_id = audio_handle.id();
        let gain = resolved.linear_gain();
        let preempted = engine::audio::play_spatial_audio_with_gain(
            audio_context,
            position,
            None,
            audio_handle,
            None,
            audio_clip,
            gain,
        );
        crate::audio_log::record_stops(&preempted);
        crate::audio_log::record(crate::audio_log::SoundRecord {
            sample: &resolved.sample_name,
            volume_millibels: Some(resolved.volume_millibels),
            gain,
            pan_millibels: Some(resolved.pan_millibels),
            pan_applied: false,
            tags: query.tag_values(),
            position: [position.x, position.y, position.z],
            duration,
            source_entity: None,
            handle: Some(handle_id),
        });
    }
}

// Implementation of GameScene trait for Mission
// ============================================================================
// DebuggableScene Implementation for MissionCore
// ============================================================================

impl MissionCore {
    /// Build a lookup from `EntityId::inner() as i32` to symbolic name, matching
    /// the id convention used by the entity-listing debug endpoints.
    fn entity_names_by_inner(&self) -> std::collections::HashMap<i32, String> {
        use shipyard::*;
        let mut names = std::collections::HashMap::new();
        self.world
            .run(|v_sym_name: View<dark::properties::PropSymName>| {
                for (entity_id, sym_name) in v_sym_name.iter().with_id() {
                    names.insert(entity_id.inner() as i32, sym_name.0.clone());
                }
            });
        names
    }
}

fn debug_link_info(
    opposite_id: EntityId,
    link: &ToLink,
    names: &View<PropSymName>,
) -> crate::game_scene::DebugLinkInfo {
    crate::game_scene::DebugLinkInfo {
        link_type: format!("{:?}", link.link),
        target_id: opposite_id.inner() as i32,
        target_name: names
            .get(opposite_id)
            .map(|name| name.0.clone())
            .unwrap_or_else(|_| format!("Entity_{}", opposite_id.inner())),
        contains_ordinal: match link.link {
            Link::Contains(ordinal) => Some(ordinal),
            _ => None,
        },
    }
}

/// Return both directions of every live link touching `id`. `Links` stores
/// only its outgoing half, so incoming links must be resolved by scanning the
/// sources. Each [`DebugLinkInfo`](crate::game_scene::DebugLinkInfo) names the
/// opposite endpoint: destination in `outgoing`, source in `incoming`.
fn debug_entity_links(
    id: EntityId,
    links: &View<Links>,
    names: &View<PropSymName>,
) -> (
    Vec<crate::game_scene::DebugLinkInfo>,
    Vec<crate::game_scene::DebugLinkInfo>,
) {
    let mut outgoing = Vec::new();
    let mut incoming = Vec::new();

    if let Ok(entity_links) = links.get(id) {
        for link in &entity_links.to_links {
            if let Some(target) = link.to_entity_id {
                outgoing.push(debug_link_info(target.0, link, names));
            }
        }
    }

    for (source_id, source_links) in links.iter().with_id() {
        for link in &source_links.to_links {
            if link.to_entity_id.map(|target| target.0) == Some(id) {
                incoming.push(debug_link_info(source_id, link, names));
            }
        }
    }

    (outgoing, incoming)
}

#[cfg(test)]
mod debug_entity_link_tests {
    use super::*;

    #[test]
    fn reports_outgoing_targets_and_incoming_sources_with_contains_metadata() {
        let mut world = World::new();
        let item = world.add_entity(PropSymName("Hydro Card A".to_owned()));
        let container = world.add_entity((
            PropSymName("Hydro Corpse".to_owned()),
            Links {
                to_links: vec![ToLink {
                    to_template_id: 934,
                    to_entity_id: Some(WrappedEntityId(item)),
                    link: Link::Contains(7),
                }],
            },
        ));

        world.run(|links: View<Links>, names: View<PropSymName>| {
            let (container_outgoing, container_incoming) =
                debug_entity_links(container, &links, &names);
            assert_eq!(container_incoming.len(), 0);
            assert_eq!(container_outgoing.len(), 1);
            assert_eq!(container_outgoing[0].target_id, item.inner() as i32);
            assert_eq!(container_outgoing[0].target_name, "Hydro Card A");
            assert_eq!(container_outgoing[0].contains_ordinal, Some(7));

            let (item_outgoing, item_incoming) = debug_entity_links(item, &links, &names);
            assert_eq!(item_outgoing.len(), 0);
            assert_eq!(item_incoming.len(), 1);
            assert_eq!(item_incoming[0].target_id, container.inner() as i32);
            assert_eq!(item_incoming[0].target_name, "Hydro Corpse");
            assert_eq!(item_incoming[0].contains_ordinal, Some(7));
        });
    }
}

impl crate::game_scene::DebuggableScene for MissionCore {
    fn list_entities(
        &self,
        limit: Option<usize>,
        filter: Option<&str>,
    ) -> Vec<crate::game_scene::DebugEntitySummary> {
        use crate::game_scene::DebugEntitySummary;
        use shipyard::*;

        let player_pos = self.player_position();
        let mut entities = Vec::new();

        // Query all entities with position
        self.world.run(
            |_entities_iter: EntitiesView,
             v_pos: View<dark::properties::PropPosition>,
             v_transform: View<crate::runtime_props::RuntimePropTransform>,
             v_sym_name: View<dark::properties::PropSymName>,
             v_scripts: View<dark::properties::PropScripts>,
             v_template_id: View<dark::properties::PropTemplateId>,
             v_links: View<dark::properties::Links>| {
                for (entity_id, pos) in v_pos.iter().with_id() {
                    let name = v_sym_name
                        .get(entity_id)
                        .map(|s| s.0.clone())
                        .unwrap_or_else(|_| format!("Entity_{}", entity_id.inner()));

                    // Apply filter if provided
                    if let Some(filter_str) = filter {
                        if !wildcard_match(&name, filter_str) {
                            continue;
                        }
                    }

                    // Prefer the live transform; PropPosition can lag for
                    // entities moved by animation/physics (e.g. walking AIs)
                    let live_pos = v_transform
                        .get(entity_id)
                        .map(|xform| {
                            use cgmath::Transform;
                            let p = xform.0.transform_point(cgmath::point3(0.0, 0.0, 0.0));
                            cgmath::vec3(p.x, p.y, p.z)
                        })
                        .unwrap_or(pos.position);
                    let position = [live_pos.x, live_pos.y, live_pos.z];
                    let distance = (cgmath::Vector3::from(position) - player_pos).magnitude();

                    let script_count = v_scripts
                        .get(entity_id)
                        .map(|scripts| scripts.scripts.len())
                        .unwrap_or(0);

                    let link_count = v_links
                        .get(entity_id)
                        .map(|links| links.to_links.len())
                        .unwrap_or(0);

                    // The template this instance was created from (a negative
                    // gamesys/mission template id). Unlike the runtime entity
                    // id, this is stable across runs. 0 if the entity has no
                    // template backlink.
                    let template_id = v_template_id
                        .get(entity_id)
                        .map(|t| t.template_id)
                        .unwrap_or(0);

                    entities.push(DebugEntitySummary {
                        id: entity_id.inner() as i32,
                        name,
                        template_id,
                        position,
                        distance,
                        script_count,
                        link_count,
                    });
                }
            },
        );

        // Sort by distance from player
        // `total_cmp` is a genuine total order. `partial_cmp(..).unwrap_or(Equal)`
        // is NOT: when an entity has a NaN distance (some levels, e.g. earth /
        // rec2, contain an entity at a non-finite position) the comparator
        // reports Equal inconsistently, which trips Rust's total-order check and
        // panics the sort - killing the game thread on the next entity list.
        entities.sort_by(|a, b| a.distance.total_cmp(&b.distance));

        // Apply limit if provided
        if let Some(limit) = limit {
            entities.truncate(limit);
        }

        entities
    }

    fn animation_state(&self, id: EntityId) -> Option<crate::game_scene::DebugAnimationState> {
        use crate::game_scene::{
            DebugAnimationBlend, DebugAnimationQueueEntry, DebugAnimationState,
        };
        use shipyard::*;

        let snapshot = self.id_to_animation_player.get(&id)?.snapshot();

        let (transform, joint_transforms) = self.world.run(
            |v_transform: View<crate::runtime_props::RuntimePropTransform>,
             v_joints: View<crate::runtime_props::RuntimePropJointTransforms>| {
                (
                    v_transform.get(id).ok().map(|t| t.0),
                    v_joints.get(id).ok().map(|j| j.0),
                )
            },
        );

        let transform = transform.unwrap_or_else(Matrix4::identity);
        let position = [transform.w.x, transform.w.y, transform.w.z];
        // The transform is translation * rotation * scale (PropScale, possibly
        // non-uniform), so the basis columns must be normalized before the
        // quaternion conversion - Quaternion::from assumes an orthonormal
        // matrix and returns a different rotation for a scaled one.
        let basis_x = vec3(transform.x.x, transform.x.y, transform.x.z).normalize();
        let basis_y = vec3(transform.y.x, transform.y.y, transform.y.z).normalize();
        let basis_z = vec3(transform.z.x, transform.z.y, transform.z.z).normalize();
        let rotation_mat = Matrix3::from_cols(basis_x, basis_y, basis_z);
        let rotation_quat = Quaternion::from(rotation_mat).normalize();
        let rotation = [
            rotation_quat.v.x,
            rotation_quat.v.y,
            rotation_quat.v.z,
            rotation_quat.s,
        ];

        let joints = joint_transforms
            .map(|joint_transforms| {
                joint_transforms
                    .iter()
                    .map(|joint| {
                        let world = transform * joint;
                        [world.w.x, world.w.y, world.w.z]
                    })
                    .collect()
            })
            .unwrap_or_default();

        let mut queue = snapshot.queue;
        let head = if queue.is_empty() {
            None
        } else {
            Some(queue.remove(0))
        };

        Some(DebugAnimationState {
            entity_id: id.inner() as i32,
            clip: head.as_ref().and_then(|entry| entry.name.clone()),
            frame: snapshot.current_frame,
            num_frames: head.as_ref().map(|entry| entry.num_frames).unwrap_or(0),
            looping: head.as_ref().map(|entry| entry.looping).unwrap_or(false),
            remaining_time: snapshot.remaining_time,
            queue: queue
                .into_iter()
                .map(|entry| DebugAnimationQueueEntry {
                    name: entry.name,
                    num_frames: entry.num_frames,
                    looping: entry.looping,
                })
                .collect(),
            last_clip: snapshot.last_clip,
            blend: snapshot.blend.map(|blend| DebugAnimationBlend {
                from_clip: blend.from_clip,
                from_frame: blend.from_frame,
                duration: blend.duration,
                elapsed: blend.elapsed,
                alpha: if blend.duration > f32::EPSILON {
                    (blend.elapsed / blend.duration).clamp(0.0, 1.0)
                } else {
                    1.0
                },
            }),
            position,
            rotation,
            joints,
        })
    }

    fn resolve_entity_id(&self, id: i32) -> Option<EntityId> {
        use shipyard::EntitiesView;

        self.world
            .borrow::<EntitiesView>()
            .ok()
            .and_then(|entities| entities.iter().find(|e| e.inner() as i32 == id))
    }

    fn entity_detail(&self, id: EntityId) -> Option<crate::game_scene::DebugEntityDetail> {
        use crate::game_scene::{DebugAimPoint, DebugEntityDetail, DebugPropertyInfo};
        use shipyard::*;

        let aim_points = self
            .world
            .run(|hitboxes: View<crate::creature::RuntimePropHitBox>| {
                hitboxes
                    .iter()
                    .with_id()
                    .filter(|(_, hitbox)| hitbox.parent_entity_id == id)
                    .filter_map(|(proxy_id, hitbox)| {
                        let handle = *self.id_to_physics.get(&proxy_id)?;
                        let body = self.physics.debug_body_detail(handle.into_raw_parts().0)?;
                        Some(DebugAimPoint {
                            proxy_entity_id: proxy_id.inner() as i32,
                            body_id: body.body_id,
                            joint_id: hitbox.joint_id,
                            classification: match hitbox.hit_box_type {
                                crate::creature::HitBoxType::Head => "head",
                                crate::creature::HitBoxType::Body => "torso",
                                crate::creature::HitBoxType::Limb => "limb",
                                crate::creature::HitBoxType::Extremity => "extremity",
                                crate::creature::HitBoxType::NoDamage => "no_damage",
                            }
                            .to_string(),
                            position: body.position,
                        })
                    })
                    .collect::<Vec<_>>()
            });

        // Looked up outside the main run: the closure below is at shipyard's
        // view-count limit.
        let hearing_rating = self
            .world
            .run(|v_hearing: View<dark::properties::PropAIHearing>| {
                v_hearing.get(id).ok().map(|h| h.rating)
            });

        // Cyber-module award sources (EXP traps carry `PropExp`, EXP-cookie
        // piles carry a stack count), surfaced so automation can read the exact
        // award before firing it. Looked up outside the main run (view limit).
        let exp_value = self
            .world
            .run(|v: View<dark::properties::PropExp>| v.get(id).ok().map(|e| e.0));
        let stack_count = self
            .world
            .run(|v: View<dark::properties::PropStackCount>| v.get(id).ok().map(|s| s.0));
        // Live health is rendered in the HUD target label and is equally
        // useful to deterministic gameplay scenarios (for example proving a
        // real projectile damaged its intended target).
        let hit_points = self
            .world
            .run(|v: View<dark::properties::PropHitPoints>| v.get(id).ok().map(|hp| hp.hit_points));
        let max_hit_points = self
            .world
            .run(|v: View<dark::properties::PropMaxHitPoints>| {
                v.get(id).ok().map(|hp| hp.hit_points)
            });
        let has_refs = self
            .world
            .run(|v: View<PropHasRefs>| v.get(id).ok().map(|refs| refs.0));
        // Most ecologies author no explicit P$EcoState; they behave as Normal
        // until their script's first transition creates the component.
        let ecology_state = self.world.run(
            |v_state: View<dark::properties::PropEcoState>,
             v_ecology: View<dark::properties::PropEcology>| {
                v_state
                    .get(id)
                    .ok()
                    .map(|state| state.0)
                    .or(v_ecology.get(id).ok().map(|_| 0))
            },
        );
        let animated_light = self.world.run(|v: View<PropAnimLight>| {
            v.get(id)
                .ok()
                .map(|light| (light.light_number, light.initial_intensity()))
        });

        self.world.run(
            |v_pos: View<dark::properties::PropPosition>,
             v_sym_name: View<dark::properties::PropSymName>,
             v_scripts: View<dark::properties::PropScripts>,
             v_model_name: View<dark::properties::PropModelName>,
             v_gun_state: View<dark::properties::PropGunState>,
             v_alertness: View<PropAIAlertness>,
             v_ai_behavior: View<RuntimePropAIBehavior>,
             v_awareness: View<crate::runtime_props::RuntimePropAITargetAwareness>,
             v_template_id: View<dark::properties::PropTemplateId>,
             v_links: View<dark::properties::Links>| {
                let position = v_pos.get(id).ok()?;
                let rotation_array = [
                    position.rotation.v.x,
                    position.rotation.v.y,
                    position.rotation.v.z,
                    position.rotation.s,
                ];

                let name = v_sym_name
                    .get(id)
                    .map(|s| s.0.clone())
                    .unwrap_or_else(|_| format!("Entity_{}", id.inner()));

                // The stable template backlink (negative id), not the per-run
                // entity id. 0 if the entity has no template.
                let template_id = v_template_id.get(id).map(|t| t.template_id).unwrap_or(0);

                // Build properties list
                let mut properties = Vec::new();

                // Add position
                properties.push(DebugPropertyInfo {
                    name: "Position".to_string(),
                    value: format!(
                        "[{:.2}, {:.2}, {:.2}]",
                        position.position.x, position.position.y, position.position.z
                    ),
                });

                // Add rotation
                properties.push(DebugPropertyInfo {
                    name: "Rotation".to_string(),
                    value: format!(
                        "[{:.3}, {:.3}, {:.3}, {:.3}]",
                        rotation_array[0], rotation_array[1], rotation_array[2], rotation_array[3]
                    ),
                });

                // Add scripts
                if let Ok(scripts) = v_scripts.get(id) {
                    properties.push(DebugPropertyInfo {
                        name: "Scripts".to_string(),
                        value: scripts.scripts.join(", "),
                    });
                }

                // Cyber-module award amount (EXP trap `PropExp` / EXP-cookie
                // stack count), so tests can read the exact award.
                if let Some(exp) = exp_value {
                    properties.push(DebugPropertyInfo {
                        name: "Exp".to_string(),
                        value: exp.to_string(),
                    });
                }
                if let Some(stack) = stack_count {
                    properties.push(DebugPropertyInfo {
                        name: "StackCount".to_string(),
                        value: stack.to_string(),
                    });
                }
                if let Some(hit_points) = hit_points {
                    properties.push(DebugPropertyInfo {
                        name: "HitPoints".to_string(),
                        value: hit_points.to_string(),
                    });
                }
                if let Some(max_hit_points) = max_hit_points {
                    properties.push(DebugPropertyInfo {
                        name: "MaxHitPoints".to_string(),
                        value: max_hit_points.to_string(),
                    });
                }
                if let Some(has_refs) = has_refs {
                    properties.push(DebugPropertyInfo {
                        name: "HasRefs".to_string(),
                        value: has_refs.to_string(),
                    });
                }
                if let Some(state) = ecology_state {
                    properties.push(DebugPropertyInfo {
                        name: "EcologyState".to_string(),
                        value: match state {
                            0 => "Normal".to_string(),
                            1 => "Hacked".to_string(),
                            2 => "Alert".to_string(),
                            other => format!("Unknown({other})"),
                        },
                    });
                }
                if let Some((light_number, intensity)) = animated_light {
                    properties.push(DebugPropertyInfo {
                        name: "AnimLightNumber".to_string(),
                        value: light_number.to_string(),
                    });
                    properties.push(DebugPropertyInfo {
                        name: "AnimLightIntensity".to_string(),
                        value: format!("{intensity:.3}"),
                    });
                }

                // Add the current render model (e.g. to assert held-model
                // behavior: VR keeps world models, flat swaps to _h viewmodels)
                if let Ok(model_name) = v_model_name.get(id) {
                    properties.push(DebugPropertyInfo {
                        name: "Model".to_string(),
                        value: model_name.0.clone(),
                    });
                }

                // Add weapon ammo (current clip) when present
                if let Ok(gun_state) = v_gun_state.get(id) {
                    properties.push(DebugPropertyInfo {
                        name: "Ammo".to_string(),
                        value: gun_state.ammo.to_string(),
                    });
                }

                // AI state (alertness mirrored by the script, behavior name
                // published for introspection)
                if let Ok(alertness) = v_alertness.get(id) {
                    properties.push(DebugPropertyInfo {
                        name: "AIAlertness".to_string(),
                        value: format!("{:?}", alertness.level),
                    });
                }
                if let Ok(behavior) = v_ai_behavior.get(id) {
                    properties.push(DebugPropertyInfo {
                        name: "AIBehavior".to_string(),
                        value: behavior.0.clone(),
                    });
                }
                // Hearing acuity when authored (0 = deaf, ignores noises)
                if let Some(rating) = hearing_rating {
                    properties.push(DebugPropertyInfo {
                        name: "AIHearing".to_string(),
                        value: rating.to_string(),
                    });
                }
                if let Ok(awareness) = v_awareness.get(id) {
                    properties.push(DebugPropertyInfo {
                        name: "AITargetVisible".to_string(),
                        value: awareness.has_line_of_sight.to_string(),
                    });
                    properties.push(DebugPropertyInfo {
                        name: "AILastKnown".to_string(),
                        value: format!(
                            "[{:.2}, {:.2}, {:.2}]",
                            awareness.last_known_pos.x,
                            awareness.last_known_pos.y,
                            awareness.last_known_pos.z
                        ),
                    });
                }

                let (outgoing_links, incoming_links) =
                    debug_entity_links(id, &v_links, &v_sym_name);
                let contained_by = incoming_links
                    .iter()
                    .find(|link| link.contains_ordinal.is_some())
                    .map(|link| link.target_id);

                Some(DebugEntityDetail {
                    entity_id: id.inner() as i32,
                    name,
                    template_id,
                    position: [
                        position.position.x,
                        position.position.y,
                        position.position.z,
                    ],
                    rotation: rotation_array,
                    inheritance_chain: vec![], // TODO: Implement inheritance chain lookup
                    properties,
                    outgoing_links,
                    incoming_links,
                    contained_by,
                    aim_points: aim_points.clone(),
                })
            },
        )
    }

    fn raycast(
        &self,
        start: cgmath::Point3<f32>,
        end: cgmath::Point3<f32>,
        mask: crate::game_scene::RaycastMask,
    ) -> crate::game_scene::DebugRayHit {
        use crate::game_scene::DebugRayHit;

        // Convert mask to collision groups
        let collision_groups = if mask.groups.contains(&"all".to_string()) {
            crate::physics::InternalCollisionGroups::ALL
        } else {
            let mut groups = crate::physics::InternalCollisionGroups::empty();
            for group_name in &mask.groups {
                match group_name.as_str() {
                    "world" => groups |= crate::physics::InternalCollisionGroups::WORLD,
                    "entity" => groups |= crate::physics::InternalCollisionGroups::ENTITIES,
                    "selectable" => groups |= crate::physics::InternalCollisionGroups::SELECTABLE,
                    "player" => groups |= crate::physics::InternalCollisionGroups::PLAYER,
                    "ui" => groups |= crate::physics::InternalCollisionGroups::UI,
                    "hitbox" => groups |= crate::physics::InternalCollisionGroups::HITBOX,
                    "raycast" => groups |= crate::physics::InternalCollisionGroups::RAYCAST,
                    _ => {} // Ignore unknown groups
                }
            }
            groups
        };

        // Perform raycast using existing physics system
        match self
            .physics
            .ray_cast3(start, end, collision_groups, None, mask.ignore_sensors)
        {
            Some(hit) => {
                let entity_name = hit.maybe_entity_id.and_then(|id| {
                    self.world.run(
                        |v_sym_name: shipyard::View<dark::properties::PropSymName>| {
                            v_sym_name.get(id).ok().map(|s| s.0.clone())
                        },
                    )
                });
                let body_id = hit
                    .maybe_rigid_body_handle
                    .map(|handle| handle.into_raw_parts().0);
                let collision_group = body_id
                    .and_then(|id| self.physics.debug_body_detail(id))
                    .and_then(|detail| detail.collision_groups.into_iter().next());

                DebugRayHit {
                    hit: true,
                    hit_point: Some([hit.hit_point.x, hit.hit_point.y, hit.hit_point.z]),
                    hit_normal: Some([hit.hit_normal.x, hit.hit_normal.y, hit.hit_normal.z]),
                    distance: Some((hit.hit_point - start).magnitude()),
                    entity_id: hit.maybe_entity_id.map(|id| id.inner() as i32),
                    entity_name,
                    body_id,
                    collision_group,
                    is_sensor: hit.is_sensor,
                }
            }
            None => DebugRayHit {
                hit: false,
                hit_point: None,
                hit_normal: None,
                distance: None,
                entity_id: None,
                entity_name: None,
                body_id: None,
                collision_group: None,
                is_sensor: false,
            },
        }
    }

    fn teleport_player(&mut self, position: cgmath::Vector3<f32>) -> Result<(), String> {
        // Apply the teleportation using the same logic as Effect::SetPlayerPosition
        self.physics
            .set_player_translation(position, &mut self.player_handle);

        // Keep PlayerInfo.pos consistent with the body immediately. It otherwise
        // only re-syncs from the physics body on the next update(), so a query
        // right after a teleport - notably the teleport endpoint's own
        // confirmation read - would report the stale pre-teleport position.
        let player_entity = {
            let mut player_info = self.world.borrow::<UniqueViewMut<PlayerInfo>>().unwrap();
            player_info.pos = position;
            player_info.entity_id
        };

        self.world
            .add_component(player_entity, PropTeleported::new());

        Ok(())
    }

    fn move_player(&mut self, target: cgmath::Vector3<f32>) -> crate::physics::MoveResult {
        // Bounded, shape-cast-validated move: physics clamps the displacement
        // and stops short of any geometry it hits.
        let result = self
            .physics
            .move_player_validated(target, &mut self.player_handle);

        if result.moved {
            // Keep PlayerInfo.pos consistent with the body immediately (same
            // reasoning as `teleport_player`), and mark the player teleported so
            // the render/camera path picks up the new position this frame.
            let player_entity = {
                let mut player_info = self.world.borrow::<UniqueViewMut<PlayerInfo>>().unwrap();
                player_info.pos = result.new_position;
                player_info.entity_id
            };

            self.world
                .add_component(player_entity, PropTeleported::new());
        }

        result
    }

    fn player_position(&self) -> cgmath::Vector3<f32> {
        // Get player position from PlayerInfo unique component
        self.world
            .run(|player_info: shipyard::UniqueView<PlayerInfo>| player_info.pos)
    }

    fn list_physics_bodies(
        &self,
        limit: Option<usize>,
    ) -> Vec<crate::game_scene::DebugPhysicsBodySummary> {
        let names = self.entity_names_by_inner();
        let mut bodies: Vec<_> = self
            .physics
            .debug_list_bodies()
            .into_iter()
            .map(|info| {
                let entity_name = info.entity_id.and_then(|id| names.get(&id).cloned());
                crate::game_scene::DebugPhysicsBodySummary {
                    body_id: info.body_id,
                    entity_id: info.entity_id,
                    entity_name,
                    body_type: info.body_type.to_string(),
                    position: info.position,
                    rotation: info.rotation,
                    mass: Some(info.mass),
                    velocity: info.linear_velocity,
                    angular_velocity: info.angular_velocity,
                    collision_groups: info.collision_groups,
                    blocks_player: info.blocks_player,
                    blocks_actor: info.blocks_actor,
                    is_sensor: info.is_sensor,
                    is_enabled: info.is_enabled,
                    is_sleeping: info.is_sleeping,
                }
            })
            .collect();

        if let Some(limit) = limit {
            bodies.truncate(limit);
        }
        bodies
    }

    fn physics_body_detail(
        &self,
        body_id: u32,
    ) -> Option<crate::game_scene::DebugPhysicsBodyDetail> {
        let info = self.physics.debug_body_detail(body_id)?;
        let entity_name = info
            .entity_id
            .and_then(|id| self.entity_names_by_inner().get(&id).cloned());
        Some(crate::game_scene::DebugPhysicsBodyDetail {
            body_id: info.body_id,
            entity_id: info.entity_id,
            entity_name,
            body_type: info.body_type.to_string(),
            position: info.position,
            rotation: info.rotation,
            linear_velocity: info.linear_velocity,
            angular_velocity: info.angular_velocity,
            mass: Some(info.mass),
            center_of_mass: info.center_of_mass,
            // Not yet surfaced from Rapier; velocity is the primary settle signal.
            moment_of_inertia: None,
            gravity_scale: info.gravity_scale,
            linear_damping: info.linear_damping,
            angular_damping: info.angular_damping,
            collision_groups: info.collision_groups,
            blocks_player: info.blocks_player,
            blocks_actor: info.blocks_actor,
            is_sensor: info.is_sensor,
            is_enabled: info.is_enabled,
            is_sleeping: info.is_sleeping,
            contact_count: 0,
        })
    }

    fn ragdoll_metrics(&self) -> Vec<crate::game_scene::DebugRagdollMetrics> {
        self.rag_doll_manager
            .debug_metrics(&self.physics)
            .into_iter()
            .map(|(id, m)| crate::game_scene::DebugRagdollMetrics {
                entity_id: id.inner() as i32,
                body_count: m.body_count,
                max_linear_speed: m.max_linear_speed,
                max_angular_speed: m.max_angular_speed,
                min_y: m.min_y,
                max_nonadjacent_overlap: m.max_nonadjacent_overlap,
                max_drift: m.max_drift,
            })
            .collect()
    }

    fn list_physics_joints(&self) -> Vec<crate::game_scene::DebugPhysicsJoint> {
        let body_to_bone = self.rag_doll_manager.body_to_joint_id();
        self.physics
            .debug_list_joints()
            .into_iter()
            .map(|j| crate::game_scene::DebugPhysicsJoint {
                bone1: body_to_bone.get(&j.body1_id).copied(),
                bone2: body_to_bone.get(&j.body2_id).copied(),
                body1_id: j.body1_id,
                body2_id: j.body2_id,
                joint_type: j.joint_type.to_string(),
                anchor1: j.anchor1,
                anchor2: j.anchor2,
                separation: j.separation,
                linear_impulse: j.linear_impulse,
                angular_impulse: j.angular_impulse,
            })
            .collect()
    }

    fn apply_body_impulse(&mut self, body_id: u32, impulse: [f32; 3]) -> bool {
        self.physics
            .apply_body_impulse(body_id, vec3(impulse[0], impulse[1], impulse[2]))
    }

    fn audit_colliders(&self) -> Vec<crate::game_scene::DebugColliderIssue> {
        let issues = self.physics.audit_colliders();
        if issues.is_empty() {
            return Vec::new();
        }
        // Resolve names via the shared `inner() as i32` map so recycled entities
        // (generation > 0) match the same truncation the physics side used -
        // reconstructing an EntityId with `from_inner` would drop the generation
        // and silently miss those.
        let names = self.entity_names_by_inner();
        issues
            .into_iter()
            .map(|iss| {
                let entity_name = iss.entity_id.and_then(|id| names.get(&id).cloned());
                crate::game_scene::DebugColliderIssue {
                    entity_id: iss.entity_id,
                    entity_name,
                    kind: iss.kind.as_str().to_string(),
                    aabb_min: iss.aabb_min,
                    aabb_max: iss.aabb_max,
                    is_sensor: iss.is_sensor,
                }
            })
            .collect()
    }

    fn ui_state(&self) -> crate::game_scene::DebugUiState {
        let panel_identity = |entity: shipyard::EntityId| {
            let name = self
                .world
                .borrow::<View<dark::properties::PropSymName>>()
                .ok()
                .and_then(|v| v.get(entity).ok().map(|s| s.0.clone()));
            let template_id = self
                .world
                .borrow::<View<dark::properties::PropTemplateId>>()
                .ok()
                .and_then(|v| v.get(entity).ok().map(|t| t.template_id))
                .unwrap_or(0);
            (name, template_id)
        };
        let flat_panel = self.flat_ui.active_panel();
        let active_panel = flat_panel
            .or_else(|| self.gui.active_panel())
            .map(|entity| {
                let (name, template_id) = panel_identity(entity);
                crate::game_scene::DebugUiPanel {
                    entity_id: entity.inner() as i32,
                    template_id,
                    name,
                    elements: if flat_panel == Some(entity) {
                        self.flat_ui.debug_elements(&self.world)
                    } else {
                        self.gui.debug_active_elements(&self.world)
                    },
                }
            });
        // The top-docked inventory strip (Tab metagame mode), same element
        // contract as the MFD panel.
        let strip = self.flat_ui.strip_entity().map(|entity| {
            let (name, template_id) = panel_identity(entity);
            crate::game_scene::DebugUiPanel {
                entity_id: entity.inner() as i32,
                template_id,
                name,
                elements: self.flat_ui.strip_debug_elements(&self.world),
            }
        });
        crate::game_scene::DebugUiState {
            mode: if self.use_mode {
                "use".to_string()
            } else {
                "shooter".to_string()
            },
            active_panel,
            strip,
            cursor: self.flat_ui.cursor_debug(),
            ammo_cycle: self.flat_ui.ammo_cycle_debug(),
            pointer: self.flat_ui.pointer_debug(),
            // The panel a client aims a controller at, reported straight off
            // the anchor that placed it - so a test cannot aim at a placement
            // the interface does not use.
            panel_pose: self.vr_use_mode_pointer.is_some().then(|| {
                let panel = self.vr_use_mode_anchor.panel();
                crate::game_scene::DebugUiPanelPose {
                    center: [panel.center.x, panel.center.y, panel.center.z],
                    rotation: [
                        panel.rotation.v.x,
                        panel.rotation.v.y,
                        panel.rotation.v.z,
                        panel.rotation.s,
                    ],
                    size: [panel.size.x, panel.size.y],
                    canvas: [
                        crate::mission::flat_ui_host::CANVAS_SIZE.x,
                        crate::mission::flat_ui_host::CANVAS_SIZE.y,
                    ],
                }
            }),
        }
    }

    fn quest_bits(&self) -> Vec<crate::game_scene::DebugQuestBit> {
        use dark::properties::QuestBitValue;
        let quests = self.world.borrow::<UniqueView<QuestInfo>>().unwrap();
        let mut bits: Vec<_> = quests
            .quest_bits()
            .into_iter()
            .map(|(name, value)| crate::game_scene::DebugQuestBit {
                name,
                // COMPLETE wins over INCOMPLETE if both bits are somehow set;
                // `bits` preserves the exact value for callers that need it.
                value: if value.contains(QuestBitValue::COMPLETE) {
                    "complete"
                } else if value.contains(QuestBitValue::INCOMPLETE) {
                    "incomplete"
                } else {
                    "unknown"
                }
                .to_string(),
                bits: value.bits(),
            })
            .collect();
        // Stable ordering (HashMap iteration is not deterministic).
        bits.sort_by(|a, b| a.name.cmp(&b.name));
        bits
    }

    fn set_quest_bit(&mut self, name: &str, value: &str) -> Result<(), String> {
        use dark::properties::QuestBitValue;
        let quest_value = match value.to_ascii_lowercase().as_str() {
            "unknown" => QuestBitValue::UNKNOWN,
            "incomplete" => QuestBitValue::INCOMPLETE,
            "complete" => QuestBitValue::COMPLETE,
            other => {
                return Err(format!(
                    "invalid quest value '{}' (expected unknown/incomplete/complete)",
                    other
                ));
            }
        };
        let mut quests = self.world.borrow::<UniqueViewMut<QuestInfo>>().unwrap();
        // Setting "unknown" resets the bit to its pristine (absent) state so it
        // doesn't linger in quest_bits() as a touched-but-unknown entry.
        if quest_value == QuestBitValue::UNKNOWN {
            quests.clear_quest_bit_value(name);
        } else {
            quests.set_quest_bit_value(name, quest_value);
        }
        Ok(())
    }

    fn player_inventory(&self) -> Vec<crate::game_scene::DebugInventoryItem> {
        // Carried items, matching what the save system persists as "held"
        // (save_load::get_held_items): each hand-held entity plus everything
        // nested under it, and the backpack entity's contents - all following
        // `Contains` links to depth 2. A `seen` set dedups items that appear in
        // more than one place, with hands taking precedence over the backpack.
        let (inventory_entity, left_hand, right_hand) = {
            let player = self.world.borrow::<UniqueView<PlayerInfo>>().unwrap();
            (
                player.inventory_entity_id,
                player.left_hand_entity_id,
                player.right_hand_entity_id,
            )
        };

        // Recursively collect `Contains` descendants of `root`, tagging each
        // unique entity with the location it is carried through. Nested
        // immutable `View<Links>` borrows are fine (mirrors get_held_items).
        fn collect(
            world: &shipyard::World,
            entity: shipyard::EntityId,
            depth: u32,
            location: &str,
            seen: &mut std::collections::HashSet<shipyard::EntityId>,
            out: &mut Vec<(shipyard::EntityId, String)>,
        ) {
            if depth == 0 {
                return;
            }
            crate::scripts::script_util::for_each_link(world, entity, &mut |link| {
                if matches!(link.link, dark::properties::Link::Contains(_)) {
                    if let Some(to_ent_id) = link.to_entity_id {
                        let child = to_ent_id.0;
                        if seen.insert(child) {
                            out.push((child, location.to_string()));
                            collect(world, child, depth - 1, location, seen, out);
                        }
                    }
                }
            });
        }

        let mut seen = std::collections::HashSet::new();
        let mut collected: Vec<(shipyard::EntityId, String)> = Vec::new();

        // Hands first (so a held item is attributed to its hand, not the
        // backpack): the hand entity itself is carried, plus its contents.
        for (hand, location) in [(left_hand, "left_hand"), (right_hand, "right_hand")] {
            if let Some(h) = hand {
                if seen.insert(h) {
                    collected.push((h, location.to_string()));
                }
                collect(&self.world, h, 2, location, &mut seen, &mut collected);
            }
        }
        // Backpack: the inventory entity's contents (the container entity itself
        // is not an item, so seed `seen` with it and only collect its children).
        seen.insert(inventory_entity);
        collect(
            &self.world,
            inventory_entity,
            2,
            "inventory",
            &mut seen,
            &mut collected,
        );

        let names = self.entity_names_by_inner();
        let mut items: Vec<_> = collected
            .into_iter()
            .map(|(eid, location)| {
                let id = eid.inner() as i32;
                crate::game_scene::DebugInventoryItem {
                    entity_id: id,
                    name: names.get(&id).cloned(),
                    location,
                }
            })
            .collect();
        // Stable ordering for deterministic snapshots.
        items.sort_by(|a, b| {
            a.location
                .cmp(&b.location)
                .then(a.name.cmp(&b.name))
                .then(a.entity_id.cmp(&b.entity_id))
        });
        items
    }

    fn give_item(&mut self, entity_id: shipyard::EntityId) -> Result<(), String> {
        use shipyard::EntitiesView;
        let is_alive = self
            .world
            .borrow::<EntitiesView>()
            .map(|entities| entities.is_alive(entity_id))
            .unwrap_or(false);
        if !is_alive {
            return Err(format!("entity {:?} is not alive", entity_id));
        }

        // Only genuine pickup items may be given - the same eligibility rule the
        // real grab path uses. This rejects doors, creatures, the player, the
        // inventory container itself, etc., which reparenting into the inventory
        // (dropping their links + unphysicalizing them) would corrupt.
        if !crate::virtual_hand::can_grab_item(&self.world, entity_id) {
            return Err(format!("entity {:?} is not a pickup item", entity_id));
        }

        // This debug lever is only for world-placed items. Moving an authored
        // container's loot straight into the backpack would make
        // `drop_entity_into_container` remove the original Contains link,
        // permanently bypassing (and invalidating tests of) the real loot UI.
        // Reject before mutating anything so the container remains intact.
        let containing_entity = {
            let links = self.world.borrow::<View<Links>>().unwrap();
            links.iter().with_id().find_map(|(container_id, links)| {
                links
                    .to_links
                    .iter()
                    .any(|link| {
                        matches!(link.link, Link::Contains(_))
                            && link.to_entity_id.map(|wrapped| wrapped.0) == Some(entity_id)
                    })
                    .then_some(container_id)
            })
        };
        if let Some(container_id) = containing_entity {
            return Err(format!(
                "entity {} is container-held by entity {}; loot it via the container MFD",
                entity_id.inner(),
                container_id.inner()
            ));
        }

        let inventory_entity = {
            let player = self.world.borrow::<UniqueView<PlayerInfo>>().unwrap();
            player.inventory_entity_id
        };
        if self.drop_entity_into_container(inventory_entity, entity_id) {
            Ok(())
        } else {
            Err("could not add item to inventory (no inventory container)".to_string())
        }
    }

    fn spawn_item_for_player(
        &mut self,
        asset_cache: &mut AssetCache,
        template: &crate::game_scene::DebugItemTemplate,
    ) -> Result<crate::game_scene::DebugSpawnedItem, String> {
        use crate::game_scene::{DebugItemTemplate, DebugSpawnedItem};

        let template_id = match template {
            DebugItemTemplate::Id(id) => *id,
            DebugItemTemplate::Name(name) => {
                self.template_name_to_template_id
                    .get(&name.to_ascii_lowercase())
                    .ok_or_else(|| format!("no template named '{}'", name))?
                    .template_id
            }
        };
        // Gamesys templates only (the negative id space). Positive ids address
        // *this mission's* authored objects, and duplicating one would hand the
        // player a second copy of a unique quest item (a keycard, a log) - a way
        // to fake progress rather than to provision a loadout.
        if template_id >= 0 {
            return Err(format!(
                "template id {} is a mission object; provisioning takes gamesys templates (negative ids)",
                template_id
            ));
        }
        if !self
            .entity_info
            .entity_to_properties
            .contains_key(&template_id)
        {
            return Err(format!("unknown template id {}", template_id));
        }

        // Spawn at the player's position: containment immediately makes the
        // item non-physical, so the spawn point is never observed - it just has
        // to be somewhere valid for instantiation.
        let (position, rotation) = {
            let player = self.world.borrow::<UniqueView<PlayerInfo>>().unwrap();
            (vec3_to_point3(player.pos), player.rotation)
        };
        let info = self.create_entity_with_position(
            asset_cache,
            template_id,
            position,
            rotation,
            Matrix4::identity(),
            CreateEntityOptions::default(),
        );
        let entity_id = info.entity_id;

        // Route the fresh entity through the very same pickup path as an item
        // found in the world, so provisioning inherits its eligibility rule:
        // only genuine pickup items land in the backpack. A creature or door
        // template is rejected here - and destroyed again, so a refused request
        // leaves nothing behind.
        if let Err(e) = self.give_item(entity_id) {
            self.destroy_entity(entity_id);
            // Report the template, not the entity: the instance is gone, and the
            // caller never saw its id.
            return Err(format!(
                "template {} is not a pickup item ({})",
                template_id, e
            ));
        }

        let name = self
            .world
            .borrow::<View<dark::properties::PropSymName>>()
            .ok()
            .and_then(|v| v.get(entity_id).ok().map(|s| s.0.clone()));
        Ok(DebugSpawnedItem {
            entity_id: entity_id.inner() as i32,
            template_id,
            name,
        })
    }

    fn set_player_stats(
        &mut self,
        request: &crate::game_scene::DebugPlayerStatsRequest,
    ) -> Result<crate::player_stats::PlayerStats, String> {
        use crate::player_stats::{Skill, Stat};
        use crate::scripts::gui::{
            PSI_TIER_CAP, SKILL_CAP, STAT_CAP, TrainerTarget, apply_purchase,
        };

        let mut quests = self
            .world
            .borrow::<UniqueViewMut<QuestInfo>>()
            .map_err(|_| "scene has no quest info".to_string())?;
        let stats = quests.player_stats_mut();
        let old_backpack_width = crate::inventory::backpack_width(stats);

        let stat_targets = [
            (Stat::Strength, request.strength, "strength"),
            (Stat::Endurance, request.endurance, "endurance"),
            (Stat::Agility, request.agility, "agility"),
            (
                Stat::PsionicAbility,
                request.psionic_ability,
                "psionic_ability",
            ),
            (
                Stat::CyberAffinity,
                request.cyber_affinity,
                "cyber_affinity",
            ),
        ];
        let skill_targets = [
            (
                Skill::StandardWeapons,
                request.skills.standard_weapons,
                "standard_weapons",
            ),
            (
                Skill::EnergyWeapons,
                request.skills.energy_weapons,
                "energy_weapons",
            ),
            (
                Skill::HeavyWeapons,
                request.skills.heavy_weapons,
                "heavy_weapons",
            ),
            (
                Skill::ExoticWeapons,
                request.skills.exotic_weapons,
                "exotic_weapons",
            ),
            (Skill::Hack, request.skills.hack, "hack"),
            (Skill::Repair, request.skills.repair, "repair"),
            (Skill::Modify, request.skills.modify, "modify"),
            (
                Skill::Maintenance,
                request.skills.maintenance,
                "maintenance",
            ),
            (Skill::Research, request.skills.research, "research"),
        ];

        // Validate everything before mutating anything, so a rejected request
        // never leaves the character sheet half-provisioned.
        let check = |field: &str, target: i32, current: i32, cap: i32| -> Result<(), String> {
            if target < current {
                return Err(format!(
                    "cannot lower {} from {} to {} (provisioning only raises)",
                    field, current, target
                ));
            }
            if target > cap {
                return Err(format!(
                    "{} maxes out at {} (asked for {})",
                    field, cap, target
                ));
            }
            Ok(())
        };
        for (stat, target, field) in stat_targets {
            if let Some(target) = target {
                check(field, target, stats.stat_level(stat), STAT_CAP)?;
            }
        }
        for (skill, target, field) in skill_targets {
            if let Some(target) = target {
                check(field, target, stats.skill_level(skill), SKILL_CAP)?;
            }
        }
        if let Some(target) = request.psi_tier {
            check("psi_tier", target, stats.psi_tier, PSI_TIER_CAP)?;
        }
        if let Some(target) = request.cyber_modules {
            // No cap on the currency, so only the "raises only" half applies.
            if target < stats.cyber_modules {
                return Err(format!(
                    "cannot lower cyber_modules from {} to {} (provisioning only raises)",
                    stats.cyber_modules, target
                ));
            }
        }

        // Apply through the same `PlayerStats` mutations a trainer purchase
        // performs, one level at a time - just without the module cost.
        for (stat, target, _) in stat_targets {
            if let Some(target) = target {
                while stats.stat_level(stat) < target {
                    stats.raise_stat(stat);
                }
            }
        }
        for (skill, target, _) in skill_targets {
            if let Some(target) = target {
                while stats.skill_level(skill) < target {
                    stats.raise_skill(skill);
                }
            }
        }
        if let Some(target) = request.psi_tier {
            // Tiers unlock sequentially, exactly as the psi trainer sells them.
            for tier in (stats.psi_tier + 1)..=target {
                apply_purchase(stats, TrainerTarget::PsiTier(tier));
            }
        }
        if let Some(target) = request.cyber_modules {
            stats.award_cyber_modules(target.saturating_sub(stats.cyber_modules));
        }

        let new_backpack_width = crate::inventory::backpack_width(stats);
        let result = stats.clone();
        drop(quests);
        self.resize_player_backpack(old_backpack_width, new_backpack_width);
        info!("Debug provisioning set player stats: {:?}", result);
        Ok(result)
    }

    fn list_transitions(&self) -> Vec<crate::game_scene::DebugTransition> {
        use dark::properties::{PropDestLevel, PropDestLoc, PropPosition, PropSymName};
        use shipyard::Get;
        // `PropDestLevel` is a live component on each transition trigger (the
        // trigger script reads it the same way), so we can list every trigger
        // with where it leads and its volume position.
        self.world.run(
            |v_dest: View<PropDestLevel>,
             v_loc: View<PropDestLoc>,
             v_pos: View<PropPosition>,
             v_sym: View<PropSymName>| {
                let mut out = Vec::new();
                for (id, dest) in v_dest.iter().with_id() {
                    let position = v_pos
                        .get(id)
                        .map(|p| [p.position.x, p.position.y, p.position.z])
                        .unwrap_or([0.0, 0.0, 0.0]);
                    out.push(crate::game_scene::DebugTransition {
                        entity_id: id.inner() as i32,
                        name: v_sym.get(id).ok().map(|s| s.0.clone()),
                        dest_level: dest.0.clone(),
                        dest_loc: v_loc.get(id).ok().map(|l| l.0),
                        position,
                    });
                }
                out
            },
        )
    }

    fn get_input_state(&self) -> crate::input_context::InputContext {
        // MissionCore doesn't store InputContext directly - it's passed to update()
        // For debugging purposes, return a default state
        tracing::warn!("get_input_state called on MissionCore - returning default state");
        crate::input_context::InputContext::default()
    }

    fn set_input(&mut self, channel: &str, _value: serde_json::Value) -> bool {
        // MissionCore doesn't manage InputContext directly - this needs to be handled
        // at the debug runtime level where InputContext is maintained
        tracing::warn!(
            "set_input called on MissionCore for channel '{}' - not implemented",
            channel
        );
        false
    }

    fn pathfinding_test_status(&self) -> crate::game_scene::DebugPathfindingTestStatus {
        use crate::mission::pathfinding_test::PathfindingTestState;

        let state = match self.pathfinding_test.state {
            PathfindingTestState::WaitingForStart => "WaitingForStart",
            PathfindingTestState::WaitingForGoal => "WaitingForGoal",
            PathfindingTestState::ShowingPath => "ShowingPath",
        };
        let test_path_waypoints = self
            .path_visualization
            .paths
            .get("test_path")
            .map(|path| path.waypoints.len())
            .unwrap_or(0);

        crate::game_scene::DebugPathfindingTestStatus {
            state: state.to_string(),
            test_path_waypoints,
        }
    }

    fn pathfinding_stats(&self) -> Option<crate::game_scene::DebugPathfindingStats> {
        self.pathfinding_service.as_ref().map(|service| {
            let stats = service.stats();
            crate::game_scene::DebugPathfindingStats {
                queries: stats.queries,
                stressed_retries: stats.stressed_retries,
                no_route: stats.no_route,
            }
        })
    }

    fn ai_paths(&self) -> Vec<crate::game_scene::DebugAiPathEntry> {
        let Some(service) = self.pathfinding_service.as_ref() else {
            return Vec::new();
        };
        service
            .ai_paths()
            .into_iter()
            .map(|(entity, record)| {
                let live = service.ai_steering(entity);
                crate::game_scene::DebugAiPathEntry {
                    entity_id: entity as i32,
                    goal: record.goal.into(),
                    outcome: format!("{:?}", record.outcome),
                    waypoints: record.waypoints.into_iter().map(Into::into).collect(),
                    live_next_waypoint: live.map(|l| l.next_waypoint),
                    live_path_len: live.map(|l| l.path_len),
                    live_target: live.and_then(|l| l.target).map(Into::into),
                    live_stall_seconds: live.map(|l| l.stall_seconds),
                }
            })
            .collect()
    }

    fn send_entity_message(
        &mut self,
        id: EntityId,
        message: crate::game_scene::DebugEntityMessage,
    ) -> bool {
        use crate::game_scene::DebugEntityMessage;
        use shipyard::EntitiesView;

        let is_alive = self
            .world
            .borrow::<EntitiesView>()
            .map(|entities| entities.is_alive(id))
            .unwrap_or(false);

        if !is_alive {
            tracing::warn!("send_entity_message: entity {:?} is not alive", id);
            return false;
        }

        let payload = match message {
            DebugEntityMessage::Damage {
                amount,
                direction,
                point,
            } => MessagePayload::Damage {
                amount,
                // A directional debug blow mirrors what a projectile hit
                // carries (point falls back to the victim's position).
                impact: direction.and_then(|d| {
                    let d = vec3(d[0], d[1], d[2]);
                    if d.magnitude2() <= 1.0e-12 {
                        return None;
                    }
                    let point = point.map(|p| vec3(p[0], p[1], p[2])).or_else(|| {
                        self.world
                            .borrow::<View<RuntimePropTransform>>()
                            .ok()
                            .and_then(|v| {
                                v.get(id)
                                    .ok()
                                    .map(|t| crate::util::get_position_from_matrix(&t.0))
                            })
                            .map(|p| vec3(p.x, p.y, p.z))
                    })?;
                    Some(crate::scripts::DamageImpact {
                        direction: d.normalize(),
                        point,
                        bone: None,
                    })
                }),
            },
            DebugEntityMessage::Frob => MessagePayload::Frob,
            DebugEntityMessage::Signal { name } => MessagePayload::Signal { name },
            DebugEntityMessage::SetAlertness { level } => {
                MessagePayload::SetAlertness { level, pin: false }
            }
            // Debug injections have no real sender; use the target itself.
            DebugEntityMessage::TurnOn => MessagePayload::TurnOn { from: id },
            DebugEntityMessage::TurnOff => MessagePayload::TurnOff { from: id },
        };

        self.script_world.dispatch(Message { to: id, payload });
        true
    }
}

#[cfg(test)]
mod log_reader_art_tests {
    use std::{cell::RefCell, collections::HashSet, io::Cursor};

    use engine::assets::asset_paths::{AbstractAssetPath, ReadableAndSeekable};

    use super::*;

    struct FakeAssetPath(HashSet<String>);

    impl AbstractAssetPath for FakeAssetPath {
        fn exists(&self, _base_path: String, asset_name: String) -> bool {
            self.0.contains(&asset_name)
        }

        fn get_reader(
            &self,
            _base_path: String,
            asset_name: String,
        ) -> Option<RefCell<Box<dyn ReadableAndSeekable>>> {
            self.exists(String::new(), asset_name)
                .then(|| RefCell::new(Box::new(Cursor::new(Vec::new())) as _))
        }
    }

    fn cache(names: &[&str]) -> AssetCache {
        AssetCache::new(
            String::new(),
            Box::new(FakeAssetPath(
                names.iter().map(|name| (*name).to_owned()).collect(),
            )),
        )
    }

    #[test]
    fn optional_log_art_resolves_alternate_encoding_and_omits_missing_keys() {
        let assets = cache(&["ramsicon.png", "ramsicon.pcx", "bayliss.pcx"]);

        assert_eq!(
            resolve_optional_log_art_texture(
                &assets,
                1,
                24,
                "deck icon",
                Some("RamsIcon".to_owned()),
            ),
            Some("ramsicon.png".to_owned()),
            "the upgraded encoding must win over a same-mount legacy PCX"
        );
        assert_eq!(
            resolve_optional_log_art_texture(
                &assets,
                1,
                24,
                "portrait",
                Some("Bayliss".to_owned()),
            ),
            Some("bayliss.pcx".to_owned())
        );
        assert_eq!(
            resolve_optional_log_art_texture(
                &assets,
                1,
                24,
                "deck icon",
                Some("NotShipped".to_owned()),
            ),
            None,
            "missing optional art must never reach the infallible UI renderer"
        );
        assert_eq!(
            resolve_optional_log_art_texture(&assets, 1, 24, "portrait", Some("  ".to_owned()),),
            None
        );
    }
}

#[cfg(test)]
mod saved_script_namespace_tests {
    use super::*;

    const STATE_KEY: &str = "test.saved-namespace";

    struct NamespaceStateScript(u32);

    impl scripts::Script for NamespaceStateScript {
        fn script_state_key(&self) -> Option<&'static str> {
            Some(STATE_KEY)
        }

        fn save_state(&self) -> Result<scripts::ScriptState, scripts::ScriptStateError> {
            scripts::ScriptState::encode(1, &self.0, STATE_KEY)
        }

        fn restore_state(
            &mut self,
            state: &scripts::ScriptState,
            _context: &scripts::ScriptRestoreContext<'_>,
        ) -> Result<(), scripts::ScriptStateError> {
            self.0 = state.decode(1, STATE_KEY)?;
            Ok(())
        }
    }

    fn source_state(value: u32) -> (EntityId, Vec<scripts::SavedScriptState>) {
        let mut world = World::new();
        let old_entity = world.add_entity(());
        let mut scripts = ScriptWorld::new();
        scripts.add_entity2(old_entity, Box::new(NamespaceStateScript(value)));
        (old_entity, scripts.save_states().unwrap())
    }

    fn saved_value(states: &[scripts::SavedScriptState], entity: EntityId) -> u32 {
        states
            .iter()
            .find(|state| state.entity_id == entity.inner())
            .unwrap()
            .state
            .decode(1, STATE_KEY)
            .unwrap()
    }

    #[test]
    fn overlapping_saved_ids_restore_in_their_own_namespaces() {
        // Mission and inventory snapshots come from separate ECS worlds, where
        // each allocator can legitimately assign the same saved EntityId.
        let (old_mission, mission_states) = source_state(11);
        let (old_held, held_states) = source_state(22);
        assert_eq!(
            old_mission, old_held,
            "the regression needs an exact ID overlap"
        );

        let mut loaded_world = World::new();
        let new_mission = loaded_world.add_entity(());
        let new_held = loaded_world.add_entity(());
        let mut loaded_scripts = ScriptWorld::new();
        loaded_scripts.add_entity2(new_mission, Box::new(NamespaceStateScript(0)));
        loaded_scripts.add_entity2(new_held, Box::new(NamespaceStateScript(0)));

        restore_saved_script_namespaces(
            &mut loaded_scripts,
            &mission_states,
            &HashMap::from([(old_mission, new_mission)]),
            &held_states,
            &HashMap::from([(old_held, new_held)]),
        );

        let restored = loaded_scripts.save_states().unwrap();
        assert_eq!(saved_value(&restored, new_mission), 11);
        assert_eq!(saved_value(&restored, new_held), 22);
    }
}

#[cfg(test)]
mod held_item_restore_tests {
    use super::*;

    /// #787: flat mode only has one wield slot, so restoring a VR save with
    /// two held items must retain the first grab's displacement effects. The
    /// load path applies this batch after the backpack is available.
    #[test]
    fn flat_two_hand_restore_returns_effect_that_stores_displaced_item() {
        let mut world = World::new();
        let left = world.add_entity(());
        let right = world.add_entity(());
        let mut interaction = FlatInteraction::new();

        let effects =
            restore_held_item_interaction(&mut interaction, &world, Some(left), Some(right));

        assert_eq!(interaction.held_entities(), (Some(right), None));
        assert!(
            effects.iter().any(|effect| matches!(
                effect,
                VirtualHandEffect::StoreItem { entity_id } if *entity_id == left
            )),
            "the displaced left-hand item must be returned to the backpack, got {effects:?}"
        );
    }
}

#[cfg(test)]
mod mission_object_name_map_tests {
    use dark::properties::PropSymName;

    use super::*;

    fn named(entity_info: &mut SystemShock2EntityInfo, id: i32, name: &str) {
        entity_info
            .entity_to_properties
            .insert(id, vec![Arc::new(Box::new(PropSymName(name.to_owned())))]);
    }

    #[test]
    fn indexes_only_uniquely_named_mission_objects() {
        let mut entity_info = SystemShock2EntityInfo::empty();
        // The authored spawn archetype: a mission object with its own name.
        named(&mut entity_info, 597, "DopeyDroid");
        // Gamesys templates are resolved through the gamesys map instead.
        named(&mut entity_info, -4015, "Training Droid");
        // Two objects sharing a name cannot pick out an archetype.
        named(&mut entity_info, 100, "Marker");
        named(&mut entity_info, 101, "Marker");

        let map = create_mission_object_name_map(&entity_info);

        assert_eq!(map.get("dopeydroid"), Some(&597));
        assert_eq!(map.get("training droid"), None);
        assert_eq!(map.get("marker"), None);
    }
}

#[cfg(test)]
mod death_motion_tests {
    use super::is_realtime_crumple;

    #[test]
    fn three_frame_already_dead_pose_is_not_a_realtime_crumple() {
        assert!(!is_realtime_crumple(3.0));
        assert!(is_realtime_crumple(79.0));
    }
}

#[cfg(test)]
mod player_death_tests {
    use dark::properties::{PropTweqModelConfig, TweqAnimationConfig, TweqHalt};

    use super::*;

    fn resurrection_world(model: &str) -> (World, Vector3<f32>, Quaternion<f32>) {
        let mut world = World::new();
        let target_position = vec3(4.0, 5.0, 6.0);
        let target_rotation = Quaternion::from_angle_y(cgmath::Deg(90.0));
        let target = world.add_entity((
            PropScripts {
                scripts: vec!["TrapTeleport".to_owned()],
                inherits: true,
            },
            PropPosition {
                position: target_position,
                rotation: target_rotation,
                cell: 0,
            },
        ));
        world.add_entity((
            PropScripts {
                scripts: vec!["ResurrectMachine".to_owned(), "Tweqable".to_owned()],
                inherits: true,
            },
            PropModelName(model.to_owned()),
            PropTweqModelConfig {
                animation_config: TweqAnimationConfig::SIM,
                halt: TweqHalt::StopTweq,
                model_names: vec!["res_pad".to_owned(), "res_pad2".to_owned()],
            },
            Links {
                to_links: vec![ToLink {
                    to_template_id: 0,
                    to_entity_id: Some(WrappedEntityId(target)),
                    link: Link::SwitchLink,
                }],
            },
        ));
        (world, target_position, target_rotation)
    }

    #[test]
    fn inactive_resurrection_scanner_has_no_respawn_target() {
        let (world, _, _) = resurrection_world("res_pad");

        assert_eq!(active_resurrection_target(&world), None);
    }

    #[test]
    fn activated_scanner_resolves_its_linked_teleport_target() {
        let (world, expected_position, expected_rotation) = resurrection_world("res_pad2");

        assert_eq!(
            active_resurrection_target(&world),
            Some((expected_position, expected_rotation))
        );
    }
}

#[cfg(test)]
mod vr_squeeze_swallow_tests {
    use super::update_squeeze_swallow;

    const LEFT: usize = 0;
    const RIGHT: usize = 1;

    /// The whole point of the latch: a squeeze that started on the panel keeps
    /// being swallowed after the ray leaves it (and after the interface
    /// closes), so it can never reach through into a world grab. Without the
    /// latch the mask is recomputed per frame and vanishes the instant the
    /// claim does - while `VirtualHand` grabs on the squeeze *level*.
    #[test]
    fn a_squeeze_begun_on_the_panel_never_reaches_the_world() {
        let mut latched = [false; 2];
        // On the panel, hand empty, squeezing: claimed.
        update_squeeze_swallow(&mut latched, [true, false], [true, true], [true, false]);
        assert!(latched[LEFT]);
        // Ray slips off the panel - still swallowed.
        update_squeeze_swallow(&mut latched, [true, false], [true, true], [false, false]);
        assert!(latched[LEFT]);
        // The interface closes (nothing claims anything) - still swallowed.
        update_squeeze_swallow(&mut latched, [true, false], [true, true], [false, false]);
        assert!(latched[LEFT]);
        // Released at last: the next squeeze is the player's own again.
        update_squeeze_swallow(&mut latched, [false, false], [true, true], [false, false]);
        assert!(!latched[LEFT]);
        update_squeeze_swallow(&mut latched, [true, false], [true, true], [false, false]);
        assert!(
            !latched[LEFT],
            "a squeeze begun off the panel is the world's"
        );
    }

    /// A hand that just took an item off the panel stops being masked, or the
    /// mask would read as a release and drop what it just took.
    #[test]
    fn taking_an_item_hands_the_squeeze_back() {
        let mut latched = [false; 2];
        update_squeeze_swallow(&mut latched, [false, true], [true, true], [false, true]);
        assert!(latched[RIGHT]);
        // The grab landed: the hand is no longer empty, squeeze still held.
        update_squeeze_swallow(&mut latched, [false, true], [true, false], [false, true]);
        assert!(
            !latched[RIGHT],
            "a hand holding something must see its own squeeze"
        );
    }

    /// Each hand latches on its own: the off-panel hand keeps world grabbing.
    #[test]
    fn the_off_panel_hand_is_untouched() {
        let mut latched = [false; 2];
        update_squeeze_swallow(&mut latched, [true, true], [true, true], [true, false]);
        assert_eq!(latched, [true, false]);
    }
}

#[cfg(test)]
mod psi_kit_use_tests {
    use dark::properties::{PropPsiState, PropStackCount};
    use shipyard::{EntitiesView, Get, View};

    use super::*;

    fn world_with_player(psi_points: i32) -> World {
        let mut world = World::new();
        let inventory_entity_id = world.add_entity(Links::empty());
        let player = world.add_entity(PropPsiState {
            psi_points,
            max_psi_points: 50,
            unknown: 50,
        });
        world.add_unique(PlayerInfo {
            pos: vec3(0.0, 0.0, 0.0),
            rotation: Quaternion::new(1.0, 0.0, 0.0, 0.0),
            entity_id: player,
            left_hand_entity_id: None,
            right_hand_entity_id: None,
            inventory_entity_id,
        });
        world
    }

    fn apply_effect_batch(world: &mut World, effects: Vec<Effect>) {
        for effect in effects {
            match effect {
                Effect::UsePsiKit { entity_id, amount } => {
                    if apply_psi_kit_use(world, entity_id, amount)
                        == PsiKitUseOutcome::DestroyEntity
                    {
                        // Production performs MissionCore's canonical teardown here;
                        // this state-level regression has no render/physics/script maps.
                        world.delete_entity(entity_id);
                    }
                }
                Effect::DropEntityInfo {
                    parent_entity_id,
                    dropped_entity_id,
                } => {
                    move_live_entity_into_container(world, parent_entity_id, dropped_entity_id);
                }
                other => panic!("unexpected effect in psi-kit test batch: {other:?}"),
            }
        }
    }

    fn player_psi_points(world: &World) -> i32 {
        let player = world.borrow::<UniqueView<PlayerInfo>>().unwrap().entity_id;
        world
            .borrow::<View<PropPsiState>>()
            .unwrap()
            .get(player)
            .unwrap()
            .psi_points
    }

    fn inventory_contains(world: &World, entity_id: EntityId) -> bool {
        let inventory = world
            .borrow::<UniqueView<PlayerInfo>>()
            .unwrap()
            .inventory_entity_id;
        world
            .borrow::<View<Links>>()
            .unwrap()
            .get(inventory)
            .unwrap()
            .to_links
            .iter()
            .any(|link| {
                matches!(link.link, Link::Contains(_))
                    && link.to_entity_id.map(|wrapped| wrapped.0) == Some(entity_id)
            })
    }

    #[test]
    fn world_frob_psi_kit_batch_ignores_pickup_after_consumption() {
        let mut world = world_with_player(5);
        let inventory = world
            .borrow::<UniqueView<PlayerInfo>>()
            .unwrap()
            .inventory_entity_id;
        let booster = world.add_entity(PropStackCount(1));

        // One world Frob is handled by both PsiKitScript and the engine's
        // internal MOVE script, in this order, before either effect is applied.
        apply_effect_batch(
            &mut world,
            vec![
                Effect::UsePsiKit {
                    entity_id: booster,
                    amount: 20,
                },
                Effect::DropEntityInfo {
                    parent_entity_id: inventory,
                    dropped_entity_id: booster,
                },
            ],
        );

        assert_eq!(player_psi_points(&world), 25);
        assert!(!world.borrow::<EntitiesView>().unwrap().is_alive(booster));
        assert!(!inventory_contains(&world, booster));
    }

    #[test]
    fn live_drop_entity_info_still_transfers_into_the_inventory() {
        let mut world = world_with_player(5);
        let inventory = world
            .borrow::<UniqueView<PlayerInfo>>()
            .unwrap()
            .inventory_entity_id;
        let booster = world.add_entity((PropStackCount(1), PropHasRefs(true)));

        apply_effect_batch(
            &mut world,
            vec![Effect::DropEntityInfo {
                parent_entity_id: inventory,
                dropped_entity_id: booster,
            }],
        );

        assert!(world.borrow::<EntitiesView>().unwrap().is_alive(booster));
        assert!(inventory_contains(&world, booster));
        assert_eq!(
            world
                .borrow::<View<PropHasRefs>>()
                .unwrap()
                .get(booster)
                .unwrap()
                .0,
            false
        );
    }

    #[test]
    fn two_same_update_psi_kit_effects_restore_against_live_state_and_consume_both() {
        let mut world = world_with_player(5);
        let first = world.add_entity(PropStackCount(1));
        let second = world.add_entity(PropStackCount(1));

        // Both effects represent Frobs dispatched before one handle_effects
        // pass. Applying them as one batch must not reuse a stale psi snapshot.
        apply_effect_batch(
            &mut world,
            vec![
                Effect::UsePsiKit {
                    entity_id: first,
                    amount: 20,
                },
                Effect::UsePsiKit {
                    entity_id: second,
                    amount: 20,
                },
            ],
        );

        assert_eq!(player_psi_points(&world), 45);
        let entities = world.borrow::<EntitiesView>().unwrap();
        assert!(!entities.is_alive(first));
        assert!(!entities.is_alive(second));
    }

    #[test]
    fn psi_kit_clamps_to_max_and_consumes_only_one_stack_unit() {
        let mut world = world_with_player(45);
        let booster = world.add_entity(PropStackCount(2));

        assert_eq!(
            apply_psi_kit_use(&world, booster, 20),
            PsiKitUseOutcome::DecrementedStack
        );
        assert_eq!(player_psi_points(&world), 50);
        assert_eq!(
            world
                .borrow::<View<PropStackCount>>()
                .unwrap()
                .get(booster)
                .unwrap()
                .0,
            1
        );
    }

    #[test]
    fn psi_kit_at_full_pool_leaves_the_source_untouched() {
        let mut world = world_with_player(50);
        let booster = world.add_entity(PropStackCount(1));

        assert_eq!(
            apply_psi_kit_use(&world, booster, 20),
            PsiKitUseOutcome::NotUsed
        );
        assert_eq!(player_psi_points(&world), 50);
        assert!(world.borrow::<EntitiesView>().unwrap().is_alive(booster));
        assert_eq!(
            world
                .borrow::<View<PropStackCount>>()
                .unwrap()
                .get(booster)
                .unwrap()
                .0,
            1
        );
    }
}

#[cfg(test)]
mod comestible_use_tests {
    use dark::properties::{Link, Links, PropHitPoints, PropMaxHitPoints, ToLink, WrappedEntityId};
    use shipyard::{EntitiesView, Get, View};

    use super::*;

    fn world_with_player(hit_points: i32, carried: bool) -> (World, EntityId, EntityId) {
        let mut world = World::new();
        let food = world.add_entity(());
        let inventory = world.add_entity(if carried {
            Links {
                to_links: vec![ToLink {
                    to_template_id: 0,
                    to_entity_id: Some(WrappedEntityId(food)),
                    link: Link::Contains(0),
                }],
            }
        } else {
            Links::empty()
        });
        let player = world.add_entity((
            PropHitPoints { hit_points },
            PropMaxHitPoints { hit_points: 30 },
        ));
        world.add_unique(PlayerInfo {
            pos: vec3(0.0, 0.0, 0.0),
            rotation: Quaternion::new(1.0, 0.0, 0.0, 0.0),
            entity_id: player,
            left_hand_entity_id: None,
            right_hand_entity_id: None,
            inventory_entity_id: inventory,
        });
        (world, player, food)
    }

    fn player_hit_points(world: &World, player: EntityId) -> i32 {
        world
            .borrow::<View<PropHitPoints>>()
            .unwrap()
            .get(player)
            .unwrap()
            .hit_points
    }

    #[test]
    fn carried_comestible_heals_one_and_is_marked_consumed() {
        let (world, player, food) = world_with_player(20, true);

        assert_eq!(
            apply_comestible_use(&world, food, 1),
            ComestibleUseOutcome::Consumed
        );
        assert_eq!(player_hit_points(&world, player), 21);
    }

    #[test]
    fn full_health_still_consumes_without_overhealing() {
        let (world, player, food) = world_with_player(30, true);

        assert_eq!(
            apply_comestible_use(&world, food, 1),
            ComestibleUseOutcome::Consumed
        );
        assert_eq!(player_hit_points(&world, player), 30);
    }

    #[test]
    fn already_destroyed_source_cannot_heal_twice() {
        let (mut world, player, food) = world_with_player(20, true);

        assert_eq!(
            apply_comestible_use(&world, food, 1),
            ComestibleUseOutcome::Consumed
        );
        world.delete_entity(food);
        assert_eq!(
            apply_comestible_use(&world, food, 1),
            ComestibleUseOutcome::NotUsed
        );
        assert_eq!(player_hit_points(&world, player), 21);
        assert!(!world.borrow::<EntitiesView>().unwrap().is_alive(food));
    }

    #[test]
    fn world_object_cannot_bypass_inventory_use() {
        let (world, player, food) = world_with_player(20, false);

        assert_eq!(
            apply_comestible_use(&world, food, 1),
            ComestibleUseOutcome::NotUsed
        );
        assert_eq!(player_hit_points(&world, player), 20);
    }
}

#[cfg(test)]
mod healing_item_use_tests {
    use dark::properties::{
        Link, Links, PropHitPoints, PropMaxHitPoints, PropStackCount, ToLink, WrappedEntityId,
    };
    use shipyard::{EntitiesView, Get, UniqueViewMut, View};

    use super::*;
    use crate::scripts::healing_item::{ActiveHealing, tick_player_healing};

    fn world_with_player(
        hit_points: i32,
        stack: i32,
        carried: bool,
    ) -> (World, EntityId, EntityId) {
        let mut world = World::new();
        let patch = world.add_entity(PropStackCount(stack));
        let inventory = world.add_entity(if carried {
            Links {
                to_links: vec![ToLink {
                    to_template_id: 0,
                    to_entity_id: Some(WrappedEntityId(patch)),
                    link: Link::Contains(0),
                }],
            }
        } else {
            Links::empty()
        });
        let player = world.add_entity((
            PropHitPoints { hit_points },
            PropMaxHitPoints { hit_points: 30 },
        ));
        world.add_unique(PlayerInfo {
            pos: vec3(0.0, 0.0, 0.0),
            rotation: Quaternion::new(1.0, 0.0, 0.0, 0.0),
            entity_id: player,
            left_hand_entity_id: None,
            right_hand_entity_id: None,
            inventory_entity_id: inventory,
        });
        world.add_unique(ActiveHealing::default());
        (world, player, patch)
    }

    #[test]
    fn damaged_player_queues_course_and_consumes_one_stack_unit() {
        let (world, player, patch) = world_with_player(8, 2, true);

        assert_eq!(
            apply_healing_item_use(&world, patch, 10, 2, 0.1, 1.5),
            HealingItemUseOutcome::DecrementedStack
        );
        assert_eq!(
            world
                .borrow::<View<PropStackCount>>()
                .unwrap()
                .get(patch)
                .unwrap()
                .0,
            1
        );
        assert!(matches!(
            tick_player_healing(&world, 0.1),
            Some(Effect::AdjustHitPoints {
                entity_id,
                delta: 2
            }) if entity_id == player
        ));
        assert_eq!(
            world
                .borrow::<View<PropHitPoints>>()
                .unwrap()
                .get(player)
                .unwrap()
                .hit_points,
            8,
            "timed healing must not bypass the central effect handler"
        );
    }

    #[test]
    fn full_health_refuses_without_consuming_or_queuing() {
        let (world, _player, patch) = world_with_player(30, 1, true);

        assert_eq!(
            apply_healing_item_use(&world, patch, 10, 2, 0.1, 1.5),
            HealingItemUseOutcome::NotUsed
        );
        assert!(world.borrow::<EntitiesView>().unwrap().is_alive(patch));
        assert_eq!(
            world
                .borrow::<View<PropStackCount>>()
                .unwrap()
                .get(patch)
                .unwrap()
                .0,
            1
        );
        assert_eq!(
            world
                .borrow::<UniqueViewMut<ActiveHealing>>()
                .unwrap()
                .advance(5.0, 30, 30),
            0
        );
    }

    #[test]
    fn world_object_cannot_bypass_inventory_use() {
        let (world, _player, patch) = world_with_player(8, 1, false);
        assert_eq!(
            apply_healing_item_use(&world, patch, 10, 2, 0.1, 1.5),
            HealingItemUseOutcome::NotUsed
        );
        assert!(world.borrow::<EntitiesView>().unwrap().is_alive(patch));
    }

    #[test]
    fn duplicate_use_of_destroyed_source_does_not_queue_twice() {
        let (mut world, _player, patch) = world_with_player(8, 1, true);
        assert_eq!(
            apply_healing_item_use(&world, patch, 10, 2, 0.1, 1.5),
            HealingItemUseOutcome::DestroyEntity
        );
        world.delete_entity(patch);
        assert_eq!(
            apply_healing_item_use(&world, patch, 10, 2, 0.1, 1.5),
            HealingItemUseOutcome::NotUsed
        );
        assert_eq!(
            world
                .borrow::<UniqueViewMut<ActiveHealing>>()
                .unwrap()
                .advance(0.1, 8, 30),
            2
        );
    }
}

// Helper function for wildcard matching
fn wildcard_match(text: &str, pattern: &str) -> bool {
    if pattern == "*" {
        return true;
    }

    let text = text.to_lowercase();
    let pattern = pattern.to_lowercase();

    if pattern.starts_with('*') && pattern.ends_with('*') {
        let inner = &pattern[1..pattern.len() - 1];
        text.contains(inner)
    } else if pattern.starts_with('*') {
        let suffix = &pattern[1..];
        text.ends_with(suffix)
    } else if pattern.ends_with('*') {
        let prefix = &pattern[..pattern.len() - 1];
        text.starts_with(prefix)
    } else {
        text.contains(&pattern)
    }
}

impl crate::game_scene::GameScene for MissionCore {
    fn is_pausable(&self) -> bool {
        true
    }

    fn update(
        &mut self,
        time: &Time,
        input_context: &InputContext,
        asset_cache: &mut AssetCache,
        game_options: &GameOptions,
        command_effects: Vec<Effect>,
    ) -> Vec<Effect> {
        self.update(
            time,
            asset_cache,
            input_context,
            game_options,
            command_effects,
        )
    }

    fn render(
        &mut self,
        asset_cache: &mut AssetCache,
        options: &GameOptions,
    ) -> (Vec<SceneObject>, Vector3<f32>, Quaternion<f32>) {
        self.render(asset_cache, options)
    }

    fn render_per_eye(
        &mut self,
        asset_cache: &mut AssetCache,
        view: Matrix4<f32>,
        projection: Matrix4<f32>,
        screen_size: Vector2<f32>,
        options: &GameOptions,
    ) -> Vec<SceneObject> {
        self.render_per_eye(asset_cache, view, projection, screen_size, options)
    }

    fn finish_render(
        &mut self,
        asset_cache: &mut AssetCache,
        view: Matrix4<f32>,
        projection: Matrix4<f32>,
        screen_size: Vector2<f32>,
    ) {
        self.finish_render(asset_cache, view, projection, screen_size)
    }

    fn handle_effects(
        &mut self,
        effects: Vec<Effect>,
        global_context: &GlobalContext,
        game_options: &GameOptions,
        asset_cache: &mut AssetCache,
        audio_context: &mut AudioContext<EntityId, String>,
    ) -> Vec<GlobalEffect> {
        self.handle_effects(
            effects,
            global_context,
            game_options,
            asset_cache,
            audio_context,
        )
    }

    fn get_hand_spotlights(&self, options: &GameOptions) -> Vec<SpotLight> {
        self.get_hand_spotlights(options)
    }

    fn world(&self) -> &World {
        &self.world
    }

    fn script_world(&self) -> Option<&crate::scripts::ScriptWorld> {
        Some(&self.script_world)
    }

    fn scene_name(&self) -> &str {
        &self.level_name
    }

    fn ambient_audio_state(&self) -> Option<AmbientAudioState> {
        self.ambient_audio_state()
    }

    fn queue_entity_trigger(&mut self, entity_name: String) {
        self.queue_entity_trigger(entity_name);
    }

    fn wants_pointer(&self) -> bool {
        self.wants_pointer()
    }
}
