use std::collections::HashSet;

use dark::properties::{
    PropLocalPlayer, PropTeleported, PropTranslatingDoor, PropTripFlags, TeleportSource, TripFlags,
};
use shipyard::{EntityId, Get, View, World};
use tracing::info;

use crate::physics::PhysicsWorld;

use super::{
    Effect, MessagePayload, Script,
    script_util::{get_all_switch_links, invert, send_to_all_switch_links},
};

pub fn is_player(world: &World, entity_id: EntityId) -> bool {
    let v_prop_player = world.borrow::<View<PropLocalPlayer>>().unwrap();

    v_prop_player.get(entity_id).is_ok()
}

fn teleport_source(world: &World, entity_id: EntityId) -> Option<TeleportSource> {
    let v_teleported = world.borrow::<View<PropTeleported>>().unwrap();
    v_teleported.get(entity_id).ok().map(|marker| marker.source)
}

pub struct TrapNewTripwire {
    has_activated: bool,
    entity_in_trap: HashSet<EntityId>,
    reconstructed_after_load: HashSet<EntityId>,
    trip_flags: TripFlags,
}
impl TrapNewTripwire {
    pub fn new() -> TrapNewTripwire {
        TrapNewTripwire {
            trip_flags: TripFlags::DEFAULT,
            has_activated: false,
            entity_in_trap: HashSet::new(),
            reconstructed_after_load: HashSet::new(),
        }
    }

    fn handle_invert(&self, msg: MessagePayload) -> MessagePayload {
        if self.trip_flags.contains(TripFlags::INVERT) {
            invert(msg)
        } else {
            msg
        }
    }

    fn should_activate(
        &mut self,
        world: &World,
        self_entity_id: EntityId,
        tripping_entity_id: EntityId,
        trip_flags: &TripFlags,
    ) -> bool {
        // TODO
        // There are still several other flags that need to be implemented, like:
        // Shove
        // Zap
        // EasterEgg

        let is_once = trip_flags.contains(TripFlags::ONCE);

        if is_once && self.has_activated {
            false
        } else if trip_flags.contains(TripFlags::PLAYER) {
            // TODO: TripFlags::Player
            // I'm not sure what the TripFlags::Player is actually used for.
            // It seems like - in the game - AI can trigger tripwires that are marked as Player
            // So I'll ignore this condition for now.
            let allow_ai_to_trigger = Self::is_linked_to_simple_door(world, self_entity_id);

            is_player(world, tripping_entity_id) || (allow_ai_to_trigger && !is_once)
        } else {
            true
        }
    }

