use std::{
    collections::{HashMap, HashSet, VecDeque},
    fs::File,
    io::BufReader,
    rc::Rc,
    sync::Arc,
    time::{Duration, SystemTime},
};

use cgmath::{EuclideanSpace, Zero};
use cgmath::{
    InnerSpace, Matrix3, Matrix4, Point3, Quaternion, Rotation, Rotation3, SquareMatrix, Transform,
    Vector2, Vector3, num_traits::ToPrimitive, vec3,
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
        PropAIAlertness, PropAIMode, PropAmbientHacked, PropClassTag, PropCreature,
        PropFrameAnimState, PropHasRefs, PropHitPoints, PropLimbModel, PropLocalPlayer,
        PropModelName, PropMotionActorTags, PropParticleGroup, PropParticleLaunchInfo,
        PropPhysDimensions, PropPhysInitialVelocity, PropPhysState, PropPhysType, PropPlayerGun,
        PropPosition, PropRenderType, PropScripts, PropTeleported, PropTripFlags,
        PropTweqDeleteConfig, PropTweqDeleteState, PropertyDefinition, RenderType, ToLink,
        TripFlags, TweqAnimationState, WrappedEntityId,
    },
    ss2_entity_info::{self, SystemShock2EntityInfo},
    tag_database::{TagQuery, TagQueryItem},
};
use engine::{
    assets::asset_cache::AssetCache,
    audio::{AudioChannel, AudioContext, AudioHandle},
    game_log, profile,
    scene::{
        BillboardMaterial, ParticleSystem, SceneObject, VertexPosition, light::SpotLight, quad,
    },
    texture::TextureTrait,
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
    gui::GuiManager,
    hud::{draw_item_name, draw_item_outline},
    input_context::{self, InputContext},
    interaction::{FlatInteraction, InteractionContext, PlayerInteraction, VrInteraction},
    inventory::PlayerInventoryEntity,
    mission::{SpatialQueryEngine, entity_populator::EntityPopulator},
    physics::{self, PlayerHandle},
    quest_info::QuestInfo,
    runtime_props::{
        RuntimePropAIBehavior, RuntimePropAttachment, RuntimePropDeathPose,
        RuntimePropDoNotSerialize, RuntimePropFlatAim, RuntimePropJointTransforms,
        RuntimePropReloading, RuntimePropSelectedAmmo, RuntimePropTransform, RuntimePropVhots,
    },
    save_load::HeldItemSaveData,
    scripts::{
        self, Effect, GlobalEffect, Message, MessagePayload,
        internal_fast_projectile::InternalFastProjectileScript,
        script_util::{get_all_links_with_template, get_environmental_sound_query},
        speech_registry::SpeechVoiceRegistry,
    },
    systems::{
        run_attachment_update, run_bitmap_animation, run_tweq, turn_off_tweqs, turn_on_tweqs,
    },
    teleport::{TeleportSystem, TeleportUI, TeleportVisualStyle},
    time::Time,
    util::{debug_entity, get_email_sound_file, has_refs, vec3_to_point3},
    virtual_hand::VirtualHandEffect,
    vr_config,
};

use crate::mission::entity_creator::{CreateEntityOptions, EntityCreationInfo};
pub use crate::resource_path;

/// `The Player` gamesys template - carries the player archetype data
/// (starting hit points, psi pool, base stats, vulnerabilities, ...).
pub const THE_PLAYER_TEMPLATE_ID: i32 = -384;

/// Vertical clearance (world units) a death-handoff ragdoll spawns with, so a
/// floor-lying crumple pose doesn't start deeply interpenetrating the level
/// trimesh (see `spawn_ragdoll`).
const RAGDOLL_SPAWN_LIFT: f32 = 0.05;

#[derive(Unique, Clone)]
pub struct PlayerInfo {
    pub pos: Vector3<f32>,
    pub rotation: Quaternion<f32>,
    pub entity_id: EntityId,

    pub left_hand_entity_id: Option<EntityId>,
    pub right_hand_entity_id: Option<EntityId>,
    pub inventory_entity_id: EntityId,
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

#[derive(Unique, Clone)]
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

    /// Flat-mode "use" (metagame) mode, toggled by `Effect::ToggleUseMode`
    /// (Tab). Mode tracking only for now - the cursor + panel presentation
    /// land with the flat UI host (see `projects/flat-ui.md`). Ignored in VR.
    pub flat_use_mode: bool,

    /// Flat-mode MFD panel host: the object-bound panel opened on frob, its
    /// canvas rendering, and the pointer -> GUIHover input mapping. Inert in
    /// VR (nothing opens a panel there). See `projects/flat-ui.md` §5.2.
    pub flat_ui: crate::mission::flat_ui_host::FlatUiHost,
}

