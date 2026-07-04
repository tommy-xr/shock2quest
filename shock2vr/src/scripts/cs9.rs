//! Scripts for CS9, the in-engine "Polito is SHODAN" reveal cutscene staged in
//! ops1.mis. Behavioral spec reconstructed from Tom N Harris' `allobjs.osm`
//! script analysis (thiefmissions.com/telliamed/allscripts.html) and the
//! mission data:
//!
//! - A walk-in tripwire sends TurnOn to `CutSceneNine` (CS9_MasterControl),
//!   which runs the whole show: raises the exit barriers (`MasterForceField`),
//!   pulls the office walls apart (`SlowDoorControl` delay chains driving
//!   StdDoor wall panels), plays SHODAN's monologue schemas cs0901..cs0909 in
//!   sequence, brings in the holographic exhibits (eggs/grubs via
//!   `EggsandGrubsControl`, rumblers at `RumblerLoc1-4`) during the "grove"
//!   section, then tears everything back down and releases the player.
//! - In the original the wall/schema transitions are event-driven (SchemaDone,
//!   CS9_DoorReporter). Our audio layer has no completion events, so the master
//!   script runs a fixed timeline using the measured lengths of the cs090x
//!   audio files (see SCHEMAS below).
use cgmath::Vector3;
use dark::motion::{MotionQueryItem, MotionQuerySelectionStrategy};
use dark::properties::{Link, PropPosition};
use shipyard::{EntityId, Get, View, World};
use tracing::info;

use engine::audio::AudioHandle;

use crate::{physics::PhysicsWorld, time::Time};

use super::{
    Effect, Message, MessagePayload, Script,
    script_util::{
        get_all_links_of_type, get_entities_by_name, get_first_entity_by_name,
        send_to_all_switch_links,
    },
    transluce::AlphaFader,
};

/// The nine SHODAN monologue schemas with their audio lengths in seconds
/// (measured from the shipped `vCs/<language>/cs090x.wav` files; English).
const SCHEMAS: [(&str, f32); 9] = [
    ("cs0901", 28.7),
    ("cs0902", 30.8),
    ("cs0903", 23.8),
    ("cs0904", 15.7),
    ("cs0905", 11.7),
    ("cs0906", 27.4),
    ("cs0907", 14.1),
    ("cs0908", 23.4),
    ("cs0909", 38.4),
];

/// Pause before the first schema (the walls start moving first) and between
/// consecutive schemas.
const INITIAL_DELAY: f32 = 2.0;
const INTER_SCHEMA_GAP: f32 = 0.6;

/// The six named controller screens on the theatre's front wall (each relays
/// to its side-wall copies via SwitchLinks).
const SCREENS: [&str; 6] = [
    "ShodanScreenTL",
    "ShodanScreenTM",
    "ShodanScreenTR",
    "ShodanScreenBL",
    "ShodanScreenBM",
    "ShodanScreenBR",
];

