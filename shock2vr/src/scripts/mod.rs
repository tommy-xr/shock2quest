pub mod ai;
pub mod effect;
pub mod speech_registry;
pub mod speech_util;

mod apparition;
mod auto_install_soft;
mod base_button;
mod base_elevator;
mod base_light;
mod base_monster;
mod base_room;
mod burst_fire;
mod camera_alert;
mod chemical;
mod choose_mission;
mod choose_service;
mod comestible;
mod core_room;
mod create_sound;
mod cs9;
mod dead_power_cell;
mod destroy_all_by_name;
mod die_shodan_die;
mod energy_station;
mod energy_weapon;
mod exp_cookie;
mod frob_qb;
pub mod gui;
pub mod healing_item;
mod internal_collision_type;
mod internal_explosion;
pub mod internal_fast_projectile;
mod internal_frob_move;
mod internal_keycard_script;
mod internal_nanites_script;
pub(crate) mod internal_radiation_source;
mod internal_simple_health;
mod internal_switch_held_model;
mod level_change_button;
mod many_ride;
mod melee_weapon;
mod obj_consume_button;
mod once_room;
mod once_router;
mod picture_swap;
pub mod player_script;
mod psi_amp_script;
mod psi_kit;
mod put_bomb_in_replicator;
pub mod radiation;
mod reduce_psi;
mod reroute_elevator_button;
mod researchable;
mod room_trigger;
pub mod script_util;
mod setup_initial_debrief;
mod std_door;
mod tool_consumable;
mod transluce;
mod trap_delay;
mod trap_destroyer;
mod trap_email;
mod trap_exp_once;
mod trap_inverter;
mod trap_new_tripwire;
mod trap_off_filter;
mod trap_on_filter;
mod trap_qb_filter;
mod trap_qb_neg_filter;
mod trap_qb_set;
mod trap_questbit_simple;
mod trap_router;
mod trap_signal;
mod trap_slayer;
mod trap_sound;
mod trap_spawn;
mod trap_teleport;
mod trap_teleport_player;
mod trap_trip_level;
mod trap_tweq;
mod trap_unlock;
mod trigger_collide;
mod trigger_damage;
mod trigger_destroy;
mod trigger_ecology;
mod trigger_multi;
mod tweq_depressable;
mod tweqable;
mod use_sound;
mod vaporize_inventory;
mod weapon_script;
use std::{
    collections::{HashMap, HashSet},
    fmt,
};

use cgmath::{Point2, Vector3};
use dark::motion::MotionFlags;
pub use effect::*;

use serde::{Deserialize, Serialize, de::DeserializeOwned};

use shipyard::{EntityId, World};
use tracing::{Level, info, span, trace, warn};

use crate::util::debug_entity;
use crate::vr_config::Handedness;
use crate::{physics::PhysicsWorld, time::Time};

use crate::gui::gui_script;

use self::chemical::ChemicalScript;
use self::choose_mission::ChooseMissionScript;
use self::choose_service::ChooseServiceScript;
use self::comestible::Comestible;
/// Preloaded elevator floor labels (from MISC.STR) + current mission, added as
/// a world unique at mission load so the (`AssetCache`-less) `ElevatorGui` can
/// read them at draw time.
pub use self::gui::ElevatorContext;
use self::gui::{
    ComputerGui, ContainerGui, ElevatorGui, GamePigGui, HackableCrateGui, KeyPadGui, MapGui,
    MediaGui, ReplicatorGui, ResearchGui, TrainerGui, TrainerMode, TraitGui,
};
use self::healing_item::{HealingItemKind, HealingItemScript};
use self::internal_frob_move::InternalFrobMove;
use self::internal_switch_held_model::InternalSwitchHeldModelScript;
use self::picture_swap::PictureSwap;
use self::psi_amp_script::PsiAmpScript;
use self::psi_kit::PsiKitScript;
use self::put_bomb_in_replicator::PutBombInReplicator;
use self::radiation::RadPatchScript;
use self::reduce_psi::ReducePsi;
use self::reroute_elevator_button::RerouteElevatorButton;
use self::researchable::ResearchableScript;
use self::trap_signal::TrapSignal;
use self::{
    auto_install_soft::AutoInstallSoft,
    base_button::BaseButton,
    base_elevator::BaseElevator,
    base_light::BaseLight,
    base_monster::BaseMonster,
    camera_alert::CameraAlert,
    core_room::*,
    create_sound::*,
    cs9::{
        CS9DoorReporter, CS9EggsAndGrubs, CS9HoloRumbler, CS9MasterControl, CS9ShodanScreen,
        SitDownRightNowMP, TrapDestroyTeleport,
    },
    dead_power_cell::DeadPowerCell,
    destroy_all_by_name::DestroyAllByName,
    die_shodan_die::DieShodanDie,
    energy_station::EnergyStation,
    energy_weapon::EnergyWeapon,
    exp_cookie::ExpCookie,
    frob_qb::FrobQB,
    internal_collision_type::InternalCollisionType,
    internal_explosion::InternalExplosion,
    internal_keycard_script::KeyCardScript,
    internal_nanites_script::InternalNanitesScript,
    internal_radiation_source::InternalRadiationSource,
    internal_simple_health::InternalSimpleHealth,
    level_change_button::LevelChangeButton,
    many_ride::{ParalyzePlayers, SitDownRightNow, StandUpAgain, WhiteOut},
    melee_weapon::{HeldMeleeWeapon, MeleeWeapon},
    obj_consume_button::ObjConsumeButton,
    once_room::OnceRoom,
    once_router::OnceRouter,
    room_trigger::RoomTrigger,
    std_door::StdDoor,
    tool_consumable::ToolConsumable,
    transluce::TransluceInOutHolo,
    trap_delay::TrapDelay,
    trap_destroyer::TrapDestroyer,
    trap_email::TrapEmail,
    trap_exp_once::TrapEXPOnce,
    trap_inverter::TrapInverter,
    trap_new_tripwire::TrapNewTripwire,
    trap_on_filter::TrapOffFilter,
    trap_qb_filter::TrapQBFilter,
    trap_qb_neg_filter::TrapQBNegFilter,
    trap_qb_set::TrapQBSet,
    trap_questbit_simple::TrapQuestbitSimple,
    trap_router::TrapRouter,
    trap_slayer::TrapSlayer,
    trap_sound::TrapSound,
    trap_spawn::TrapSpawn,
    trap_teleport::TrapTeleport,
    trap_teleport_player::TrapTeleportPlayer,
    trap_trip_level::TrapTripLevel,
    trap_tweq::TrapTweq,
    trap_unlock::TrapUnlock,
    trigger_collide::TriggerCollide,
    trigger_damage::TriggerDamage,
    trigger_destroy::TriggerDestroy,
    trigger_ecology::TriggerEcology,
    trigger_multi::TriggerMulti,
    tweq_depressable::TweqDepressable,
    tweqable::Tweqable,
    use_sound::UseSound,
    vaporize_inventory::VaporizeInventory,
    weapon_script::WeaponScript,
};

/// World-space description of the blow behind a `Damage` message, for physics
/// reactions (ragdoll seeding). None when the source has no meaningful
/// direction (scripted damage; radius blasts shove bodies directly).
#[derive(Clone, Copy, Debug)]
pub struct DamageImpact {
    /// Unit direction the blow traveled (attacker toward victim).
    pub direction: cgmath::Vector3<f32>,
    /// World-space hit point.
    pub point: cgmath::Vector3<f32>,
    /// Skeleton joint id of the hitbox that was struck, when known (filled in
    /// by HitBoxScript as it forwards damage to its parent creature).
    pub bone: Option<u32>,
}

#[derive(Clone, Debug)]
pub enum MessagePayload {
    Frob,

    /// This entity's MFD panel was opened. Frobbing used to be the only way in,
    /// so panels reset their per-open state on `Frob`; the audio-log reader is
    /// opened by `ReadLastUnreadLog` instead, and any panel may later be opened
    /// without a frob.
    PanelOpened,