pub struct GlobalContext {
    pub properties: Vec<Box<dyn PropertyDefinition<BufReader<File>>>>,
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

pub struct AbstractMission {
    pub scene_objects: Vec<SceneObject>,
    pub song_params: SongParams,
    pub room_db: RoomDatabase,
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
        let game_entity_info = &global_context.gamesys;
        let _motiondb = &global_context.motiondb;

        let mut world = World::new();
        let start = SystemTime::now();
        info!("starting level load");
        let scene = abstract_mission.scene_objects;
        let duration: Duration = start.elapsed().unwrap();
        info!("loading level took {}s", duration.as_secs_f32());

        let entity_info =
            ss2_entity_info::merge_with_gamesys(&abstract_mission.entity_info, game_entity_info);
        let entity_info_rc = Arc::new(entity_info);

        let speech_registry = SpeechVoiceRegistry::from_entity_info(&entity_info_rc);

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
        let template_name_to_template_id = create_template_name_map(game_entity_info);

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
        // idempotent across level loads. Setting current = max mirrors the
        // engine's existing model: the player is `RuntimePropDoNotSerialize` and
        // its HP/psi are re-seeded from the template on every load (there is no
        // current-HP persistence yet), so this only changes the values arrived
        // with, not whether a reset happens.
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

        // ** Entity creation

        let template_to_entity_id = entity_populator.populate(
            &entity_info_rc,
            &abstract_mission.entity_info,
            &abstract_mission.obj_map,
            &mut world,
        );