    fn is_linked_to_simple_door(world: &World, entity_id: EntityId) -> bool {
        let links = get_all_switch_links(world, entity_id);

        let v_simple_door = world.borrow::<View<PropTranslatingDoor>>().unwrap();

        // Are there any links that are a simple door?
        // TODO: Make sure it is _simple_ - ie, not locked
        // TODO: Handle rotating doors?
        links.iter().any(|link| v_simple_door.get(*link).is_ok())
    }
}
impl Script for TrapNewTripwire {
    fn initialize(&mut self, entity_id: EntityId, world: &World) -> Effect {
        let v_trip_flags = world.borrow::<View<PropTripFlags>>().unwrap();
        let default_flags = PropTripFlags::default();
        let trip_flags = v_trip_flags.get(entity_id).unwrap_or(&default_flags);

        self.trip_flags = trip_flags.trip_flags;
        info!("initializing: trip_flags: {:?}", trip_flags);

        Effect::NoEffect
    }
    fn handle_message(
        &mut self,
        entity_id: EntityId,
        world: &World,
        _physics: &PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        let v_trip_flags = world.borrow::<View<PropTripFlags>>().unwrap();
        let default_flags = PropTripFlags::default();
        let trip_flags = v_trip_flags.get(entity_id).unwrap_or(&default_flags);

        // NOTE: intersections count no matter how the entity got here - walking,
        // VR teleport locomotion, or a debug teleport. The Dark engine fires
        // PhysEnter/PhysExit for teleports too, and suppressing them made
        // teleported-in players silently skip triggers (e.g. the ops1 cutscene
        // tripwire).
        match msg {
            MessagePayload::SensorBeginIntersect { with } => {
                match teleport_source(world, *with) {
                    // A scripted teleport trap's arrival must not touch this
                    // tripwire at all - no ENTER signal and not tracked as
                    // present. Tracking it would make walking back out emit an
                    // unbalanced EXIT TurnOff and re-trigger the earth montage
                    // loop (#515).
                    //
                    // NOTE: this clear is an Effect, applied only after the
                    // whole frame's message queue has run - the SOURCE
                    // tripwire's same-frame SensorEndIntersect (below) must
                    // still see the marker, so the clear must never move
                    // before message processing.
                    Some(TeleportSource::ScriptedTrap) => {
                        return Effect::ClearTeleportedMarker { entity_id: *with };
                    }
                    // Loading creates a new physics world, so Rapier reports
                    // every sensor containing the restored player as a fresh
                    // BeginIntersect. Reconstruct that existing overlap in the
                    // transient set without replaying ENTER. Unlike a scripted
                    // teleport, track presence so leaving establishes the clean
                    // edge needed for a later genuine re-entry (#547).
                    Some(TeleportSource::LoadRestore) => {
                        if self.should_activate(world, entity_id, *with, &trip_flags.trip_flags) {
                            self.entity_in_trap.insert(*with);
                            self.reconstructed_after_load.insert(*with);
                        }
                        return Effect::NoEffect;
                    }
                    Some(TeleportSource::Locomotion) | None => {}
                }

                if self.should_activate(world, entity_id, *with, &trip_flags.trip_flags) {
                    info!("activating tripwire");
                    self.has_activated = true;
                    let was_empty = self.entity_in_trap.is_empty();

                    self.entity_in_trap.insert(*with);

                    if was_empty && self.trip_flags.contains(TripFlags::ENTER) {
                        send_to_all_switch_links(
                            world,
                            entity_id,
                            self.handle_invert(MessagePayload::TurnOn { from: entity_id }),
                        )
                    } else {
                        Effect::NoEffect
                    }
                } else {
                    Effect::NoEffect
                }
            }
            MessagePayload::SensorEndIntersect { with } => {
                let had_keys_before = !self.entity_in_trap.is_empty();
                let was_reconstructed_after_load = self.reconstructed_after_load.remove(with);
                // A scripted teleport trap yanked the entity out of this box -
                // that departure is not a gameplay EXIT edge, the mirror of the
                // arrival suppression above. On earth.mis the training wires
                // switch a teleport trap *and* an inverter fanning out to a
                // dozen Sound Traps: emitting EXIT's TurnOff here inverts into
                // a TurnOn broadcast that starts every narration at once (and
                // re-fires the teleport). Presence is still cleared so a later
                // genuine re-entry sees a clean edge - which deliberately
                // leaves ENTER's TurnOn without a balancing TurnOff, the same
                // trade the arrival suppression already makes.
                let departed_by_scripted_teleport =
                    teleport_source(world, *with) == Some(TeleportSource::ScriptedTrap);

                self.entity_in_trap.remove(with);

                let has_keys_now = !self.entity_in_trap.is_empty();

                info!(
                    "sensor end intersect for {:?} - has_keys_now: {} had_keys_before: {} trip_flags: {:?}",
                    with, has_keys_now, had_keys_before, trip_flags
                );

                // The first EndIntersect paired with a load-reconstructed
                // overlap is not a gameplay EXIT edge. Emitting TurnOff here
                // would be as unbalanced as replaying ENTER and, on Earth,
                // would activate the same teleport wiring while merely
                // leaving the lobby sensor. Once removed, a later genuine
                // enter/exit pair behaves normally.
                if !was_reconstructed_after_load
                    && !departed_by_scripted_teleport
                    && !has_keys_now
                    && had_keys_before
                    && self.trip_flags.contains(TripFlags::EXIT)
                {
                    send_to_all_switch_links(
                        world,
                        entity_id,
                        self.handle_invert(MessagePayload::TurnOff { from: entity_id }),
                    )
                } else {
                    Effect::NoEffect
                }
            }
            _ => Effect::NoEffect,
        }
    }
}