    // Physics events
    SensorBeginIntersect {
        with: EntityId,
    },
    SensorEndIntersect {
        with: EntityId,
    },
    Collided {
        with: EntityId,
        /// Exact physical contact geometry when this came from Rapier. The
        /// normal points from the message receiver toward `with`; synthetic
        /// script collisions have no contact.
        contact: Option<crate::physics::CollisionContact>,
    },

    // Animation event
    AnimationFlagTriggered {
        motion_flags: MotionFlags,
    },
    AnimationCompleted,

    // Gameplay events
    Recharge,
    ProvideForConsumption {
        entity: EntityId,
    }, // propose to consume this entity
    Damage {
        amount: f32,
        /// The blow's direction/point/bone, when the source knows them -
        /// seeds the death ragdoll's reaction.
        impact: Option<DamageImpact>,
    }, // damage the entity

    // The entity heard a noise (gunfire, etc.) at this position - an AI
    // alerts and investigates the source, without the noise being an attack.
    HeardNoise {
        origin: Vector3<f32>,
    },

    // AI Signal
    Signal {
        name: String,
    },

    // Debug: force an AI's alertness level (clamped by its alert cap).
    // `pin` holds it against decay (and tracks the live player) until a
    // non-pinned SetAlertness clears it.
    SetAlertness {
        level: dark::properties::AIAlertLevel,
        pin: bool,
    },

    Slay, // kill the entity

    // Interaction events
    // Raw hover event
    Hover {
        held_entity_id: Option<EntityId>,
        world_position: Vector3<f32>,
        is_triggered: bool,
        is_grabbing: bool,
        hand: Handedness,
    },
    // GUI Events... distilled by the raw hover event
    GUIHover {
        held_entity_id: Option<EntityId>,
        screen_coordinates: Point2<f32>,
        is_triggered: bool,
        is_grabbing: bool,
        hand: Handedness,
    },

    // VR Interactions
    TriggerPull,    // player started pulling the trigger
    TriggerRelease, // player stopped pulling the trigger
    Hold,
    Drop,

    TurnOn {
        from: EntityId,
    },
    TurnOff {
        from: EntityId,
    },
    /// A security device has raised an alarm; the linked ecology switches to
    /// its alert-column population profile.
    Alarm {
        from: EntityId,
    },
    /// Clear an active security alarm and return linked devices/ecologies to
    /// their normal state.
    Reset {
        from: EntityId,
    },
    /// Replace a moving-terrain elevator's next waypoint. This is deliberately
    /// distinct from `TurnOn`, whose BaseElevator meaning is merely "advance
    /// to the next sequential TPath node."
    RerouteElevator {
        target_waypoint: EntityId,
    },
}

#[derive(Clone, Debug)]
pub struct Message {
    pub payload: MessagePayload,
    pub to: EntityId,
}

/// One script-owned, versioned payload.
///
/// The payload deliberately remains data, rather than a serialized trait
/// object. Each opting-in script owns its schema and must either restore the
/// saved version or reject it explicitly.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ScriptState {
    pub version: u32,
    pub payload: serde_json::Value,
}

impl ScriptState {
    pub fn encode<T: Serialize>(
        version: u32,
        payload: &T,
        script_key: &str,
    ) -> Result<Self, ScriptStateError> {
        serde_json::to_value(payload)
            .map(|payload| Self { version, payload })
            .map_err(|error| ScriptStateError::InvalidPayload {
                script_key: script_key.to_owned(),
                message: error.to_string(),
            })
    }

    pub fn decode<T: DeserializeOwned>(
        &self,
        supported_version: u32,
        script_key: &str,
    ) -> Result<T, ScriptStateError> {
        if self.version != supported_version {
            return Err(ScriptStateError::UnsupportedVersion {
                script_key: script_key.to_owned(),
                found: self.version,
                supported: supported_version,
            });
        }
        serde_json::from_value(self.payload.clone()).map_err(|error| {
            ScriptStateError::InvalidPayload {
                script_key: script_key.to_owned(),
                message: error.to_string(),
            }
        })
    }
}

/// Collision-safe identity within one entity's script tree.
///
/// `path` contains the top-level `ScriptWorld` ordinal followed by every
/// `CompositeScript` child ordinal. `script_key` is an explicit stable name
/// owned by the script implementation; the path distinguishes duplicate keys.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ScriptStateIdentity {
    pub script_key: String,
    pub path: Vec<u32>,
}

/// Save-file envelope for one script instance.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SavedScriptState {
    /// Entity ID in the pre-save world. It is remapped before hydration.
    pub entity_id: u64,
    pub identity: ScriptStateIdentity,
    pub state: ScriptState,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ScriptStateError {
    UnsupportedVersion {
        script_key: String,
        found: u32,
        supported: u32,
    },
    InvalidPayload {
        script_key: String,
        message: String,
    },
    InvalidEntityId(u64),
    MissingEntityReference(u64),
    MissingOwner(u64),
    MissingScript {
        entity_id: u64,
        identity: ScriptStateIdentity,
    },
    DuplicateState {
        entity_id: u64,
        path: Vec<u32>,
    },
    StateHookMissing(String),
}

impl fmt::Display for ScriptStateError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedVersion {
                script_key,
                found,
                supported,
            } => write!(
                formatter,
                "script '{script_key}' has unsupported version {found} (supported: {supported})"
            ),
            Self::InvalidPayload {
                script_key,
                message,
            } => write!(
                formatter,
                "invalid state for script '{script_key}': {message}"
            ),
            Self::InvalidEntityId(entity_id) => {
                write!(
                    formatter,
                    "saved script state has invalid entity ID {entity_id}"
                )
            }
            Self::MissingEntityReference(entity_id) => write!(
                formatter,
                "saved script state references entity ID {entity_id}, which was not restored"
            ),
            Self::MissingOwner(entity_id) => write!(
                formatter,
                "saved script state owner {entity_id} was not restored"
            ),
            Self::MissingScript {
                entity_id,
                identity,
            } => write!(
                formatter,
                "saved script '{}' at path {:?} has no matching instance on restored entity {entity_id}",
                identity.script_key, identity.path
            ),
            Self::DuplicateState { entity_id, path } => write!(
                formatter,
                "saved entity {entity_id} has duplicate script state at path {path:?}"
            ),
            Self::StateHookMissing(script_key) => write!(
                formatter,
                "script '{script_key}' declares a state key but does not implement its state hook"
            ),
        }
    }
}

impl std::error::Error for ScriptStateError {}

/// Entity-ID remapping available while a script hydrates its payload.
pub struct ScriptRestoreContext<'a> {
    entity_id_map: &'a HashMap<EntityId, EntityId>,
}

impl<'a> ScriptRestoreContext<'a> {
    fn new(entity_id_map: &'a HashMap<EntityId, EntityId>) -> Self {
        Self { entity_id_map }
    }

    /// Convert a pre-save entity ID stored in a script payload to the entity
    /// instantiated for the loaded world. Missing references are errors, not
    /// silently stale handles.
    pub fn remap_entity(&self, saved_entity_id: u64) -> Result<EntityId, ScriptStateError> {
        let old_entity = EntityId::from_inner(saved_entity_id)
            .ok_or(ScriptStateError::InvalidEntityId(saved_entity_id))?;
        self.entity_id_map
            .get(&old_entity)
            .copied()
            .ok_or(ScriptStateError::MissingEntityReference(saved_entity_id))
    }
}

pub trait Script {
    fn initialize(&mut self, _entity_id: EntityId, _world: &World) -> Effect {
        Effect::NoEffect
    }

    fn update(
        &mut self,
        _entity_id: EntityId,
        _world: &World,
        _physics: &PhysicsWorld,
        _time: &Time,
    ) -> Effect {
        Effect::NoEffect
    }