#[derive(Clone, Copy, Debug)]
enum Cs9Action {
    /// Send TurnOn to every entity with this sym name.
    TurnOnByName(&'static str),
    /// Send TurnOff to every entity with this sym name.
    TurnOffByName(&'static str),
    /// Start a monologue schema.
    PlaySchema(&'static str),
    /// Fade the theatre screens in, if the MovingWallsOpen report from
    /// CS9_DoorReporter hasn't already done so.
    ScreensOnFallback,
}

/// Master sequencer for the reveal (script `CS9_MasterControl`, on the
/// `CutSceneNine` marker). Fires a one-shot timeline of actions once tripped.
pub struct CS9MasterControl {
    started: bool,
    elapsed: f32,
    /// (fire time, action), sorted by time; `next` indexes the first unfired.
    schedule: Vec<(f32, Cs9Action)>,
    next: usize,
    /// Set when the theatre screens have been faded in (by the DoorReporter's
    /// MovingWallsOpen event, or the timed fallback).
    screens_faded_in: bool,
}

impl CS9MasterControl {
    pub fn new() -> CS9MasterControl {
        let mut schedule: Vec<(f32, Cs9Action)> = vec![
            // Seal the player in and start pulling the theatre apart.
            (0.0, Cs9Action::TurnOnByName("MasterForceField")),
            (0.0, Cs9Action::TurnOnByName("SlowDoorControl")),
        ];

        let mut t = INITIAL_DELAY;
        for (i, (schema, duration)) in SCHEMAS.iter().enumerate() {
            schedule.push((t, Cs9Action::PlaySchema(schema)));
            match i {
                // cs0901: SHODAN appears. The screens fade in when the
                // reporter panel signals MovingWallsOpen (dark screens are
                // revealed, then the face materializes during the opening
                // line); this scheduled entry is a fallback in case the
                // report never arrives.
                0 => {
                    schedule.push((t + 16.0, Cs9Action::ScreensOnFallback));
                }
                // cs0903: the "garden grove" section - the exhibits appear.
                2 => {
                    schedule.push((t, Cs9Action::TurnOnByName("EggsandGrubsControl")));
                    for r in ["Rumbler1", "Rumbler2", "Rumbler3", "Rumbler4"] {
                        schedule.push((t, Cs9Action::TurnOnByName(r)));
                    }
                }
                // cs0907: "and now they seek to destroy me" - exhibits leave.
                6 => {
                    for r in ["Rumbler1", "Rumbler2", "Rumbler3", "Rumbler4"] {
                        schedule.push((t, Cs9Action::TurnOffByName(r)));
                    }
                }
                _ => {}
            }
            t += duration + INTER_SCHEMA_GAP;
        }

        // Teardown: fade the screens, stash the exhibits, close the walls,
        // release the player.
        for s in SCREENS {
            schedule.push((t, Cs9Action::TurnOffByName(s)));
        }
        schedule.push((t, Cs9Action::TurnOffByName("EggsandGrubsControl")));
        schedule.push((t, Cs9Action::TurnOffByName("SlowDoorControl")));
        schedule.push((t + 2.0, Cs9Action::TurnOffByName("MasterForceField")));

        schedule.sort_by(|a, b| a.0.total_cmp(&b.0));

        CS9MasterControl {
            started: false,
            elapsed: 0.0,
            schedule,
            next: 0,
            screens_faded_in: false,
        }
    }

    /// Fade the theatre screens in, once.
    fn fire_screens_on(&mut self, entity_id: EntityId, world: &World) -> Effect {
        if self.screens_faded_in {
            return Effect::NoEffect;
        }
        self.screens_faded_in = true;
        info!("cs9: fading theatre screens in");
        let effects = SCREENS
            .iter()
            .map(|name| send_by_name(entity_id, world, name, /* on */ true))
            .collect();
        Effect::Combined { effects }
    }

    fn run_action(&mut self, entity_id: EntityId, world: &World, action: Cs9Action) -> Effect {
        info!("cs9: firing action {:?}", action);
        match action {
            Cs9Action::TurnOnByName(name) => send_by_name(entity_id, world, name, true),
            Cs9Action::TurnOffByName(name) => send_by_name(entity_id, world, name, false),
            Cs9Action::PlaySchema(schema) => Effect::PlaySound {
                handle: AudioHandle::new(),
                name: schema.to_string(),
            },
            Cs9Action::ScreensOnFallback => self.fire_screens_on(entity_id, world),
        }
    }
}

/// Send TurnOn/TurnOff to every entity with the given sym name.
fn send_by_name(from: EntityId, world: &World, name: &str, on: bool) -> Effect {
    let targets = get_entities_by_name(world, name);
    if targets.is_empty() {
        info!("cs9: no entities named '{}'", name);
    }
    let effects = targets
        .into_iter()
        .map(|to| {
            let payload = if on {
                MessagePayload::TurnOn { from }
            } else {
                MessagePayload::TurnOff { from }
            };
            Effect::Send {
                msg: Message { to, payload },
            }
        })
        .collect();
    Effect::Combined { effects }
}

impl Script for CS9MasterControl {
    fn handle_message(
        &mut self,
        entity_id: EntityId,
        world: &World,
        _physics: &PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        match msg {
            MessagePayload::TurnOn { from: _ } => {
                if !self.started {
                    info!("cs9: master control tripped - starting cutscene");
                    self.started = true;
                }
                Effect::NoEffect
            }
            // CS9_DoorReporter tells us the moving walls finished opening -
            // reveal SHODAN on the now-exposed screens.
            MessagePayload::Signal { name } if name == "MovingWallsOpen" && self.started => {
                info!("cs9: MovingWallsOpen reported");
                self.fire_screens_on(entity_id, world)
            }
            _ => Effect::NoEffect,
        }
    }

    fn update(
        &mut self,
        entity_id: EntityId,
        world: &World,
        _physics: &PhysicsWorld,
        time: &Time,
    ) -> Effect {
        if !self.started || self.next >= self.schedule.len() {
            return Effect::NoEffect;
        }

        self.elapsed += time.elapsed.as_secs_f32();

        let mut effects = Vec::new();
        while self.next < self.schedule.len() && self.schedule[self.next].0 <= self.elapsed {
            let (_, action) = self.schedule[self.next];
            effects.push(self.run_action(entity_id, world, action));
            self.next += 1;
        }

        if effects.is_empty() {
            Effect::NoEffect
        } else {
            Effect::Combined { effects }
        }
    }
}

/// Script `CS9_ShodanScreen`: a theatre screen piece. Starts invisible, fades
/// in/out on TurnOn/TurnOff like a hologram, and - unlike the plain Transluce
/// script - FORWARDS the message to its SwitchLinked screen copies: the master
/// addresses only the six named controller screens on the front wall, and each
/// controller relays to its counterparts on the side walls (the mission wires
/// this relay as SwitchLinks; the original script uses them the same way).
pub struct CS9ShodanScreen {
    fader: AlphaFader,
    visible_alpha: f32,
}

impl CS9ShodanScreen {
    pub fn new() -> CS9ShodanScreen {
        CS9ShodanScreen {
            fader: AlphaFader::new(0.0),
            visible_alpha: 1.0,
        }
    }
}

impl Script for CS9ShodanScreen {
    fn initialize(&mut self, entity_id: EntityId, world: &World) -> Effect {
        let v_alpha = world
            .borrow::<View<dark::properties::PropRenderAlpha>>()
            .unwrap();
        self.visible_alpha = v_alpha.get(entity_id).map(|a| a.0).unwrap_or(1.0);
        self.fader.snap_to(0.0);
        Effect::SetRenderAlpha {
            entity_id,
            alpha: 0.0,
        }
    }

    fn handle_message(
        &mut self,
        entity_id: EntityId,
        world: &World,
        _physics: &PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        match msg {
            MessagePayload::TurnOn { from: _ } => self.fader.fade_to(self.visible_alpha),
            MessagePayload::TurnOff { from: _ } => self.fader.fade_to(0.0),
            _ => return Effect::NoEffect,
        }
        // Relay to the SwitchLinked screen copies. Loops are impossible in the
        // mission data (controllers link one-way to copies), and a re-received
        // message would only re-set an identical fade target anyway.
        send_to_all_switch_links(world, entity_id, msg.clone())
    }

    fn update(
        &mut self,
        entity_id: EntityId,
        _world: &World,
        _physics: &PhysicsWorld,
        time: &Time,
    ) -> Effect {
        self.fader.update(entity_id, time)
    }
}

/// Position + rotation of a named marker, if it exists.
fn marker_pose(world: &World, name: &str) -> Option<(Vector3<f32>, cgmath::Quaternion<f32>)> {
    let marker = get_first_entity_by_name(world, name)?;
    let v_pos = world.borrow::<View<PropPosition>>().unwrap();
    v_pos.get(marker).ok().map(|p| (p.position, p.rotation))
}

/// Teleport an entity to a named marker; used by the CS9 exhibit scripts.
/// Returns NoEffect when the marker is missing. (The holo look itself comes
/// from `TransluceInOutHolo`, composed on the same entities, which fades to
/// the authored alpha on the same TurnOn.)
fn teleport_to_marker(world: &World, entity_id: EntityId, marker: &str) -> Effect {
    match marker_pose(world, marker) {
        Some((position, rotation)) => Effect::SetPositionRotation {
            entity_id,
            position,
            rotation,
        },
        None => {
            info!("cs9: marker '{}' not found", marker);
            Effect::NoEffect
        }
    }
}

/// Script `CS9_HoloRumbler`: the four rumblers are parked out of sight; on
/// TurnOn each appears at its matching `RumblerLoc<N>` marker (fading in via
/// the composed `TransluceInOutHolo`) and walks in place, like the original's
/// `WalkInPlace` motion; on TurnOff it stops and is stashed at
/// `SafeTeleportLoc`.
pub struct CS9HoloRumbler {
    walking: bool,
}

impl CS9HoloRumbler {
    pub fn new() -> CS9HoloRumbler {
        CS9HoloRumbler { walking: false }
    }

    /// "Rumbler3" -> "RumblerLoc3", matched via this entity's own sym name.
    fn loc_marker_name(world: &World, entity_id: EntityId) -> Option<String> {
        let v_name = world
            .borrow::<View<dark::properties::PropSymName>>()
            .unwrap();
        let name = v_name.get(entity_id).ok()?.0.clone();
        let digit = name.chars().rev().find(|c| c.is_ascii_digit())?;
        Some(format!("RumblerLoc{digit}"))
    }

    /// The hologram's walk cycle: the rumblers are authored with
    /// `PropCreaturePose { TAG, "cs 131" }` - the cutscene's walk-in-place
    /// motion (resolves to `rmbplace`). The entity has no AI script, so this
    /// script owns the animation and re-queues it on completion to loop.
    fn walk_animation(world: &World, entity_id: EntityId) -> Effect {
        let v_pose = world
            .borrow::<View<dark::properties::PropCreaturePose>>()
            .unwrap();
        let Ok(pose) = v_pose.get(entity_id) else {
            return Effect::NoEffect;
        };

        // "cs 131" -> tag "cs" with value 131; a plain tag has no value.
        let mut parts = pose.motion_or_tag_name.split_whitespace();
        let item = match (parts.next(), parts.next().and_then(|v| v.parse().ok())) {
            (Some(tag), Some(value)) => MotionQueryItem::with_value(tag, value),
            (Some(tag), None) => MotionQueryItem::new(tag),
            (None, _) => return Effect::NoEffect,
        };

        Effect::QueueAnimationBySchema {
            entity_id,
            selection_strategy: MotionQuerySelectionStrategy::Random,
            motion_query_items: vec![item],
        }
    }
}

impl Script for CS9HoloRumbler {
    fn handle_message(
        &mut self,
        entity_id: EntityId,
        world: &World,
        _physics: &PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        match msg {
            MessagePayload::TurnOn { from: _ } => match Self::loc_marker_name(world, entity_id) {
                Some(marker) => {
                    self.walking = true;
                    Effect::Combined {
                        effects: vec![
                            teleport_to_marker(world, entity_id, &marker),
                            Self::walk_animation(world, entity_id),
                        ],
                    }
                }
                None => Effect::NoEffect,
            },
            MessagePayload::TurnOff { from: _ } => {
                self.walking = false;
                teleport_to_marker(world, entity_id, "SafeTeleportLoc")
            }
            MessagePayload::AnimationCompleted if self.walking => {
                Self::walk_animation(world, entity_id)
            }
            _ => Effect::NoEffect,
        }
    }
}

/// Script `CS9_EggsandGrubs` (on the `EggsandGrubsControl` marker): on TurnOn,
/// walks the SwitchLink chain starting at `FirstEgg`, teleporting one object
/// per tick to its own `Teleport`-linked display marker (the eggs rise into
/// the exhibit one at a time). On TurnOff everything is stashed back at
/// `SafeTeleportLoc`.
pub struct CS9EggsAndGrubs {
    /// Remaining chain to reveal, one per NEXT_ITERATION_SECONDS.
    pending: Vec<EntityId>,
    revealed: Vec<EntityId>,
    tick_timer: f32,
    active: bool,
}

const NEXT_ITERATION_SECONDS: f32 = 1.2;

impl CS9EggsAndGrubs {
    pub fn new() -> CS9EggsAndGrubs {
        CS9EggsAndGrubs {
            pending: Vec::new(),
            revealed: Vec::new(),
            tick_timer: 0.0,
            active: false,
        }
    }

    /// FirstEgg -> SwitchLink -> next -> ... collects the whole exhibit chain.
    fn collect_chain(world: &World, first: EntityId) -> Vec<EntityId> {
        let mut chain = vec![first];
        let mut current = first;
        loop {
            let next = get_all_links_of_type(world, current, Link::SwitchLink);
            match next.into_iter().find(|e| !chain.contains(e)) {
                Some(e) => {
                    chain.push(e);
                    current = e;
                }
                None => break,
            }
        }
        chain
    }

    /// An exhibit object's display pose is its own `Teleport` link target.
    fn display_pose(
        world: &World,
        entity_id: EntityId,
    ) -> Option<(Vector3<f32>, cgmath::Quaternion<f32>)> {
        let marker = get_all_links_of_type(world, entity_id, Link::Teleport)
            .into_iter()
            .next()?;
        let v_pos = world.borrow::<View<PropPosition>>().unwrap();
        v_pos.get(marker).ok().map(|p| (p.position, p.rotation))
    }
}

impl Script for CS9EggsAndGrubs {
    fn handle_message(
        &mut self,
        _entity_id: EntityId,
        world: &World,
        _physics: &PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        match msg {
            MessagePayload::TurnOn { from: _ } => {
                if let Some(first) = get_first_entity_by_name(world, "FirstEgg") {
                    self.pending = Self::collect_chain(world, first);
                    info!("cs9: eggs/grubs chain of {} objects", self.pending.len());
                    self.active = true;
                    self.tick_timer = 0.0;
                }
                Effect::NoEffect
            }
            MessagePayload::TurnOff { from: _ } => {
                self.active = false;
                let stash: Vec<Effect> = self
                    .revealed
                    .drain(..)
                    .chain(self.pending.drain(..))
                    .map(|e| teleport_to_marker(world, e, "SafeTeleportLoc"))
                    .collect();
                Effect::Combined { effects: stash }
            }
            _ => Effect::NoEffect,
        }
    }

    fn update(
        &mut self,
        _entity_id: EntityId,
        world: &World,
        _physics: &PhysicsWorld,
        time: &Time,
    ) -> Effect {
        if !self.active || self.pending.is_empty() {
            return Effect::NoEffect;
        }

        self.tick_timer -= time.elapsed.as_secs_f32();
        if self.tick_timer > 0.0 {
            return Effect::NoEffect;
        }
        self.tick_timer = NEXT_ITERATION_SECONDS;

        let egg = self.pending.remove(0);
        self.revealed.push(egg);
        // TurnOn fades the exhibit in (TransluceInOutHolo) and opens the egg
        // (GrubEgg tweq), both composed on the egg entity itself.
        let turn_on = Effect::Send {
            msg: Message {
                to: egg,
                payload: MessagePayload::TurnOn { from: egg },
            },
        };
        match Self::display_pose(world, egg) {
            Some((position, rotation)) => Effect::Combined {
                effects: vec![
                    Effect::SetPositionRotation {
                        entity_id: egg,
                        position,
                        rotation,
                    },
                    turn_on,
                ],
            },
            // No Teleport link: leave the object where it is (it may already
            // sit at its display spot and only need the fade-in).
            None => turn_on,
        }
    }
}

/// Script `CS9_DoorReporter` (on one designated moving-wall panel): forwards
/// the door's open/close completion to the cutscene master as
/// MovingWallsOpen/MovingWallsClose, so the show reacts to the walls actually
/// finishing instead of a guessed time.
pub struct CS9DoorReporter {}

impl CS9DoorReporter {
    pub fn new() -> CS9DoorReporter {
        CS9DoorReporter {}
    }
}

impl Script for CS9DoorReporter {
    fn handle_message(
        &mut self,
        _entity_id: EntityId,
        world: &World,
        _physics: &PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        let MessagePayload::Signal { name } = msg else {
            return Effect::NoEffect;
        };
        let report = match name.as_str() {
            "DoorOpen" => "MovingWallsOpen",
            "DoorClose" => "MovingWallsClose",
            _ => return Effect::NoEffect,
        };
        match get_first_entity_by_name(world, "CutSceneNine") {
            Some(master) => Effect::Send {
                msg: Message {
                    to: master,
                    payload: MessagePayload::Signal {
                        name: report.to_string(),
                    },
                },
            },
            None => {
                info!("cs9: door reporter found no CutSceneNine");
                Effect::NoEffect
            }
        }
    }
}

/// Script `TrapDestroyTeleport`: destroys whatever its `Teleport` links point
/// at when tripped/turned on. (In ops1 the marker carrying it has no Teleport
/// links, so it is inert there - implemented for data-faithfulness.)
pub struct TrapDestroyTeleport {}

impl TrapDestroyTeleport {
    pub fn new() -> TrapDestroyTeleport {
        TrapDestroyTeleport {}
    }
}

impl Script for TrapDestroyTeleport {
    fn handle_message(
        &mut self,
        entity_id: EntityId,
        world: &World,
        _physics: &PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        match msg {
            MessagePayload::TurnOn { from: _ } | MessagePayload::SensorBeginIntersect { .. } => {
                let effects = get_all_links_of_type(world, entity_id, Link::Teleport)
                    .into_iter()
                    .map(|target| Effect::DestroyEntity { entity_id: target })
                    .collect();
                Effect::Combined { effects }
            }
            _ => Effect::NoEffect,
        }
    }
}

/// Script `SitDownRightNowMP`: in the original this freezes players into the
/// Seat1-4 markers *in multiplayer only* - single-player relies on the
/// barriers alone (players can move freely inside them during the show). We
/// only support single player, so this is deliberately inert.
pub struct SitDownRightNowMP {}

impl SitDownRightNowMP {
    pub fn new() -> SitDownRightNowMP {
        SitDownRightNowMP {}
    }
}

impl Script for SitDownRightNowMP {
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
