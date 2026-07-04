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
use dark::properties::{Link, PropPosition};
use shipyard::{EntityId, Get, View, World};
use tracing::info;

use engine::audio::AudioHandle;

use crate::{physics::PhysicsWorld, time::Time};

use super::{
    Effect, Message, MessagePayload, Script,
    script_util::{get_all_links_of_type, get_entities_by_name, get_first_entity_by_name},
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

#[derive(Clone, Copy, Debug)]
enum Cs9Action {
    /// Send TurnOn to every entity with this sym name.
    TurnOnByName(&'static str),
    /// Send TurnOff to every entity with this sym name.
    TurnOffByName(&'static str),
    /// Start a monologue schema.
    PlaySchema(&'static str),
}

/// Master sequencer for the reveal (script `CS9_MasterControl`, on the
/// `CutSceneNine` marker). Fires a one-shot timeline of actions once tripped.
pub struct CS9MasterControl {
    started: bool,
    elapsed: f32,
    /// (fire time, action), sorted by time; `next` indexes the first unfired.
    schedule: Vec<(f32, Cs9Action)>,
    next: usize,
}

impl CS9MasterControl {
    pub fn new() -> CS9MasterControl {
        const SCREENS: [&str; 6] = [
            "ShodanScreenTL",
            "ShodanScreenTM",
            "ShodanScreenTR",
            "ShodanScreenBL",
            "ShodanScreenBM",
            "ShodanScreenBR",
        ];

        let mut schedule: Vec<(f32, Cs9Action)> = vec![
            // Seal the player in and start pulling the theatre apart.
            (0.0, Cs9Action::TurnOnByName("MasterForceField")),
            (0.0, Cs9Action::TurnOnByName("SlowDoorControl")),
        ];

        let mut t = INITIAL_DELAY;
        for (i, (schema, duration)) in SCHEMAS.iter().enumerate() {
            schedule.push((t, Cs9Action::PlaySchema(schema)));
            match i {
                // cs0901: SHODAN appears. The screens fade in once the moving
                // walls have pulled back (~6s into the panel stagger), so the
                // reveal shows dark screens and the face materializes during
                // the opening line. (PR-later: gate on CS9_DoorReporter's
                // MovingWallsOpen instead of a timed offset.)
                0 => {
                    for s in SCREENS {
                        schedule.push((t + 6.0, Cs9Action::TurnOnByName(s)));
                    }
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
        }
    }

    fn run_action(&self, entity_id: EntityId, world: &World, action: Cs9Action) -> Effect {
        info!("cs9: firing action {:?}", action);
        match action {
            Cs9Action::TurnOnByName(name) | Cs9Action::TurnOffByName(name) => {
                let targets = get_entities_by_name(world, name);
                if targets.is_empty() {
                    info!("cs9: no entities named '{}'", name);
                }
                let effects = targets
                    .into_iter()
                    .map(|to| {
                        let payload = match action {
                            Cs9Action::TurnOnByName(_) => {
                                MessagePayload::TurnOn { from: entity_id }
                            }
                            _ => MessagePayload::TurnOff { from: entity_id },
                        };
                        Effect::Send {
                            msg: Message { to, payload },
                        }
                    })
                    .collect();
                Effect::Combined { effects }
            }
            Cs9Action::PlaySchema(schema) => Effect::PlaySound {
                handle: AudioHandle::new(),
                name: schema.to_string(),
            },
        }
    }
}

impl Script for CS9MasterControl {
    fn handle_message(
        &mut self,
        _entity_id: EntityId,
        _world: &World,
        _physics: &PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        if let MessagePayload::TurnOn { from: _ } = msg {
            if !self.started {
                info!("cs9: master control tripped - starting cutscene");
                self.started = true;
            }
        }
        Effect::NoEffect
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
/// the composed `TransluceInOutHolo`), and on TurnOff is stashed at
/// `SafeTeleportLoc`. (The original also plays a walk-in-place motion; we keep
/// them idle.)
pub struct CS9HoloRumbler {}

impl CS9HoloRumbler {
    pub fn new() -> CS9HoloRumbler {
        CS9HoloRumbler {}
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
                Some(marker) => teleport_to_marker(world, entity_id, &marker),
                None => Effect::NoEffect,
            },
            MessagePayload::TurnOff { from: _ } => {
                teleport_to_marker(world, entity_id, "SafeTeleportLoc")
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