    fn handle_message(
        &mut self,
        _entity_id: EntityId,
        _world: &World,
        _physics: &PhysicsWorld,
        _msg: &MessagePayload,
    ) -> Effect {
        Effect::NoEffect
    }

    /// Stable opt-in identity for private runtime state. Entity properties and
    /// links do not belong here: they are already serialized by the ECS save
    /// layer. Return a key only for state owned exclusively by this script.
    fn script_state_key(&self) -> Option<&'static str> {
        None
    }

    /// Serialize this script's current private state. An opting-in script must
    /// always emit an envelope, including for its default/idle state, so load
    /// can distinguish hydration from a legacy save with no script state.
    fn save_state(&self) -> Result<ScriptState, ScriptStateError> {
        Err(ScriptStateError::StateHookMissing(
            self.script_state_key()
                .unwrap_or("<unregistered>")
                .to_owned(),
        ))
    }

    /// Restore private state before the first update. Payloads containing
    /// entity IDs must use `context` instead of retaining pre-save handles.
    fn restore_state(
        &mut self,
        _state: &ScriptState,
        _context: &ScriptRestoreContext<'_>,
    ) -> Result<(), ScriptStateError> {
        Err(ScriptStateError::StateHookMissing(
            self.script_state_key()
                .unwrap_or("<unregistered>")
                .to_owned(),
        ))
    }

    /// Fresh instances run `initialize`. Hydrated instances skip its
    /// fresh-session side effects because `restore_state` has already supplied
    /// their complete private state. Composite scripts override this to make
    /// the decision independently for each child.
    #[doc(hidden)]
    fn initialize_after_hydration(
        &mut self,
        entity_id: EntityId,
        world: &World,
        hydrated: bool,
    ) -> Effect {
        if hydrated {
            Effect::NoEffect
        } else {
            self.initialize(entity_id, world)
        }
    }

    #[doc(hidden)]
    fn collect_script_states(
        &self,
        entity_id: EntityId,
        path: &mut Vec<u32>,
        output: &mut Vec<SavedScriptState>,
    ) -> Result<(), ScriptStateError> {
        if let Some(script_key) = self.script_state_key() {
            output.push(SavedScriptState {
                entity_id: entity_id.inner(),
                identity: ScriptStateIdentity {
                    script_key: script_key.to_owned(),
                    path: path.clone(),
                },
                state: self.save_state()?,
            });
        }
        Ok(())
    }

    #[doc(hidden)]
    fn restore_script_state(
        &mut self,
        path: &[u32],
        saved: &SavedScriptState,
        context: &ScriptRestoreContext<'_>,
    ) -> Result<bool, ScriptStateError> {
        if !path.is_empty() || self.script_state_key() != Some(saved.identity.script_key.as_str()) {
            return Ok(false);
        }
        self.restore_state(&saved.state, context)?;
        Ok(true)
    }
}

struct UnimplementedScript {
    name: String,
}

impl Script for UnimplementedScript {
    fn handle_message(
        &mut self,
        _entity_id: EntityId,
        _world: &World,
        _physics: &PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        match msg {
            MessagePayload::Collided { with, .. } => {
                info!("ignoring collision with {:?}", *with);
                Effect::NoEffect
            }
            _ => {
                warn!("Unimplemented script: {}", self.name);
                Effect::NoEffect
            }
        }
    }
}

struct ScriptInstance {
    script: Box<dyn Script>,
    hydrated: bool,
}

impl ScriptInstance {
    fn fresh(script: Box<dyn Script>) -> Self {
        Self {
            script,
            hydrated: false,
        }
    }
}

pub struct CompositeScript {
    scripts: Vec<ScriptInstance>,
}

impl CompositeScript {
    pub fn new(scripts: Vec<Box<dyn Script>>) -> CompositeScript {
        CompositeScript {
            scripts: scripts.into_iter().map(ScriptInstance::fresh).collect(),
        }
    }
}

impl Script for CompositeScript {
    fn initialize(&mut self, entity_id: EntityId, world: &World) -> Effect {
        let effects = self
            .scripts
            .iter_mut()
            .map(|instance| instance.script.initialize(entity_id, world))
            .collect();

        Effect::combine(effects)
    }

    fn update(
        &mut self,
        entity_id: EntityId,
        world: &World,
        physics: &PhysicsWorld,
        time: &Time,
    ) -> Effect {
        let effects = self
            .scripts
            .iter_mut()
            .map(|instance| instance.script.update(entity_id, world, physics, time))
            .collect();

        Effect::combine(effects)
    }

    fn handle_message(
        &mut self,
        entity_id: EntityId,
        world: &World,
        physics: &PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        let effects = self
            .scripts
            .iter_mut()
            .map(|instance| {
                instance
                    .script
                    .handle_message(entity_id, world, physics, msg)
            })
            .collect();

        Effect::combine(effects)
    }

    fn initialize_after_hydration(
        &mut self,
        entity_id: EntityId,
        world: &World,
        _hydrated: bool,
    ) -> Effect {
        Effect::combine(
            self.scripts
                .iter_mut()
                .map(|instance| {
                    instance
                        .script
                        .initialize_after_hydration(entity_id, world, instance.hydrated)
                })
                .collect(),
        )
    }

    fn collect_script_states(
        &self,
        entity_id: EntityId,
        path: &mut Vec<u32>,
        output: &mut Vec<SavedScriptState>,
    ) -> Result<(), ScriptStateError> {
        for (index, instance) in self.scripts.iter().enumerate() {
            path.push(index as u32);
            instance
                .script
                .collect_script_states(entity_id, path, output)?;
            path.pop();
        }
        Ok(())
    }

    fn restore_script_state(
        &mut self,
        path: &[u32],
        saved: &SavedScriptState,
        context: &ScriptRestoreContext<'_>,
    ) -> Result<bool, ScriptStateError> {
        let Some((&child_index, child_path)) = path.split_first() else {
            return Ok(false);
        };
        let Some(instance) = self.scripts.get_mut(child_index as usize) else {
            return Ok(false);
        };
        let restored = instance
            .script
            .restore_script_state(child_path, saved, context)?;
        if restored {
            instance.hydrated = true;
        }
        Ok(restored)
    }
}

impl UnimplementedScript {
    pub fn new(name: &str) -> UnimplementedScript {
        UnimplementedScript {
            name: name.to_owned(),
        }
    }
}
struct PanicOnLoadScript {
    name: String,
}

impl PanicOnLoadScript {
    #[allow(dead_code)]
    pub fn new(name: &str) -> PanicOnLoadScript {
        PanicOnLoadScript {
            name: name.to_owned(),
        }
    }
}

#[allow(dead_code)]
struct PanicOnMessageScript;
impl Script for PanicOnMessageScript {
    fn handle_message(
        &mut self,
        _entity_id: EntityId,
        _world: &World,
        _physics: &PhysicsWorld,
        _msg: &MessagePayload,
    ) -> Effect {
        panic!("Panic for this script");
    }
}

impl Script for PanicOnLoadScript {
    fn initialize(&mut self, _entity_id: EntityId, _world: &World) -> Effect {
        panic!("Unimplemented script: {}", self.name);
    }
    fn handle_message(
        &mut self,
        _entity_id: EntityId,
        _world: &World,
        _physics: &PhysicsWorld,
        _msg: &MessagePayload,
    ) -> Effect {
        panic!("Unimplemented script: {}", self.name);
    }
}
struct NoopScript {}
impl NoopScript {
    pub fn new() -> NoopScript {
        NoopScript {}
    }
}
impl Script for NoopScript {
    fn handle_message(
        &mut self,
        _entity_id: EntityId,
        _world: &World,
        _physics: &PhysicsWorld,
        _msg: &MessagePayload,
    ) -> Effect {
        trace!("message {:?} sent to NoopScript, unhandled", _msg);
        Effect::NoEffect
    }
}