        // Instantiate held items
        let mut interaction: Box<dyn PlayerInteraction> =
            if game_options.presentation_mode == crate::PresentationMode::Flat {
                Box::new(FlatInteraction::new())
            } else {
                Box::new(VrInteraction::new())
            };
        let (left_hand_entity, right_hand_entity, maybe_inventory_entity) =
            held_item_save_data.instantiate(&mut world);

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
                    revealed_rects,
                    explored_rects,
                },
            ));
            world.add_unique(MapPanelEntity(entity));
        }

        world.add_unique(GlobalTemplateIdMap(template_to_entity_id.clone()));

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

        let world_entity_id = world.add_entity(RuntimePropDoNotSerialize {});
        if let Some(collider) = abstract_mission.physics_geometry {
            physics.add_collider(world_entity_id, collider);
        }

        // Finally, instantiate these entities
        for (entity_id, template_id) in entities_to_instantiate {
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

        // If the player is holding anything, we should un-physical it

        if let Some(entity_id) = left_hand_entity {
            interaction.grab(&world, entity_id, vr_config::Handedness::Left);
            make_un_physical2(&mut id_to_physics, &mut physics, entity_id);
        };

        if let Some(entity_id) = right_hand_entity {
            interaction.grab(&world, entity_id, vr_config::Handedness::Right);
            make_un_physical2(&mut id_to_physics, &mut physics, entity_id);
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

        world.add_unique(quest_info);

        // Preload the elevator floor labels (MISC.STR) + current mission so the
        // AssetCache-less ElevatorGui can label/gate floors at draw time.
        world.add_unique(crate::scripts::ElevatorContext::load(asset_cache, &mission));
        world.add_unique(crate::scripts::gui::TraitsContext::load(asset_cache));

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
        // pose.
        {
            let mut attachments: Vec<(EntityId, EntityId, Matrix4<f32>)> = Vec::new();
            {
                let v_links = world.borrow::<View<Links>>().unwrap();
                let v_transform = world.borrow::<View<RuntimePropTransform>>().unwrap();
                for (id, links) in v_links.iter().with_id() {
                    for link in &links.to_links {
                        if !matches!(link.link, dark::properties::Link::ParticleAttachement(_)) {
                            continue;
                        }
                        let Some(parent) = link.to_entity_id else {
                            continue;
                        };
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
                let Some(hit) = physics.ray_cast2(
                    origin,
                    direction,
                    remaining,
                    crate::physics::InternalCollisionGroups::WORLD
                        | crate::physics::InternalCollisionGroups::ENTITY,
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

        MissionCore {
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
            scene_objects: scene,
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
            flat_use_mode: false,
            flat_ui: crate::mission::flat_ui_host::FlatUiHost::new(),
        }
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
        // no longer exist, so dead AIs don't ghost in GET /v1/ai/paths
        if time.elapsed.as_secs_f32() > 0.0 {
            if let Some(service) = &self.pathfinding_service {
                if let Ok(entities) = self.world.borrow::<shipyard::EntitiesView>() {
                    service.prune_ai_paths(|inner| {
                        shipyard::EntityId::from_inner(inner)
                            .map(|id| entities.is_alive(id))
                            .unwrap_or(false)
                    });
                }
            }
        }

        // Sync live door state into the pathfinding service: impassable
        // doors make their below-door cells unpathable, so A* routes around
        // them (or stops at them) instead of through them. Impassable means
        // locked-and-closed, OR a door this engine cannot operate at all - no
        // runtime entity, or no TransDoor prop (e.g. medsci1's space-shield
        // membranes, which StdDoor can't move; AIs wedged against them
        // forever - issue #489). Unlocked closed translating doors stay
        // pathable - pursuing AIs open those on arrival.
        if time.elapsed.as_secs_f32() > 0.0 {
            if let Some(service) = &self.pathfinding_service {
                if let Ok(id_map) = self
                    .world
                    .borrow::<UniqueView<crate::mission::GlobalTemplateIdMap>>()
                {
                    let locked: std::collections::HashSet<i32> = service
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
                            match crate::scripts::script_util::door_is_closed(&self.world, ent) {
                                Some(true) => {
                                    crate::scripts::script_util::is_entity_locked(&self.world, ent)
                                }
                                Some(false) => false,
                                // Not a translating door: neither AIs nor
                                // StdDoor can move it - a wall until shield/
                                // rotating door support exists
                                None => true,
                            }
                        })
                        .collect();
                    service.set_locked_doors(locked);
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
        let mut effects = command_effects;

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
                self.physics.update(
                    forward + cgmath::vec3(0.0, up_value, 0.0),
                    &mut self.player_handle,
                )
            )
        };

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
                } => {
                    self.script_world.dispatch(Message {
                        to: entity1_id,
                        payload: MessagePayload::Collided { with: entity2_id },
                    });
                    self.script_world.dispatch(Message {
                        to: entity2_id,
                        payload: MessagePayload::Collided { with: entity1_id },
                    });
                }
            }
        }

        // Update PropTeleported entities
        self.world.run(
            |mut v_teleported: ViewMut<dark::properties::PropTeleported>| {
                let mut ents_to_remove = Vec::new();
                for (id, door) in (&mut v_teleported).iter().with_id() {
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

        // VR drives two hands; flat drives a single first-person weapon
        // controller. Both feed the same effect-processing path.
        let interaction_msgs = self.interaction.update(&InteractionContext {
            physics: &self.physics,
            world: &self.world,
            input: input_context,
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
        if game_options.presentation_mode == crate::PresentationMode::Flat {
            // Expose the AMMOFULL ammo-cycle button for hit-testing exactly when
            // the flat HUD draws it (the shared visibility predicate, so the
            // clickable rect never diverges from the rendered button).
            let ammo_button =
                if crate::hud::ammo_cycle_button_visible(&self.world, self.flat_use_mode) {
                    Some(crate::hud::AMMO_CYCLE_BUTTON)
                } else {
                    None
                };
            self.flat_ui.set_ammo_cycle_button(ammo_button);

            let (messages, drag_actions) = self.flat_ui.update(&self.world, input_context.pointer);
            for msg in messages {
                self.script_world.dispatch(msg);
            }
            for action in drag_actions {
                let drag_effects = self.apply_flat_drag_action(action);
                effects.extend(drag_effects);
            }
        }

        // Update scripts
        let mut script_effects = profile!(
            scope: "game", level: DEBUG, "script_world.update",
            self.script_world.update(&self.world, &self.physics, time)
        );
        effects.append(&mut script_effects);

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
                    let result = global_context.motiondb.query(query.clone());
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
                            resolved_death_pose = Some(next_animation);
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

        if let Some(motion_or_tag_name) = resolved_death_pose {
            self.world
                .add_component(entity_id, RuntimePropDeathPose(motion_or_tag_name));
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
                let scaled = vec3(
                    adj_velocity.x * scale,
                    adj_velocity.y,
                    adj_velocity.z * scale,
                );
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
                if !holds_terminal_death_pose {
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
                        CreateEntityOptions::default(),
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
                CreateEntityOptions::default(),
            ))
        } else {
            None
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
        let mut was_able_to_drop = false;
        {
            // First, remove any existing contains links for the dropped entity..
            let mut v_links = self.world.borrow::<ViewMut<Links>>().unwrap();

            for (id, links) in (&mut v_links).iter().with_id() {
                drop_contains_links_to(links, dropped_entity_id);

                // If it is the container, we'll add the link!
                if id == container_entity_id {
                    links.to_links.push(ToLink {
                        link: Link::Contains(0),
                        to_entity_id: Some(dark::properties::WrappedEntityId(dropped_entity_id)),
                        to_template_id: 0, // todo?
                    });
                    was_able_to_drop = true;
                }
            }
        }
        if was_able_to_drop {
            self.world
                .add_component(dropped_entity_id, PropHasRefs(false));
            self.make_un_physical(dropped_entity_id);
        }
        was_able_to_drop
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
        const THROW_SPAWN_DISTANCE: f32 = 0.5;
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
        // TODO: gui - remove entity
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

        let mut effects = VecDeque::from(effects);
        while let Some(effect) = effects.pop_front() {
            match effect {
                Effect::AcquireKeyCard { key_card } => {
                    let mut quests = self.world.borrow::<UniqueViewMut<QuestInfo>>().unwrap();
                    quests.add_key_card(key_card);
                    drop(quests);
                }

                Effect::AdjustHitPoints { entity_id, delta } => {
                    let mut v_hit_points = self
                        .world
                        .borrow::<ViewMut<dark::properties::PropHitPoints>>()
                        .unwrap();

                    if let Ok(hit_points) = (&mut v_hit_points).get(entity_id) {
                        hit_points.hit_points += delta;
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
                    let wielded = self
                        .world
                        .borrow::<UniqueView<PlayerInfo>>()
                        .unwrap()
                        .left_hand_entity_id;

                    if let Some(weapon) = wielded {
                        self.begin_reload(weapon);
                    }
                }

                Effect::CycleAmmo => {
                    let wielded = self
                        .world
                        .borrow::<UniqueView<PlayerInfo>>()
                        .unwrap()
                        .left_hand_entity_id;

                    if let Some(weapon) = wielded {
                        self.cycle_ammo(weapon);
                    }
                }

                Effect::ToggleUseMode => {
                    // Flat-presentation only: VR has no cursor mode to toggle.
                    if game_options.presentation_mode == crate::PresentationMode::Flat {
                        self.flat_use_mode = !self.flat_use_mode;
                        // Use mode shows the player's backpack as the
                        // top-docked inventory strip: bind the strip to the
                        // `internal_inventory` entity (whose GuiScript
                        // already emits SetUI every frame).
                        // Leaving use mode drops any item on the cursor. It was
                        // never removed from the backpack (the host only hid it
                        // from the strip), so clearing the cursor is enough -
                        // the item is already reachable there (projects/flat-ui.md
                        // §1.5). This also means a save/transition mid-drag
                        // serializes it correctly, with no orphan.
                        if !self.flat_use_mode {
                            self.flat_ui.take_cursor_item();
                        }
                        let strip_entity = if self.flat_use_mode {
                            self.world
                                .borrow::<UniqueView<PlayerInfo>>()
                                .ok()
                                .map(|player| player.inventory_entity_id)
                        } else {
                            None
                        };
                        self.flat_ui.set_strip(strip_entity);
                    }
                }

                Effect::OpenPanel { entity } => {
                    // Flat-presentation only: bind the MFD panel to the
                    // frobbed object (the original's frob-script -> overlay
                    // flow). VR ignores it - panels are world quads there.
                    if game_options.presentation_mode == crate::PresentationMode::Flat {
                        self.flat_ui.open(entity);
                    }
                }

                Effect::CollectLog {
                    entity_id,
                    deck,
                    log,
                } => {
                    // Record the log identity into the persistent collection...
                    {
                        let mut quests = self.world.borrow::<UniqueViewMut<QuestInfo>>().unwrap();
                        quests.collect_log(deck, log);
                    }
                    // ...and resolve the reader strings from `level<deck>.str`,
                    // caching them on the disc so the MediaGui panel can render
                    // the portrait / deck icon / header / transcript.
                    let level_file = format!("level{deck:02}.str");
                    if let Some(strings) =
                        asset_cache.get_opt(&dark::importers::STRINGS_IMPORTER, &level_file)
                    {
                        // The .str values carry literal backslash-n escapes
                        // ("AMANPOUR 07.JUL.14\nre: New code\n"); unescape them
                        // into real line breaks so the reader never draws "\n".
                        let get = |prefix: &str| {
                            strings
                                .get(&format!("{prefix}{log}"))
                                .map(|s| s.replace("\\n", "\n"))
                        };
                        self.world.add_component(
                            entity_id,
                            crate::runtime_props::RuntimePropLogData {
                                name: get("logname"),
                                text: get("logtext"),
                                portrait: get("logportrait"),
                                icon: get("logicon"),
                            },
                        );
                    }
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
                    let player_entity = self
                        .world
                        .borrow::<UniqueView<PlayerInfo>>()
                        .unwrap()
                        .entity_id;
                    let mut v_psi = self
                        .world
                        .borrow::<ViewMut<dark::properties::PropPsiState>>()
                        .unwrap();
                    if let Ok(psi) = (&mut v_psi).get(player_entity) {
                        psi.psi_points = (psi.psi_points - amount).max(0);
                    }
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
                    template_name,
                    position,
                    orientation,
                } => {
                    self.create_entity_by_template_name(
                        asset_cache,
                        &template_name,
                        position,
                        orientation,
                    );
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
                Effect::DropEntityInfo {
                    parent_entity_id,
                    dropped_entity_id,
                } => {
                    self.drop_entity_into_container(parent_entity_id, dropped_entity_id);
                }

                Effect::GrabEntity {
                    entity_id,
                    hand,
                    current_parent_id: _,
                } => {
                    // What's held before the grab, so anything the grab
                    // displaces (the flat wield swaps the viewmodel out) can
                    // be holstered back into the backpack below. VR grabs
                    // into an occupied hand no-op, so no displacement there.
                    let held_before = self.interaction.held_entities();

                    let grab_effects = self.interaction.grab(&self.world, entity_id, hand);
                    self.process_virtual_hand_effects(asset_cache, grab_effects);

                    if self.interaction.is_holding(entity_id) {
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

                    // Holster anything the grab displaced (a weapon the flat
                    // wield swapped out) back into the player's backpack -
                    // the original returns it to the inventory grid, not the
                    // floor at the viewmodel position.
                    let held_after = self.interaction.held_entities();
                    let displaced =
                        [held_before.0, held_before.1]
                            .into_iter()
                            .flatten()
                            .find(|prev| {
                                *prev != entity_id
                                    && held_after.0 != Some(*prev)
                                    && held_after.1 != Some(*prev)
                            });
                    if let Some(prev) = displaced {
                        let inventory_entity = self
                            .world
                            .borrow::<UniqueView<PlayerInfo>>()
                            .map(|player| player.inventory_entity_id)
                            .ok();
                        if let Some(inventory_entity) = inventory_entity {
                            self.drop_entity_into_container(inventory_entity, prev);
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
                    // Flat presentation: the active panel's components are
                    // drawn by the FlatUiHost onto the screen-space canvas.
                    // Deliberately NOT behind `--experimental gui` - the flat
                    // MFD is the #435 fix; only the VR world-quad path below
                    // keeps its gating (projects/flat-ui.md §7).
                    if game_options.presentation_mode == crate::PresentationMode::Flat {
                        self.flat_ui
                            .on_set_ui(&self.world, parent_entity, world_size, &components);
                    }
                    if game_options.experimental_features.contains("gui") {
                        self.gui.update_ui(
                            &mut self.world,
                            &mut self.physics,
                            &mut self.script_world,
                            &mut self.id_to_physics,
                            handle,
                            parent_entity,
                            world_size,
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
                    if let Some(model) = self.id_to_model.get(&entity_id) {
                        // if !scene_objs.is_empty() {
                        //let scene_obj = scene_objs.get(0).unwrap().borrow();
                        let xform = model.get_transform();
                        //drop(scene_obj);

                        let _ext_name = model_name.clone();
                        let orig_model =
                            asset_cache.get(&MODELS_IMPORTER, &format!("{model_name}.BIN"));

                        let orig_model_ref = orig_model.as_ref();

                        let new_model = Model::transform(orig_model_ref, xform);

                        let vhots = new_model.vhots();
                        self.id_to_model.insert(entity_id, new_model);
                        self.world
                            .add_component(entity_id, PropModelName(model_name));

                        self.world.add_component(entity_id, RuntimePropVhots(vhots));
                    }
                }
                Effect::SetVhotsFromModel {
                    entity_id,
                    model_name,
                } => {
                    if let Some(model) = self.id_to_model.get(&entity_id) {
                        let xform = model.get_transform();
                        let donor_model =
                            asset_cache.get(&MODELS_IMPORTER, &format!("{model_name}.BIN"));
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
                        engine::audio::play_audio(
                            audio_context,
                            AudioHandle::new(),
                            Some(AudioChannel::new("email".to_owned())),
                            audio_clip,
                        );
                        // Observability: record the email so the headless e2e
                        // can assert it played (and only once).
                        crate::audio_log::record(
                            &email_file,
                            vec![("kind".to_string(), "email".to_string())],
                            [0.0, 0.0, 0.0],
                        );
                    }
                    drop(quests);
                }
                Effect::PlaySound { handle, name } => {
                    println!("Trying to play sound: {}", &name);
                    let audio_file = resolve_schema(global_context, &name.to_string());
                    let maybe_audio_clip =
                        asset_cache.get_opt(&AUDIO_IMPORTER, &format!("{audio_file}.wav"));

                    if let Some(audio_clip) = maybe_audio_clip {
                        info!("Playing clip: {} handle: {:?}", name, &handle);
                        engine::audio::play_audio(audio_context, handle, None, audio_clip);
                        // Observability: record scripted one-shot sounds (audio
                        // logs, keypad beeps, ...) so headless tooling can assert
                        // a schema actually resolved and played.
                        crate::audio_log::record(
                            &audio_file,
                            vec![("kind".to_string(), "sound".to_string())],
                            [0.0, 0.0, 0.0],
                        );
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
                    if let Some(sample_name) = resolve_speech_sample(
                        &global_context.gamesys,
                        voice_index,
                        concept.as_str(),
                        &tags,
                    ) {
                        let audio_path = format!("{sample_name}.wav");
                        if let Some(audio_clip) = asset_cache.get_opt(&AUDIO_IMPORTER, &audio_path)
                        {
                            let handle = AudioHandle::new();
                            if let Some(position) = get_entity_position(&self.world, entity_id) {
                                engine::audio::play_spatial_audio(
                                    audio_context,
                                    position,
                                    handle,
                                    None,
                                    audio_clip,
                                );
                            } else {
                                engine::audio::play_audio(audio_context, handle, None, audio_clip);
                            }
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
                        let spawned_ragdoll =
                            game_options.experimental_features.contains("ragdoll")
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
                    // Death-crumple handoff (AI deaths): once the death
                    // animation has finished, replace the animated corpse with
                    // a physics ragdoll seeded from its final pose. Gated on
                    // the `ragdoll` experimental flag - without it the
                    // animated corpse entity persists exactly as before.
                    // (Known experimental limitations: the ragdoll corpse is
                    // not serialized, so it vanishes on save/load; and the
                    // creature entity is removed, which will matter once
                    // corpse looting (`creaturecontainer`) is implemented.)
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
                }
                Effect::StopSound { handle } => {
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
                    let applied = quests
                        .player_stats_mut()
                        .apply_tour_reward(career, year, tour);
                    if applied {
                        info!(
                            "Applied training-tour reward ({:?} year {} tour {}): {:?}",
                            career,
                            year,
                            tour,
                            quests.player_stats()
                        );
                    }
                }

                Effect::TrainerPurchase { target } => {
                    // Authoritative validation + mutation (the panel only
                    // pre-validates for feedback): re-quote from the cost
                    // tables, spend atomically, then raise the target.
                    use crate::scripts::gui::{apply_purchase, upgrade_quote};
                    let costs = self
                        .world
                        .borrow::<UniqueView<GlobalTrainerCosts>>()
                        .unwrap()
                        .0
                        .clone();
                    if let Some(costs) = costs {
                        let mut quests = self.world.borrow::<UniqueViewMut<QuestInfo>>().unwrap();
                        let stats = quests.player_stats_mut();
                        match upgrade_quote(&costs, stats, target) {
                            Some(cost) if stats.spend_cyber_modules(cost) => {
                                apply_purchase(stats, target);
                                info!(
                                    "Trainer purchase {:?} (-{} modules, balance {})",
                                    target, cost, stats.cyber_modules
                                );
                            }
                            Some(cost) => {
                                info!(
                                    "Trainer purchase {:?} refused: costs {}, balance {}",
                                    target, cost, stats.cyber_modules
                                );
                            }
                            None => {
                                info!("Trainer purchase {:?} refused: maxed/locked", target);
                            }
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
                            name: "repfail".to_owned(),
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
                            name: "replic2e".to_owned(),
                        });
                    } else {
                        info!(
                            "Replicator purchase refused: {} costs {} nanites",
                            template_name, cost
                        );
                        effects.push_front(Effect::PlaySound {
                            handle: AudioHandle::new(),
                            name: "repfail".to_owned(),
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
                    let mut quests = self.world.borrow::<UniqueViewMut<QuestInfo>>().unwrap();
                    // Without a stable machine id the used state could never
                    // be recorded - refuse rather than vend repeatably.
                    let Some(machine_id) = machine_template_id else {
                        warn!("O/S trait {} refused: machine has no template id", trait_id);
                        continue;
                    };
                    let used = quests
                        .read_quest_bit_value(&used_bit_name(machine_id))
                        .bits()
                        != 0;
                    if used {
                        info!("O/S trait {} refused: machine already used", trait_id);
                    } else if !quests.player_stats_mut().add_os_trait(trait_id) {
                        info!("O/S trait {} refused: owned or slots full", trait_id);
                    } else {
                        quests.set_quest_bit_value(
                            &used_bit_name(machine_id),
                            dark::properties::QuestBitValue::COMPLETE,
                        );
                        // Live effects for the implemented subset; the rest
                        // are storage-only (their consumers don't exist yet).
                        match trait_id {
                            TRAIT_NATURALLY_ABLE => {
                                quests
                                    .player_stats_mut()
                                    .award_cyber_modules(NATURALLY_ABLE_MODULES);
                            }
                            TRAIT_TANK => {
                                // Tank (Trait8): the original raises the
                                // ceiling AND current HP by the bonus (buying
                                // at 25/30 yields 30/35), clamped to the new
                                // max. Loads re-derive both from the trait
                                // list (current HP is not persisted - it
                                // re-seeds from template + career + traits),
                                // so the live grant cannot double-apply.
                                drop(quests);
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
                                                hp.hit_points =
                                                    hp.hit_points.min(new_max as i32);
                                            }
                                        }
                                    },
                                );
                            }
                            _ => {}
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
                        }
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
                    // Force-wield (unlike SpawnDebugItem): `wield` drops the
                    // previously held weapon back into the world, so each cycle
                    // swaps the viewmodel. No-op in VR (wield returns nothing).
                    let msgs = self.interaction.wield(info.entity_id);
                    self.process_virtual_hand_effects(asset_cache, msgs);
                }
                Effect::PositionInventoryRelativeToPlayer { head_rotation } => {
                    let (pos, rot) = {
                        let player = self.world.borrow::<UniqueView<PlayerInfo>>().unwrap();
                        (player.pos, player.rotation * head_rotation)
                    };
                    let forward = rot * vec3(0.0, 0.5 / SCALE_FACTOR, -8.0 / SCALE_FACTOR);
                    PlayerInventoryEntity::set_position_rotation(
                        &mut self.world,
                        pos + forward,
                        Quaternion::from_angle_y(cgmath::Deg(180.0)) * rot,
                    )
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
        let (next, _flags, events, _disp) = AnimationPlayer::update(&player, dt);
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
                    for (i, obj) in scene_objs.into_iter().enumerate() {
                        let mut o = obj.clone();
                        o.set_transform(squish * xform);
                        if i == 0 {
                            o.set_clear_depth(true);
                        }
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
                !self.flat_use_mode,
                // Use mode expands the compact readouts to BIOFULL/AMMOFULL.
                self.flat_use_mode,
            ));

            // Flat MFD panel (keypad, container, ...) + cursor, drawn over
            // the HUD. Also records the render-target size the pointer ->
            // canvas mapping needs.
            self.flat_ui.set_screen_size(screen_size);
            ret.extend(self.flat_ui.render(asset_cache, screen_size));
        }

        ret
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

            if let Ok(xform) = v_transform.get(*entity_id).map(|p| p.0) {
                for obj in scene_objs {
                    let mut xformed_obj = obj.clone();
                    xformed_obj.set_transform(xform);
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
        // drawn on top in `render_per_eye`).
        scene.append(&mut self.interaction.render(asset_cache, &self.world));

        // Render inventory
        let inventory_objs = PlayerInventoryEntity::render(&self.world);
        scene.extend(inventory_objs);

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

        // Render gui
        if options.experimental_features.contains("gui") {
            let guis = self.gui.render(asset_cache, &self.world);

            scene.extend(guis);
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
    /// mouse-look: an MFD panel is open, or Tab "use" mode is active
    /// (projects/flat-ui.md §5.2). Both are flat-only states, so this is
    /// inherently false in VR.
    pub fn wants_pointer(&self) -> bool {
        self.flat_ui.active_panel().is_some() || self.flat_use_mode
    }

    /// Actual crouch state of the player collider (stand-up can be refused
    /// for lack of headroom, so this can lag the crouch input).
    pub fn player_is_crouched(&self) -> bool {
        self.player_handle.is_crouched()
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
                    self.make_un_physical(entity_id);
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
                    self.make_physical(entity_id);

                    self.script_world.dispatch(Message {
                        payload: MessagePayload::Drop,
                        to: entity_id,
                    });
                }
            }
        }
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

fn create_template_name_map(game_entity_info: &Gamesys) -> HashMap<String, EntityMetadata> {
    let mut gamesys_world = World::new();
    game_entity_info.entity_info.initialize_world_with_entities(
        &mut gamesys_world,
        HashMap::new(),
        |_id| true,
    );

    let mut name_to_template_id = HashMap::new();

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
            }
        },
    );

    name_to_template_id
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

/// Remove every `Contains` link in `links` that points at `target` - used when
/// an item leaves a container (moved/dropped into another container, or
/// destroyed while contained), so no stale `Contains` link is left behind.
fn drop_contains_links_to(links: &mut Links, target: EntityId) {
    links.to_links.retain(|link| {
        !(matches!(link.link, Link::Contains(_)) && link.to_entity_id.map(|e| e.0) == Some(target))
    });
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

fn get_entity_position(world: &World, entity_id: EntityId) -> Option<Vector3<f32>> {
    if let Ok(positions) = world.borrow::<View<PropPosition>>() {
        if let Ok(prop) = positions.get(entity_id) {
            return Some(prop.position);
        }
    }
    None
}

fn resolve_schema(global_context: &GlobalContext, name: &str) -> String {
    let sound_schema = &global_context.gamesys.sound_schema;
    let ret = sound_schema
        .get_random_sample(name)
        .unwrap_or_else(|| name.to_owned());
    trace!("resolved sound schema {} to {}", name, ret);
    ret
}

fn resolve_speech_sample(
    gamesys: &Gamesys,
    voice_index: usize,
    concept: &str,
    tags: &[(String, String)],
) -> Option<String> {
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

    Some(samples[selected_index].sample_name.clone())
}

fn play_environmental_sound(
    gamesys: &Gamesys,
    asset_cache: &mut AssetCache,
    audio_context: &mut AudioContext<EntityId, String>,
    query: dark::EnvSoundQuery,
    audio_handle: AudioHandle,
    position: Vector3<f32>,
) {
    let maybe_audio_file = gamesys.get_random_environmental_sound(&query);
    if maybe_audio_file.is_some() {
        let audio_file = maybe_audio_file.unwrap();
        let audio_clip = asset_cache.get(&AUDIO_IMPORTER, &format!("{audio_file}.wav").to_owned());

        info!(
            "Playing clip: {} handle: {:?} position: {:?}",
            audio_file, &audio_handle, position
        );
        // Log the resolved play so headless tooling (debug runtime
        // /v1/audio/recent) can assert a schema actually played.
        crate::audio_log::record(
            &audio_file,
            query.tag_values(),
            [position.x, position.y, position.z],
        );
        engine::audio::play_spatial_audio(audio_context, position, audio_handle, None, audio_clip);
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
        use crate::game_scene::{DebugEntityDetail, DebugLinkInfo, DebugPropertyInfo};
        use shipyard::*;

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

                // Build links
                let mut outgoing_links = Vec::new();
                let incoming_links = Vec::new();

                if let Ok(links) = v_links.get(id) {
                    for link in &links.to_links {
                        if let Some(target_entity) = link.to_entity_id {
                            outgoing_links.push(DebugLinkInfo {
                                link_type: format!("{:?}", link.link),
                                target_id: target_entity.0.inner() as i32,
                                target_name: v_sym_name
                                    .get(target_entity.0)
                                    .map(|s| s.0.clone())
                                    .unwrap_or_else(|_| {
                                        format!("Entity_{}", target_entity.0.inner())
                                    }),
                            });
                        }
                    }
                    // TODO: Incoming links require scanning all entities - simplified for now
                }

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
                    "entity" => groups |= crate::physics::InternalCollisionGroups::ENTITY,
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
            .ray_cast3(start, end, collision_groups, None, false)
        {
            Some(hit) => {
                let entity_name = hit.maybe_entity_id.and_then(|id| {
                    self.world.run(
                        |v_sym_name: shipyard::View<dark::properties::PropSymName>| {
                            v_sym_name.get(id).ok().map(|s| s.0.clone())
                        },
                    )
                });

                DebugRayHit {
                    hit: true,
                    hit_point: Some([hit.hit_point.x, hit.hit_point.y, hit.hit_point.z]),
                    hit_normal: Some([hit.hit_normal.x, hit.hit_normal.y, hit.hit_normal.z]),
                    distance: Some((end - start).magnitude()),
                    entity_id: hit.maybe_entity_id.map(|id| id.inner() as i32),
                    entity_name,
                    collision_group: None, // TODO: Add collision group info
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
        let active_panel = self.flat_ui.active_panel().map(|entity| {
            let (name, template_id) = panel_identity(entity);
            crate::game_scene::DebugUiPanel {
                entity_id: entity.inner() as i32,
                template_id,
                name,
                elements: self.flat_ui.debug_elements(&self.world),
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
            mode: if self.flat_use_mode {
                "use".to_string()
            } else {
                "shooter".to_string()
            },
            active_panel,
            strip,
            cursor: self.flat_ui.cursor_debug(),
            ammo_cycle: self.flat_ui.ammo_cycle_debug(),
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
