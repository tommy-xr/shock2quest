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

/// Did this entity arrive via a *scripted* teleport trap (as opposed to walking
/// or VR teleport locomotion)? Scripted-trap arrivals must not fire tripwire
/// ENTER, so a return teleport that lands inside another trap's box doesn't
/// re-fire it (#515). Locomotion teleports still fire (e.g. ops1 cutscene).
fn arrived_via_scripted_teleport(world: &World, entity_id: EntityId) -> bool {
    let v_teleported = world.borrow::<View<PropTeleported>>().unwrap();
    matches!(
        v_teleported.get(entity_id),
        Ok(t) if t.source == TeleportSource::ScriptedTrap
    )
}

pub struct TrapNewTripwire {
    has_activated: bool,
    entity_in_trap: HashSet<EntityId>,
    trip_flags: TripFlags,
}
impl TrapNewTripwire {
    pub fn new() -> TrapNewTripwire {
        TrapNewTripwire {
            trip_flags: TripFlags::DEFAULT,
            has_activated: false,
            entity_in_trap: HashSet::new(),
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
                // The ONE exception: a *scripted* teleport trap. Its arrival must
                // not touch this tripwire at all - no ENTER signal AND not tracked
                // as present. If we tracked it, walking back out would fire an
                // unbalanced EXIT TurnOff and re-trigger the trap (the earth.mis
                // montage loop, #515). This matches the original engine ignoring
                // teleported-in entities. Consume the marker so a later walk-in to
                // a *different* nearby tripwire still fires normally.
                if arrived_via_scripted_teleport(world, *with) {
                    return Effect::ClearTeleportedMarker { entity_id: *with };
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

                self.entity_in_trap.remove(with);

                let has_keys_now = !self.entity_in_trap.is_empty();

                info!(
                    "sensor end intersect for {:?} - has_keys_now: {} had_keys_before: {} trip_flags: {:?}",
                    with, has_keys_now, had_keys_before, trip_flags
                );

                if !has_keys_now && had_keys_before && self.trip_flags.contains(TripFlags::EXIT) {
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