pub struct ScriptWorld {
    entity_has_initialized: HashMap<EntityId, bool>,
    entity_to_scripts: HashMap<EntityId, Vec<ScriptInstance>>,
    message_queue: Vec<Message>,
    /// Floating damage readouts earned by the messages dispatched here, drawn
    /// by the mission's render pass. Owned by the script world so they die
    /// with their scene - a readout is a world point in one level.
    damage_popups: Vec<crate::damage_overlay::DamagePopup>,
}

impl ScriptWorld {
    /// The floating damage readouts recorded so far, for the render pass to
    /// draw and age out.
    pub(crate) fn damage_popups(&mut self) -> &mut Vec<crate::damage_overlay::DamagePopup> {
        &mut self.damage_popups
    }

    pub fn new() -> ScriptWorld {
        ScriptWorld {
            entity_has_initialized: HashMap::new(),
            entity_to_scripts: HashMap::new(),
            message_queue: Vec::new(),
            damage_popups: Vec::new(),
        }
    }

    fn create_script(script_name: String) -> Box<dyn Script> {
        match script_name.to_ascii_lowercase().as_str() {
            // PROJECTILE stuff
            "lasershot" => Box::new(NoopScript::new()),
            "timedgrenade" => Box::new(NoopScript::new()),
            "transluceinoutprop" => Box::new(NoopScript::new()),

            // KEYCARD stuff
            // The unlock trap sets the lock state of the objects it controls
            // via SwitchLinks (eng1's Unlock Trap hands out the elevator /
            // grav-lift call buttons this way).
            "trapunlock" => Box::new(TrapUnlock::new()),

            "createsound" => Box::new(CreateSound::new()),

            // INTERACTIVE stuff
            "trapslayer" => Box::new(TrapSlayer::new()),
            "deadpowercell" => Box::new(DeadPowerCell::new()),
            "energystation" => Box::new(EnergyStation::new()),
            "toolconsumable" => Box::new(ToolConsumable::new()),

            // AI stuff
            "trapsignal" => Box::new(TrapSignal::new()),
            "gooegg" => Box::new(Tweqable::new()),
            "grubegg" => Box::new(Tweqable::new()),
            "swarmeregg" => Box::new(Tweqable::new()),
            "containerscript" => gui_script(Box::new(ContainerGui::loot_container())),

            "lootable" => Box::new(NoopScript::new()),

            // Weapons
            "weaponscript" => Box::new(CompositeScript::new(vec![
                Box::new(WeaponScript::new()),
                Box::new(InternalSwitchHeldModelScript::new()),
            ])),
            // Attached by entity_creator to every PropLimbModel weapon. The
            // held-model swap rides along because most melee weapons (Electro
            // Shock, Crystal Shard, PsiSword) author no WeaponScript, so this
            // marker is their only script. The Wrench gets a second copy via
            // its WeaponScript composite - harmless, the swap is idempotent
            // (InternalPropOriginalModelName is written once at creation).
            "internal_triggered_melee_weapon" => Box::new(CompositeScript::new(vec![
                Box::new(HeldMeleeWeapon::new()),
                Box::new(InternalSwitchHeldModelScript::new()),
            ])),
            "pistolmodify" => Box::new(NoopScript::new()),

            // TODO: Necessary
            "changeinterface" => Box::new(NoopScript::new()),
            "reducehp" => Box::new(NoopScript::new()),
            "engineremoverad" => Box::new(NoopScript::new()),
            "radroom" => Box::new(NoopScript::new()),
            // SHODAN's ordinary creature/base-monster scripts own combat and
            // death state. Keep the retail finale hook inert until its ending
            // sequence is implemented; TrapSpawn must still be able to create
            // the authored Avatar.
            "shodandeath" => Box::new(NoopScript::new()),
            "trapspawn" => Box::new(TrapSpawn::new()),
            // ops1 cutscene (the "Polito is SHODAN" reveal) - see scripts/cs9.rs
            "transluceinoutholo" => Box::new(TransluceInOutHolo::new()),
            "cs9_doorreporter" => Box::new(CS9DoorReporter::new()),
            "cs9_eggsandgrubs" => Box::new(CS9EggsAndGrubs::new()),
            "cs9_mastercontrol" => Box::new(CS9MasterControl::new()),
            "cs9_holorumbler" => Box::new(CS9HoloRumbler::new()),
            // Screens fade in/out (alpha; the original uses an extra-light ramp +
            // ScreenSwap model swaps - future polish) and relay to their copies.
            "cs9_shodanscreen" => Box::new(CS9ShodanScreen::new()),
            "sitdownrightnowmp" => Box::new(SitDownRightNowMP::new()),
            "trapdestroyteleport" => Box::new(TrapDestroyTeleport::new()),
            "triggerdamage" => Box::new(TriggerDamage::new()),
            // many.micontain
            "brain" => Box::new(NoopScript::new()),
            "wormheartimplant" => Box::new(NoopScript::new()),
            "wormskin" => Box::new(NoopScript::new()),
            // shodan.mis
            "toggleshodantexture" => Box::new(NoopScript::new()),
            "changedelay" => Box::new(NoopScript::new()), //?
            "shodanhead" => Box::new(NoopScript::new()),  //?
            "shodanshield" => Box::new(NoopScript::new()), //?
            "seatplayer" => Box::new(TrapTeleportPlayer::new()), // This is used in final battle -does this do anything else besides teleport?
            "dieshodandie" => Box::new(DieShodanDie::new()),
            "teleportpath" => Box::new(NoopScript::new()), // end cutscene!
            "translucebydamage" => Box::new(NoopScript::new()), // end cutscene!

            // TODO: Should these actually be implemented?
            "earthtext" => Box::new(NoopScript::new()),
            "trapgravity" => Box::new(NoopScript {}), // medsci1 - vent that falls
            "trapmessage" => Box::new(NoopScript {}), // eng2 - installing override. What prop for message? Where to load string?
            "charmable" => Box::new(NoopScript::new()),
            "transientcorpse" => Box::new(NoopScript::new()),
            "whiteout" => Box::new(WhiteOut::new()),
            "vaporizeinventory" => Box::new(VaporizeInventory::new()),

            // Internal
            "internal_collision_type" => Box::new(InternalCollisionType::new()),
            "internal_inventory" => gui_script(Box::new(ContainerGui::inv_container())),
            "internal_map" => gui_script(Box::new(MapGui)),
            "internal_media" => gui_script(Box::new(MediaGui)),
            // "internal_inventory" => Box::new(PanicOnLoadScript::new("internal_inventory")),
            "internal_explosion" => Box::new(InternalExplosion::new()),
            "internal_radiation_source" => Box::new(InternalRadiationSource),
            "internal_frob_move" => Box::new(InternalFrobMove::new()),
            "internal_keycard" => Box::new(KeyCardScript::new()),
            "internal_nanites" => Box::new(InternalNanitesScript::new()),
            "internal_room_trigger" => Box::new(RoomTrigger::new()),
            "internal_simple_health" => Box::new(InternalSimpleHealth::new()),
            // Implemented
            "basebutton" => Box::new(BaseButton::new()),
            "baseelevator" => Box::new(BaseElevator::new()),
            "destroyallbyname" => Box::new(DestroyAllByName::new()),
            "levelchangebutton" => Box::new(LevelChangeButton::new()),
            "logdiscscript" => gui_script(Box::new(MediaGui)),
            "oncerouter" => Box::new(OnceRouter::new()),
            "stddoor" => Box::new(StdDoor::new()),
            "trapdelay" => Box::new(TrapDelay::new()),
            "trapterminator" => Box::new(TrapDestroyer::new()), // TODO: What is the difference between Terminate vs Destroyer vs Destroy?
            "trapdestroyer" => Box::new(TrapDestroyer::new()),
            "trapdestroy" => Box::new(TrapDestroyer::new()),
            "trapemail" => Box::new(TrapEmail::new()),
            "trapexponce" => Box::new(TrapEXPOnce::new()),
            "trapinverter" => Box::new(TrapInverter::new()),
            "trapnewtripwire" => Box::new(TrapNewTripwire::new()),
            "trapofffilter" => Box::new(TrapOffFilter::new()),
            "traponfilter" => Box::new(TrapOffFilter::new()),
            "trapteleportplayer" => Box::new(TrapTeleportPlayer::new()),
            "traprouter" => Box::new(TrapRouter::new()),
            "trapsound" => Box::new(TrapSound::new()),
            "trapsoundamb" => Box::new(TrapSound::new()),
            "trapteleport" => Box::new(TrapTeleport::new()),
            "traptriplevel" => Box::new(TrapTripLevel::new()),
            "triggermulti" => Box::new(TriggerMulti::new()), // nacelle control
            "triggercollide" => Box::new(TriggerCollide::new()),
            "usesound" => Box::new(UseSound::new()),

            // TWEQ stuff
            "traptweq" => Box::new(CompositeScript::new(vec![
                Box::new(Tweqable::new()),
                Box::new(TrapTweq::new()),
            ])),
            "tweqdepressable" => Box::new(TweqDepressable::new()), // send tweqstart, then tweqoff?
            "tweqable" => Box::new(Tweqable::new()),               // send tweqstart on frob?
            "modelswappable" => Box::new(Tweqable::new()), // send tweqstart on signal on? eng1: fluidics control? same as traptweq?
            "tweqlockedbutton" => Box::new(CompositeScript::new(vec![
                Box::new(BaseButton::new()),
                Box::new(TweqDepressable::new()),
            ])),
            "objconsumebutton" => Box::new(CompositeScript::new(vec![
                Box::new(ObjConsumeButton::new()),
                Box::new(Tweqable::new()),
            ])),

            // Rooms:
            "baseroom" => Box::new(base_room::BaseRoom::new()),
            "coreroom" => Box::new(CoreRoom::new()),
            "onceroom" => Box::new(OnceRoom::new()),
            "emailroom" => Box::new(TrapEmail::new()),
            "zerogravroom" => Box::new(CoreRoom::new()),

            // Questbits:
            "frobqb" => Box::new(FrobQB::new()),
            "trapqbnegfilter" => Box::new(TrapQBNegFilter::new()),
            "trapqbfilter" => Box::new(TrapQBFilter::new()),
            "trapqbset" => Box::new(TrapQBSet::new()),
            "trapquestbitsimple" => Box::new(TrapQuestbitSimple::new()),

            // SimpleLevelChangeButton is the ungated frob-to-travel variant of the
            // Level Change Button: on frob it reads the object's PropDestLevel/PropDestLoc
            // and transitions the level, identical to LevelChangeButton. Objects override
            // their template's LevelChangeButton with this to select the "simple" behavior
            // (e.g. the rec1 tram and the Rickenbacker shuttle buttons), so reuse the same
            // implemented script rather than duplicating it.
            "simplelevelchangebutton" => Box::new(LevelChangeButton::new()),

            // TODO:
            "freezefx" => Box::new(UnimplementedScript::new(&script_name)), // command1
            "torpedolift" => Box::new(UnimplementedScript::new(&script_name)), // rick1
            "torpedohack" => Box::new(UnimplementedScript::new(&script_name)), // rick1
            "eraseradiation" => Box::new(UnimplementedScript::new(&script_name)), // rick1

            // station:
            "oldstylebaseelevator" => Box::new(UnimplementedScript::new(&script_name)),
            "choosemission" => Box::new(ChooseMissionScript::new()),

            // "trapquestbit" => Box::new(UnimplementedScript {
            //     name: "trapquestbit".to_owned(),
            // }),

            // TODO:

            // partially implemented:
            "keypadunhackable" => gui_script(Box::new(KeyPadGui)),
            "keypad" => gui_script(Box::new(KeyPadGui)),
            "securitycomputer" => Box::new(UnimplementedScript::new(&script_name)),
            "resurrectmachine" => Box::new(BaseButton {}),
            "twostatebutton" => Box::new(BaseButton::new()),

            // weapons:
            "delaygrenade" => Box::new(UnimplementedScript::new(&script_name)),
            "annelidmodify" => Box::new(UnimplementedScript::new(&script_name)),
            "empmodify" => Box::new(NoopScript::new()),
            "lasermodify" => Box::new(NoopScript::new()),
            "fusionmodify" => Box::new(NoopScript::new()),
            "riflemodify" => Box::new(NoopScript::new()),
            "stasismodify" => Box::new(NoopScript::new()),
            "shotgunmodify" => Box::new(NoopScript::new()),
            "energyweapon" => Box::new(EnergyWeapon::new()),
            "grenademodify" => Box::new(NoopScript::new()),
            "weapontrainer" => gui_script(Box::new(TrainerGui::new(TrainerMode::Weapons))),
            "wrench" => Box::new(CompositeScript::new(vec![
                Box::new(MeleeWeapon::new()),
                Box::new(InternalSwitchHeldModelScript::new()),
            ])),
            "psiampscript" => Box::new(CompositeScript::new(vec![
                Box::new(PsiAmpScript::new()),
                Box::new(InternalSwitchHeldModelScript::new()),
            ])),
            "viralmodify" => Box::new(UnimplementedScript::new(&script_name)),

            //goodies:
            "expcookie" => Box::new(ExpCookie::new()), // cyber modules
            "medkitscript" => Box::new(HealingItemScript::new(HealingItemKind::MedicalKit)),
            "speedpatch" => Box::new(UnimplementedScript::new(&script_name)), // speed boost
            "radpatch" => Box::new(RadPatchScript),
            "autoinstallsoft" => Box::new(AutoInstallSoft::new()), // auto install software
            "strboost" => Box::new(UnimplementedScript::new(&script_name)), // strength boost
            "intboost" => Box::new(UnimplementedScript::new(&script_name)),
            "statboostimplant" => Box::new(UnimplementedScript::new(&script_name)),

            // earth:
            "comestible" => Box::new(Comestible::new()),
            "liquor" => Box::new(NoopScript::new()),

            // Not implemented - new medsci1 ones:
            "apparition" => Box::new(CompositeScript::new(vec![
                Box::new(BaseMonster::new()),
                Box::new(apparition::Apparition::new()),
            ])),
            "ectoplasm" => Box::new(UnimplementedScript::new(&script_name)),
            "medpatchscript" => Box::new(HealingItemScript::new(HealingItemKind::MedPatch)),
            "psikitscript" => Box::new(PsiKitScript::new()),
            "computer" => gui_script(Box::new(ComputerGui)),
            "lightsoundon" => Box::new(NoopScript::new()),
            "hackablecrate" => gui_script(Box::new(HackableCrateGui::new())),
            "turret" => Box::new(UnimplementedScript::new(&script_name)),
            "triggerdestroy" => Box::new(TriggerDestroy::new()),

            // skill point machines
            "psitrainer" => gui_script(Box::new(TrainerGui::new(TrainerMode::Psi))),
            "techtrainer" => gui_script(Box::new(TrainerGui::new(TrainerMode::Tech))),
            "statstrainer" => gui_script(Box::new(TrainerGui::new(TrainerMode::Stats))),
            "traitmachine" => gui_script(Box::new(TraitGui)),

            // medsci2
            // Keycard in watt's office
            "minigameboy" => gui_script(Box::new(GamePigGui)),
            "minigamecart" => Box::new(NoopScript::new()),
            "forcedoor" => Box::new(UnimplementedScript::new(&script_name)),
            "wormpilescript" => Box::new(UnimplementedScript::new(&script_name)),
            "trapradcleanse" => Box::new(UnimplementedScript::new(&script_name)),
            "armorscript" => Box::new(NoopScript::new()),
            "battery" => Box::new(UnimplementedScript::new(&script_name)),
            "healingstation" => Box::new(UnimplementedScript::new(&script_name)),
            "brokenhealingstation" => Box::new(UnimplementedScript::new(&script_name)),

            // eng1
            "healinggland" => Box::new(UnimplementedScript::new(&script_name)),
            "researchableusescript" => Box::new(UnimplementedScript::new(&script_name)),
            "beakerscript" => Box::new(UnimplementedScript::new(&script_name)),
            "trapmetapropbylist" => Box::new(NoopScript::new()),

            // eng2
            "overlord" => Box::new(UnimplementedScript::new(&script_name)),
            "freemodify" => Box::new(UnimplementedScript::new(&script_name)),
            "manybrain" => Box::new(UnimplementedScript::new(&script_name)),
            "trapsuicide" => Box::new(UnimplementedScript::new(&script_name)),
            // many ride?
            "paralyzeplayers" => Box::new(ParalyzePlayers::new()),
            "standupagain" => Box::new(StandUpAgain::new()),
            "sitdownrightnow" => Box::new(SitDownRightNow::new()),

            // hydro1
            "transluceinout" => Box::new(UnimplementedScript::new(&script_name)),
            "freerepair" => Box::new(UnimplementedScript::new(&script_name)),
            "cancerstick" => Box::new(UnimplementedScript::new(&script_name)),

            // hydro2
            "trapparticle" => Box::new(UnimplementedScript::new(&script_name)),
            // The ICE Pick. It carries no behavior of its own: the object it
            // is applied to claims it off the tool channel (see
            // `HackableCrateGui::on_provide_for_consumption`).
            "freehack" => Box::new(NoopScript::new()),

            // hydro3
            "poweredarmor" => Box::new(UnimplementedScript::new(&script_name)),

            // ops2
            "slotmachine" => Box::new(UnimplementedScript::new(&script_name)),

            // rec1
            // elevator buttons
            "elevatorbutton" => gui_script(Box::new(ElevatorGui)),
            "pictureswap" => Box::new(PictureSwap::new()),
            "testimplant" => Box::new(UnimplementedScript::new(&script_name)),

            // ric2:
            "shakeyourbooty" => Box::new(UnimplementedScript::new(&script_name)), // what does this one do?

            // command1: some crazy scripts here
            "rerouteelevatorbutton" => Box::new(RerouteElevatorButton::new()),
            "trapambientoff" => Box::new(NoopScript::new()),
            "trapcollideoff" => Box::new(NoopScript::new()),
            "tweqbutton" => Box::new(NoopScript::new()),
            "tweqtrap" => Box::new(NoopScript::new()),
            "putbombinreplicator" => Box::new(PutBombInReplicator::new()),
            "trapunref" => Box::new(NoopScript::new()),

            // shodan
            // TODO: What's the difference between base elevator / dont stop elevator?
            "dontstopelevator" => Box::new(BaseElevator::continuous()),

            // Not implemented
            // TODO: Handle keypad code
            "ammoscript" => Box::new(NoopScript::new()),
            //"BaseElevator" => Box::new(UnimplementedScript::new(&name)),
            "baselight" => Box::new(BaseLight::new()),
            //"baseai" => Box::new(PanicOnLoadScript::new(&script_name)),
            "baseai" => Box::new(NoopScript::new()),
            "basemonster" => Box::new(BaseMonster::new()),
            "cameraalert" => Box::new(CameraAlert::new()),
            "cameradeath" => Box::new(UnimplementedScript::new(&script_name)),
            "censor" => Box::new(UnimplementedScript::new(&script_name)),
            "censorme" => Box::new(UnimplementedScript::new(&script_name)),
            "creaturecontainer" => gui_script(Box::new(ContainerGui::loot_creature())),
            "chemical" => Box::new(ChemicalScript::new()),
            "chooseservice" => Box::new(ChooseServiceScript::new()),
            "infocomputer" => Box::new(UnimplementedScript::new(&script_name)),
            "reducepsi" => Box::new(ReducePsi::new()),
            "replicatorscript" => gui_script(Box::new(ReplicatorGui)),
            "researchablescript" => Box::new(CompositeScript::new(vec![
                Box::new(ResearchableScript::new()),
                gui_script(Box::new(ResearchGui)),
            ])),
            "setupinitialdebrief" => {
                Box::new(setup_initial_debrief::SetupInitialDebriefScript::new())
            }
            "toxinpatch" => Box::new(UnimplementedScript::new(&script_name)),
            "triggerecology" => Box::new(TriggerEcology::new()),
            // Retail's difficulty variant applies shock.cfg/difficulty
            // population adjustments before entering this same state machine.
            // The port has neither setting yet, so its authored baseline is
            // identical to TriggerEcology.
            "triggerecologydiff" => Box::new(TriggerEcology::new()),
            "unhackhack" => Box::new(UnimplementedScript::new(&script_name)),
            _ => Box::new(PanicOnLoadScript { name: script_name }),
        }
    }

