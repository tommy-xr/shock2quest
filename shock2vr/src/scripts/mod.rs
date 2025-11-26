pub mod ai;
pub mod effect;
pub mod speech_registry;
pub mod speech_util;

mod base_button;
mod base_elevator;
mod base_monster;
mod choose_mission;
mod choose_service;
mod core_room;
mod create_sound;
mod dead_power_cell;
mod destroy_all_by_name;
mod energy_station;
mod frob_qb;
mod gui;
mod internal_collision_type;
pub mod internal_fast_projectile;
mod internal_keycard_script;
mod internal_simple_health;
mod internal_switch_held_model;
mod level_change_button;
mod logdiscscript;
mod melee_weapon;
mod obj_consume_button;
mod once_room;
mod once_router;
mod room_trigger;
pub mod script_util;
mod setup_initial_debrief;
mod std_door;
mod tool_consumable;
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
mod trap_teleport;
mod trap_teleport_player;
mod trap_trip_level;
mod trap_tweq;
mod trigger_collide;
mod trigger_multi;
mod tweq_depressable;
mod tweqable;
mod use_sound;
mod weapon_script;
use std::collections::{HashMap, HashSet};

use cgmath::{Point2, Vector3};
use dark::motion::MotionFlags;
pub use effect::*;

use shipyard::{EntityId, World};
use tracing::{Level, info, span, warn};

use crate::util::debug_entity;
use crate::vr_config::Handedness;
use crate::{physics::PhysicsWorld, time::Time};

use crate::gui::gui_script;

use self::choose_mission::ChooseMissionScript;
use self::choose_service::ChooseServiceScript;
use self::gui::{ContainerGui, ElevatorGui, GamePigGui, KeyPadGui, ReplicatorGui};
use self::internal_switch_held_model::InternalSwitchHeldModelScript;
use self::trap_signal::TrapSignal;
use self::{
    base_button::BaseButton, base_elevator::BaseElevator, base_monster::BaseMonster, core_room::*,
    create_sound::*, dead_power_cell::DeadPowerCell, destroy_all_by_name::DestroyAllByName,
    energy_station::EnergyStation, frob_qb::FrobQB, internal_collision_type::InternalCollisionType,
    internal_keycard_script::KeyCardScript, internal_simple_health::InternalSimpleHealth,
    level_change_button::LevelChangeButton, logdiscscript::LogDiscScript,
    melee_weapon::MeleeWeapon, obj_consume_button::ObjConsumeButton, once_room::OnceRoom,
    once_router::OnceRouter, room_trigger::RoomTrigger, std_door::StdDoor,
    tool_consumable::ToolConsumable, trap_delay::TrapDelay, trap_destroyer::TrapDestroyer,
    trap_email::TrapEmail, trap_exp_once::TrapEXPOnce, trap_inverter::TrapInverter,
    trap_new_tripwire::TrapNewTripwire, trap_on_filter::TrapOffFilter,
    trap_qb_filter::TrapQBFilter, trap_qb_neg_filter::TrapQBNegFilter, trap_qb_set::TrapQBSet,
    trap_questbit_simple::TrapQuestbitSimple, trap_router::TrapRouter, trap_slayer::TrapSlayer,
    trap_sound::TrapSound, trap_teleport::TrapTeleport, trap_teleport_player::TrapTeleportPlayer,
    trap_trip_level::TrapTripLevel, trap_tweq::TrapTweq, trigger_collide::TriggerCollide,
    trigger_multi::TriggerMulti, tweq_depressable::TweqDepressable, tweqable::Tweqable,
    use_sound::UseSound, weapon_script::WeaponScript,
};

#[derive(Clone, Debug)]
pub enum MessagePayload {
    Frob,

    // Physics events
    SensorBeginIntersect {
        with: EntityId,
    },
    SensorEndIntersect {
        with: EntityId,
    },
    Collided {
        with: EntityId,
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
    }, // damage the entity