    pub fn add_entity(&mut self, entity_id: EntityId, script_name: &str) {
        let script = Self::create_script(script_name.to_ascii_lowercase());
        self.add_entity2(entity_id, script);
    }

    pub fn add_entity2(&mut self, entity_id: EntityId, script: Box<dyn Script>) {
        self.entity_to_scripts
            .entry(entity_id)
            .or_default()
            .push(ScriptInstance::fresh(script));

        self.entity_has_initialized.insert(entity_id, false);
    }

    pub fn remove_entity(&mut self, entity_id: EntityId) {
        self.entity_to_scripts.remove(&entity_id);
        self.entity_has_initialized.remove(&entity_id);
    }

    pub fn dispatch(&mut self, message: Message) {
        self.message_queue.push(message);
    }

    /// Snapshot all opting-in private script state. The output is sorted so
    /// identical worlds produce reviewable, deterministic save JSON despite
    /// `HashMap` iteration order.
    pub fn save_states(&self) -> Result<Vec<SavedScriptState>, ScriptStateError> {
        let mut output = Vec::new();
        for (entity_id, scripts) in &self.entity_to_scripts {
            for (index, instance) in scripts.iter().enumerate() {
                let mut path = vec![index as u32];
                instance
                    .script
                    .collect_script_states(*entity_id, &mut path, &mut output)?;
            }
        }
        output.sort_by(|left, right| {
            left.entity_id
                .cmp(&right.entity_id)
                .then_with(|| left.identity.path.cmp(&right.identity.path))
                .then_with(|| left.identity.script_key.cmp(&right.identity.script_key))
        });
        Ok(output)
    }

    /// Hydrate matching script instances before their first update.
    ///
    /// Missing state is intentionally not an error: legacy saves and scripts
    /// which do not opt in retain the existing fresh initialization path.
    pub fn restore_states(
        &mut self,
        saved_states: &[SavedScriptState],
        entity_id_map: &HashMap<EntityId, EntityId>,
    ) -> Result<(), ScriptStateError> {
        let context = ScriptRestoreContext::new(entity_id_map);
        let mut restored_paths = HashSet::new();

        for saved in saved_states {
            let old_entity = EntityId::from_inner(saved.entity_id)
                .ok_or(ScriptStateError::InvalidEntityId(saved.entity_id))?;
            let new_entity = entity_id_map
                .get(&old_entity)
                .copied()
                .ok_or(ScriptStateError::MissingOwner(saved.entity_id))?;
            if !restored_paths.insert((new_entity, saved.identity.path.clone())) {
                return Err(ScriptStateError::DuplicateState {
                    entity_id: saved.entity_id,
                    path: saved.identity.path.clone(),
                });
            }

            let Some((&top_index, child_path)) = saved.identity.path.split_first() else {
                return Err(ScriptStateError::MissingScript {
                    entity_id: saved.entity_id,
                    identity: saved.identity.clone(),
                });
            };
            let Some(instance) = self
                .entity_to_scripts
                .get_mut(&new_entity)
                .and_then(|scripts| scripts.get_mut(top_index as usize))
            else {
                return Err(ScriptStateError::MissingScript {
                    entity_id: saved.entity_id,
                    identity: saved.identity.clone(),
                });
            };

            let restored = instance
                .script
                .restore_script_state(child_path, saved, &context)?;
            if !restored {
                return Err(ScriptStateError::MissingScript {
                    entity_id: saved.entity_id,
                    identity: saved.identity.clone(),
                });
            }
            instance.hydrated = true;
        }

        Ok(())
    }