    // AI Signal
    Signal {
        name: String,
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
}

#[derive(Clone, Debug)]
pub struct Message {
    pub payload: MessagePayload,
    pub to: EntityId,
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
            MessagePayload::Collided { with } => {
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

pub struct CompositeScript {
    scripts: Vec<Box<dyn Script>>,
}

impl CompositeScript {
    pub fn new(scripts: Vec<Box<dyn Script>>) -> CompositeScript {
        CompositeScript { scripts }
    }
}

impl Script for CompositeScript {
    fn initialize(&mut self, entity_id: EntityId, world: &World) -> Effect {
        let effects = self
            .scripts
            .iter_mut()
            .map(|sc| sc.initialize(entity_id, world))
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
            .map(|sc| sc.update(entity_id, world, physics, time))
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
            .map(|sc| sc.handle_message(entity_id, world, physics, msg))
            .collect();

        Effect::combine(effects)
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
        warn!("message {:?} sent to NoopScript, unhandled", _msg);
        Effect::NoEffect
    }
}

pub struct ScriptWorld {
    entity_has_initialized: HashMap<EntityId, bool>,
    entity_to_scripts: HashMap<EntityId, Vec<Box<dyn Script>>>,
    message_queue: Vec<Message>,
}

impl ScriptWorld {
    pub fn new() -> ScriptWorld {
        ScriptWorld {
            entity_has_initialized: HashMap::new(),
            entity_to_scripts: HashMap::new(),
            message_queue: Vec::new(),
        }
    }

    fn create_script(script_name: String) -> Box<dyn Script> {
        match script_name.to_ascii_lowercase().as_str() {
            // PROJECTILE stuff
            "lasershot" => Box::new(NoopScript::new()),
            "timedgrenade" => Box::new(NoopScript::new()),
            "transluceinoutprop" => Box::new(NoopScript::new()),

            // KEYCARD stuff
            "trapunlock" => Box::new(NoopScript::new()),

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
            "pistolmodify" => Box::new(NoopScript::new()),

            // TODO: Necessary
            "changeinterface" => Box::new(NoopScript::new()),
            "reducehp" => Box::new(NoopScript::new()),
            "engineremoverad" => Box::new(NoopScript::new()),
            "radroom" => Box::new(NoopScript::new()),
            "trapspawn" => Box::new(NoopScript::new()),
            // ops1 cutscene
            "transluceinoutholo" => Box::new(NoopScript::new()),
            "cs9_doorreporter" => Box::new(NoopScript::new()),
            "cs9_eggsandgrubs" => Box::new(NoopScript::new()),
            "cs9_mastercontrol" => Box::new(NoopScript::new()),
            "cs9_holorumbler" => Box::new(NoopScript::new()),
            "cs9_shodanscreen" => Box::new(NoopScript::new()),
            "sitdownrightnowmp" => Box::new(NoopScript::new()),
            "trapdestroyteleport" => Box::new(NoopScript::new()),
            "triggerdamage" => Box::new(NoopScript::new()),
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
            "dieshodandie" => Box::new(NoopScript::new()),       // end cutscene!
            "teleportpath" => Box::new(NoopScript::new()),       // end cutscene!
            "translucebydamage" => Box::new(NoopScript::new()),  // end cutscene!

            // TODO: Should these actually be implemented?
            "earthtext" => Box::new(NoopScript::new()),
            "trapgravity" => Box::new(NoopScript {}), // medsci1 - vent that falls
            "trapmessage" => Box::new(NoopScript {}), // eng2 - installing override. What prop for message? Where to load string?
            "charmable" => Box::new(NoopScript::new()),
            "transientcorpse" => Box::new(NoopScript::new()),
            "whiteout" => Box::new(NoopScript::new()),
            "vaporizeinventory" => Box::new(NoopScript::new()),

            // Internal
            "internal_collision_type" => Box::new(InternalCollisionType::new()),
            "internal_inventory" => gui_script(Box::new(ContainerGui::inv_container())),
            // "internal_inventory" => Box::new(PanicOnLoadScript::new("internal_inventory")),
            "internal_keycard" => Box::new(KeyCardScript::new()),
            "internal_room_trigger" => Box::new(RoomTrigger::new()),
            "internal_simple_health" => Box::new(InternalSimpleHealth::new()),
            // Implemented
            "basebutton" => Box::new(BaseButton::new()),
            "baseelevator" => Box::new(BaseElevator::new()),
            "destroyallbyname" => Box::new(DestroyAllByName::new()),
            "levelchangebutton" => Box::new(LevelChangeButton::new()),
            "logdiscscript" => Box::new(LogDiscScript::new()),
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
            "baseroom" => Box::new(CoreRoom::new()),
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

            // TODO:
            "simplelevelchangebutton" => Box::new(UnimplementedScript::new(&script_name)), // rec1
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
            "energyweapon" => Box::new(NoopScript::new()),
            "grenademodify" => Box::new(NoopScript::new()),
            "weapontrainer" => Box::new(UnimplementedScript::new(&script_name)),
            "wrench" => Box::new(CompositeScript::new(vec![
                Box::new(MeleeWeapon::new()),
                Box::new(InternalSwitchHeldModelScript::new()),
            ])),
            "psiampscript" => Box::new(CompositeScript::new(vec![
                Box::new(WeaponScript::new()),
                Box::new(InternalSwitchHeldModelScript::new()),
            ])),
            "viralmodify" => Box::new(UnimplementedScript::new(&script_name)),

            //goodies:
            "expcookie" => Box::new(UnimplementedScript::new(&script_name)), // cyber modules
            "medkitscript" => Box::new(UnimplementedScript::new(&script_name)), // cyber modules
            "speedpatch" => Box::new(UnimplementedScript::new(&script_name)), // speed boost
            "radpatch" => Box::new(UnimplementedScript::new(&script_name)),  // speed boost
            "autoinstallsoft" => Box::new(UnimplementedScript::new(&script_name)), // auto install software
            "strboost" => Box::new(UnimplementedScript::new(&script_name)),        // strength boost
            "intboost" => Box::new(UnimplementedScript::new(&script_name)),
            "statboostimplant" => Box::new(UnimplementedScript::new(&script_name)),

            // earth:
            "comestible" => Box::new(UnimplementedScript::new(&script_name)),
            "liquor" => Box::new(NoopScript::new()),

            // Not implemented - new medsci1 ones:
            "apparition" => Box::new(UnimplementedScript::new(&script_name)),
            "ectoplasm" => Box::new(UnimplementedScript::new(&script_name)),
            "medpatchscript" => Box::new(UnimplementedScript::new(&script_name)),
            "psikitscript" => Box::new(UnimplementedScript::new(&script_name)),
            "computer" => Box::new(UnimplementedScript::new(&script_name)),
            "lightsoundon" => Box::new(NoopScript::new()),
            "hackablecrate" => Box::new(UnimplementedScript::new(&script_name)),
            "turret" => Box::new(UnimplementedScript::new(&script_name)),
            "triggerdestroy" => Box::new(NoopScript::new()),

            // skill point machines
            "psitrainer" => Box::new(UnimplementedScript::new(&script_name)),
            "techtrainer" => Box::new(UnimplementedScript::new(&script_name)),
            "statstrainer" => Box::new(UnimplementedScript::new(&script_name)),
            "traitmachine" => Box::new(NoopScript::new()),

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
            "paralyzeplayers" => Box::new(UnimplementedScript::new(&script_name)),
            "standupagain" => Box::new(UnimplementedScript::new(&script_name)),
            "sitdownrightnow" => Box::new(UnimplementedScript::new(&script_name)),

            // hydro1
            "transluceinout" => Box::new(UnimplementedScript::new(&script_name)),
            "freerepair" => Box::new(UnimplementedScript::new(&script_name)),
            "cancerstick" => Box::new(UnimplementedScript::new(&script_name)),

            // hydro2
            "trapparticle" => Box::new(UnimplementedScript::new(&script_name)),
            "freehack" => Box::new(UnimplementedScript::new(&script_name)),

            // hydro3
            "poweredarmor" => Box::new(UnimplementedScript::new(&script_name)),

            // ops2
            "slotmachine" => Box::new(UnimplementedScript::new(&script_name)),

            // rec1
            // elevator buttons
            "elevatorbutton" => gui_script(Box::new(ElevatorGui)),
            "pictureswap" => Box::new(NoopScript::new()),
            "testimplant" => Box::new(UnimplementedScript::new(&script_name)),

            // ric2:
            "shakeyourbooty" => Box::new(UnimplementedScript::new(&script_name)), // what does this one do?

            // command1: some crazy scripts here
            "rerouteelevatorbutton" => Box::new(NoopScript::new()),
            "trapambientoff" => Box::new(NoopScript::new()),
            "trapcollideoff" => Box::new(NoopScript::new()),
            "tweqbutton" => Box::new(NoopScript::new()),
            "tweqtrap" => Box::new(NoopScript::new()),
            "putbombinreplicator" => Box::new(NoopScript::new()),
            "trapunref" => Box::new(NoopScript::new()),

            // shodan
            // TODO: What's the difference between base elevator / dont stop elevator?
            "dontstopelevator" => Box::new(BaseElevator::continuous()),

            // Not implemented
            // TODO: Handle keypad code
            "ammoscript" => Box::new(NoopScript::new()),
            //"BaseElevator" => Box::new(UnimplementedScript::new(&name)),
            "baselight" => Box::new(NoopScript {}),
            //"baseai" => Box::new(PanicOnLoadScript::new(&script_name)),
            "baseai" => Box::new(NoopScript::new()),
            "basemonster" => Box::new(BaseMonster::new()),
            "cameraalert" => Box::new(UnimplementedScript::new(&script_name)),
            "cameradeath" => Box::new(UnimplementedScript::new(&script_name)),
            "censor" => Box::new(UnimplementedScript::new(&script_name)),
            "censorme" => Box::new(UnimplementedScript::new(&script_name)),
            "creaturecontainer" => Box::new(NoopScript {}),
            "chemical" => Box::new(NoopScript::new()),
            "chooseservice" => Box::new(ChooseServiceScript::new()),
            "infocomputer" => Box::new(UnimplementedScript::new(&script_name)),
            "reducepsi" => Box::new(UnimplementedScript::new(&script_name)),
            "replicatorscript" => gui_script(Box::new(ReplicatorGui)),
            "researchablescript" => Box::new(UnimplementedScript::new(&script_name)),
            "setupinitialdebrief" => {
                Box::new(setup_initial_debrief::SetupInitialDebriefScript::new())
            }
            "toxinpatch" => Box::new(UnimplementedScript::new(&script_name)),
            // Need to read ambient hacked property
            "triggerecology" => Box::new(UnimplementedScript::new(&script_name)),
            "triggerecologydiff" => Box::new(UnimplementedScript::new(&script_name)),
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
            .push(script);

        self.entity_has_initialized.insert(entity_id, false);
    }

    pub fn remove_entity(&mut self, entity_id: EntityId) {
        self.entity_to_scripts.remove(&entity_id);
        self.entity_has_initialized.remove(&entity_id);
    }

    pub fn dispatch(&mut self, message: Message) {
        self.message_queue.push(message);
    }

    pub fn update(&mut self, world: &World, physics: &PhysicsWorld, time: &Time) -> Vec<Effect> {
        let mut produced_effects = Vec::new();

        // Initialize any entities that haven't been initialized yet
        for (entity_id, initialized) in self.entity_has_initialized.iter_mut() {
            if !(*initialized) {
                self.entity_to_scripts
                    .entry(*entity_id)
                    .and_modify(|scripts| {
                        for script in scripts {
                            let eff = script.initialize(*entity_id, world);
                            produced_effects.push(eff);
                        }
                    });

                *initialized = true;
            }
        }

        // Process any incoming messages
        let mut slayed_entities: HashSet<EntityId> = HashSet::new();
        let span = span!(Level::INFO, "messages");
        let _ = span.enter();
        for msg in &self.message_queue {
            let to_entity_id = msg.to;

            if matches!(msg.payload, MessagePayload::Slay) {
                slayed_entities.insert(to_entity_id);
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
                    for script in scripts {
                        if is_turn_on {
                            info!(
                                "-- processing turn on message: {}",
                                debug_entity(world, to_entity_id)
                            );
                        }
                        let eff = script.handle_message(to_entity_id, world, physics, &msg.payload);
                        produced_effects.push(eff);
                    }
                });
        }

        for ent in slayed_entities {
            produced_effects.push(Effect::SlayEntity { entity_id: ent })
        }

        self.message_queue.clear();

        for (entity_id, scripts) in self.entity_to_scripts.iter_mut() {
            for script in scripts.iter_mut() {
                let eff = script.update(*entity_id, world, physics, time);
                produced_effects.push(eff);
            }
        }

        let flattened_effects = Effect::flatten(produced_effects);

        // Filter out message effects, add to queue
        // TODO: Is this necessary to filter out and manually queue?
        let mut ret = Vec::new();
        for eff in flattened_effects {
            match eff {
                Effect::Send { msg } => self.message_queue.push(msg),
                _ => ret.push(eff),
            }
        }

        ret
    }
}