    pub fn update(&mut self, world: &World, physics: &PhysicsWorld, time: &Time) -> Vec<Effect> {
        let mut produced_effects = Vec::new();

        // Initialize any entities that haven't been initialized yet
        for (entity_id, initialized) in self.entity_has_initialized.iter_mut() {
            if !(*initialized) {
                self.entity_to_scripts
                    .entry(*entity_id)
                    .and_modify(|scripts| {
                        for instance in scripts {
                            let eff = instance.script.initialize_after_hydration(
                                *entity_id,
                                world,
                                instance.hydrated,
                            );
                            produced_effects.push(eff);
                        }
                    });

                *initialized = true;
            }
        }

        // Process any incoming messages
        let mut slayed_entities: HashSet<EntityId> = HashSet::new();
        // Collected here and appended after the loop: the queue is borrowed
        // for the duration of it.
        let mut new_damage_popups = Vec::new();
        let span = span!(Level::INFO, "messages");
        let _ = span.enter();
        for msg in &self.message_queue {
            let to_entity_id = msg.to;

            if matches!(msg.payload, MessagePayload::Slay) {
                slayed_entities.insert(to_entity_id);
            }

            // Observability: trace the delivery so headless tooling (the debug
            // runtime's GET /v1/messages/recent) can see what drove scripts on
            // a given frame. High-frequency payloads are filtered out there.
            crate::message_trace::record(
                world,
                time.total.as_secs_f64(),
                to_entity_id,
                &msg.payload,
            );
            // Same choke point feeds the floating damage readouts, so they see
            // every damage path (melee contact, projectiles, hitbox-forwarded
            // hits, script injection) rather than one of them.
            if let Some(popup) = crate::damage_overlay::popup_for(
                world,
                time.total.as_secs_f64(),
                to_entity_id,
                &msg.payload,
            ) {
                new_damage_popups.push(popup);
            }

            let mut is_turn_on = false;
            match msg.payload {
                MessagePayload::TurnOn { from: _ } => is_turn_on = true,
                _ => {}
            }

            if is_turn_on {
                info!("Got turn on message: {}", debug_entity(world, to_entity_id));
            }

            self.entity_to_scripts
                .entry(to_entity_id)
                .and_modify(|scripts| {
                    for instance in scripts {
                        if is_turn_on {
                            info!(
                                "-- processing turn on message: {}",
                                debug_entity(world, to_entity_id)
                            );
                        }
                        let eff = instance.script.handle_message(
                            to_entity_id,
                            world,
                            physics,
                            &msg.payload,
                        );
                        produced_effects.push(eff);
                    }
                });
        }

        for ent in slayed_entities {
            produced_effects.push(Effect::SlayEntity { entity_id: ent })
        }

        self.message_queue.clear();
        self.damage_popups.extend(new_damage_popups);
        crate::damage_overlay::trim(&mut self.damage_popups);

        for (entity_id, scripts) in self.entity_to_scripts.iter_mut() {
            for instance in scripts.iter_mut() {
                let eff = instance.script.update(*entity_id, world, physics, time);
                produced_effects.push(eff);
            }
        }

        let flattened_effects = Effect::flatten(produced_effects);

        // Filter out message effects, add to queue
        // TODO: Is this necessary to filter out and manually queue?
        let mut ret = Vec::new();
        for eff in flattened_effects {
            match eff {
                Effect::Send { msg } if matches!(msg.payload, MessagePayload::Slay) => {
                    let entity_id = msg.to;
                    // Slay is dispatched here instead of through the queue, so
                    // trace it here too - otherwise it is the one event class
                    // missing from GET /v1/messages/recent.
                    crate::message_trace::record(
                        world,
                        time.total.as_secs_f64(),
                        entity_id,
                        &MessagePayload::Slay,
                    );
                    let mut slay_effects = Vec::new();
                    if let Some(scripts) = self.entity_to_scripts.get_mut(&entity_id) {
                        for instance in scripts {
                            slay_effects.push(instance.script.handle_message(
                                entity_id,
                                world,
                                physics,
                                &MessagePayload::Slay,
                            ));
                        }
                    }
                    for slay_effect in Effect::flatten(slay_effects) {
                        match slay_effect {
                            Effect::Send { msg } => self.message_queue.push(msg),
                            other => ret.push(other),
                        }
                    }
                    ret.push(Effect::SlayEntity { entity_id });
                }
                Effect::Send { msg } => self.message_queue.push(msg),
                _ => ret.push(eff),
            }
        }

        ret
    }
}

#[cfg(test)]
mod script_state_tests {
    use std::{
        cell::{Cell, RefCell},
        collections::HashMap,
        rc::Rc,
    };

    use serde::{Deserialize, Serialize};

    use super::*;

    #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
    struct TestState {
        mode: String,
        timer: f32,
        referenced_entity: Option<u64>,
    }

    struct StatefulTestScript {
        state: TestState,
        initialize_count: Rc<Cell<u32>>,
        observed: Rc<RefCell<Option<TestState>>>,
    }

    impl StatefulTestScript {
        fn new(
            mode: &str,
            timer: f32,
            referenced_entity: Option<EntityId>,
            initialize_count: Rc<Cell<u32>>,
            observed: Rc<RefCell<Option<TestState>>>,
        ) -> Self {
            Self {
                state: TestState {
                    mode: mode.to_owned(),
                    timer,
                    referenced_entity: referenced_entity.map(EntityId::inner),
                },
                initialize_count,
                observed,
            }
        }
    }

    impl Script for StatefulTestScript {
        fn initialize(&mut self, _entity_id: EntityId, _world: &World) -> Effect {
            self.initialize_count
                .set(self.initialize_count.get().saturating_add(1));
            self.state = TestState {
                mode: "fresh".to_owned(),
                timer: 0.0,
                referenced_entity: None,
            };
            Effect::NoEffect
        }

        fn update(
            &mut self,
            _entity_id: EntityId,
            _world: &World,
            _physics: &PhysicsWorld,
            _time: &Time,
        ) -> Effect {
            *self.observed.borrow_mut() = Some(self.state.clone());
            Effect::NoEffect
        }

        fn script_state_key(&self) -> Option<&'static str> {
            Some("test.stateful")
        }

        fn save_state(&self) -> Result<ScriptState, ScriptStateError> {
            ScriptState::encode(1, &self.state, "test.stateful")
        }

        fn restore_state(
            &mut self,
            state: &ScriptState,
            context: &ScriptRestoreContext<'_>,
        ) -> Result<(), ScriptStateError> {
            let mut restored: TestState = state.decode(1, "test.stateful")?;
            if let Some(saved_entity) = restored.referenced_entity {
                restored.referenced_entity = Some(context.remap_entity(saved_entity)?.inner());
            }
            self.state = restored;
            Ok(())
        }
    }

    fn tick(scripts: &mut ScriptWorld, world: &World) {
        scripts.update(world, &PhysicsWorld::new(), &Time::default());
    }

    #[test]
    fn put_bomb_in_replicator_turn_on_is_not_a_noop() {
        let mut world = World::new();
        let replicator = world.add_entity(dark::properties::PropReplicatorHackedContents {
            costs: [100, 75, 100, 45, 0, 0],
            object_names: std::array::from_fn(|_| String::new()),
        });
        let mut script = ScriptWorld::create_script("PutBombInReplicator".to_owned());

        let effect = script.handle_message(
            replicator,
            &world,
            &PhysicsWorld::new(),
            &MessagePayload::TurnOn { from: replicator },
        );

        assert!(
            !matches!(effect, Effect::NoEffect),
            "the Command objective relay must add the resonator to the hacked catalog"
        );
    }

    #[test]
    fn hydrated_mode_and_timer_survive_without_replaying_initialize() {
        let mut source_world = World::new();
        let old_entity = source_world.add_entity(());
        let mut source_scripts = ScriptWorld::new();
        source_scripts.add_entity2(
            old_entity,
            Box::new(StatefulTestScript::new(
                "waiting",
                2.75,
                None,
                Rc::new(Cell::new(0)),
                Rc::new(RefCell::new(None)),
            )),
        );
        let saved = source_scripts.save_states().unwrap();

        let mut loaded_world = World::new();
        let _different_generation = loaded_world.add_entity(());
        let new_entity = loaded_world.add_entity(());
        let initialize_count = Rc::new(Cell::new(0));
        let observed = Rc::new(RefCell::new(None));
        let mut loaded_scripts = ScriptWorld::new();
        loaded_scripts.add_entity2(
            new_entity,
            Box::new(StatefulTestScript::new(
                "fresh",
                0.0,
                None,
                initialize_count.clone(),
                observed.clone(),
            )),
        );
        loaded_scripts
            .restore_states(&saved, &HashMap::from([(old_entity, new_entity)]))
            .unwrap();

        tick(&mut loaded_scripts, &loaded_world);

        assert_eq!(initialize_count.get(), 0);
        assert_eq!(
            *observed.borrow(),
            Some(TestState {
                mode: "waiting".to_owned(),
                timer: 2.75,
                referenced_entity: None,
            })
        );
    }

    #[test]
    fn missing_legacy_state_keeps_fresh_initialization_semantics() {
        let mut world = World::new();
        let entity = world.add_entity(());
        let initialize_count = Rc::new(Cell::new(0));
        let observed = Rc::new(RefCell::new(None));
        let mut scripts = ScriptWorld::new();
        scripts.add_entity2(
            entity,
            Box::new(StatefulTestScript::new(
                "stale-constructor-value",
                9.0,
                None,
                initialize_count.clone(),
                observed.clone(),
            )),
        );

        tick(&mut scripts, &world);

        assert_eq!(initialize_count.get(), 1);
        assert_eq!(
            *observed.borrow(),
            Some(TestState {
                mode: "fresh".to_owned(),
                timer: 0.0,
                referenced_entity: None,
            })
        );
    }

    #[test]
    fn duplicate_and_nested_composite_children_have_distinct_paths() {
        let mut source_world = World::new();
        let old_entity = source_world.add_entity(());
        let mut source_scripts = ScriptWorld::new();
        let inert_count = Rc::new(Cell::new(0));
        let inert_observation = Rc::new(RefCell::new(None));
        let stateful = |mode: &str, timer: f32| {
            Box::new(StatefulTestScript::new(
                mode,
                timer,
                None,
                inert_count.clone(),
                inert_observation.clone(),
            )) as Box<dyn Script>
        };
        source_scripts.add_entity2(
            old_entity,
            Box::new(CompositeScript::new(vec![
                stateful("outer-first", 1.0),
                Box::new(CompositeScript::new(vec![
                    stateful("nested-first", 2.0),
                    stateful("nested-second", 3.0),
                ])),
            ])),
        );
        source_scripts.add_entity2(old_entity, stateful("top-duplicate", 4.0));

        let saved = source_scripts.save_states().unwrap();
        assert_eq!(
            saved
                .iter()
                .map(|state| state.identity.path.clone())
                .collect::<Vec<_>>(),
            vec![vec![0, 0], vec![0, 1, 0], vec![0, 1, 1], vec![1]]
        );

        let mut loaded_world = World::new();
        let new_entity = loaded_world.add_entity(());
        let observations: Vec<_> = (0..4).map(|_| Rc::new(RefCell::new(None))).collect();
        let counts: Vec<_> = (0..4).map(|_| Rc::new(Cell::new(0))).collect();
        let loaded = |index: usize| {
            Box::new(StatefulTestScript::new(
                "fresh",
                0.0,
                None,
                counts[index].clone(),
                observations[index].clone(),
            )) as Box<dyn Script>
        };
        let mut loaded_scripts = ScriptWorld::new();
        loaded_scripts.add_entity2(
            new_entity,
            Box::new(CompositeScript::new(vec![
                loaded(0),
                Box::new(CompositeScript::new(vec![loaded(1), loaded(2)])),
            ])),
        );
        loaded_scripts.add_entity2(new_entity, loaded(3));
        loaded_scripts
            .restore_states(&saved, &HashMap::from([(old_entity, new_entity)]))
            .unwrap();

        tick(&mut loaded_scripts, &loaded_world);

        assert!(counts.iter().all(|count| count.get() == 0));
        assert_eq!(
            observations
                .iter()
                .map(|state| state.borrow().as_ref().unwrap().mode.clone())
                .collect::<Vec<_>>(),
            vec![
                "outer-first",
                "nested-first",
                "nested-second",
                "top-duplicate"
            ]
        );
    }

    #[test]
    fn restore_context_remaps_saved_entity_references() {
        let mut source_world = World::new();
        let old_owner = source_world.add_entity(());
        let old_target = source_world.add_entity(());
        let mut source_scripts = ScriptWorld::new();
        source_scripts.add_entity2(
            old_owner,
            Box::new(StatefulTestScript::new(
                "tracking",
                1.0,
                Some(old_target),
                Rc::new(Cell::new(0)),
                Rc::new(RefCell::new(None)),
            )),
        );
        let mut save_data = crate::save_load::EntitySaveData::empty();
        save_data.all_entities = vec![old_owner.inner(), old_target.inner()];
        save_data.script_states = source_scripts.save_states().unwrap();
        let save_data: crate::save_load::EntitySaveData =
            serde_json::from_value(serde_json::to_value(save_data).unwrap()).unwrap();

        let mut loaded_world = World::new();
        let _sentinel = loaded_world.add_entity(());
        let (_, entity_id_map) = save_data.instantiate(&mut loaded_world);
        let new_owner = entity_id_map[&old_owner];
        let new_target = entity_id_map[&old_target];
        let observed = Rc::new(RefCell::new(None));
        let mut loaded_scripts = ScriptWorld::new();
        loaded_scripts.add_entity2(
            new_owner,
            Box::new(StatefulTestScript::new(
                "fresh",
                0.0,
                None,
                Rc::new(Cell::new(0)),
                observed.clone(),
            )),
        );
        loaded_scripts
            .restore_states(&save_data.script_states, &entity_id_map)
            .unwrap();

        tick(&mut loaded_scripts, &loaded_world);

        assert_eq!(
            observed.borrow().as_ref().unwrap().referenced_entity,
            Some(new_target.inner())
        );
    }

    #[test]
    fn unsupported_script_state_version_fails_clearly() {
        let mut world = World::new();
        let entity = world.add_entity(());
        let mut source = ScriptWorld::new();
        source.add_entity2(
            entity,
            Box::new(StatefulTestScript::new(
                "waiting",
                1.0,
                None,
                Rc::new(Cell::new(0)),
                Rc::new(RefCell::new(None)),
            )),
        );
        let mut saved = source.save_states().unwrap();
        saved[0].state.version = 99;

        let mut loaded = ScriptWorld::new();
        loaded.add_entity2(
            entity,
            Box::new(StatefulTestScript::new(
                "fresh",
                0.0,
                None,
                Rc::new(Cell::new(0)),
                Rc::new(RefCell::new(None)),
            )),
        );
        let error = loaded
            .restore_states(&saved, &HashMap::from([(entity, entity)]))
            .unwrap_err();

        assert!(error.to_string().contains("unsupported version 99"));
        assert!(error.to_string().contains("test.stateful"));
    }
}
