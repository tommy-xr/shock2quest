use std::collections::HashSet;

use dark::properties::{PropLocalPlayer, PropTeleported, PropTripFlags, TripFlags};
use shipyard::{EntityId, Get, View, World};
use tracing::info;

use crate::physics::PhysicsWorld;

use super::{
    script_util::{invert, send_to_all_switch_links},
    Effect, MessagePayload, Script,
};

pub fn is_player(world: &World, entity_id: EntityId) -> bool {
    let v_prop_player = world.borrow::<View<PropLocalPlayer>>().unwrap();

    v_prop_player.get(entity_id).is_ok()
}

fn did_entity_just_teleport(world: &World, entity_id: EntityId) -> bool {
    let teleported = world.borrow::<View<PropTeleported>>().unwrap();
    teleported.contains(entity_id)
}

pub struct TrapNewTripwire {
    has_activated: bool,
    entity_in_trap: HashSet<EntityId>,
    teleported_entities_to_ignore: HashSet<EntityId>,
    trip_flags: TripFlags,
}
impl TrapNewTripwire {
    pub fn new() -> TrapNewTripwire {
        TrapNewTripwire {
            trip_flags: TripFlags::Default,
            has_activated: false,
            entity_in_trap: HashSet::new(),
            //entity_to_time: HashMap::new(),
            teleported_entities_to_ignore: HashSet::new(),
        }
    }

    fn handle_invert(&self, msg: MessagePayload) -> MessagePayload {
        if self.trip_flags.contains(TripFlags::Invert) {
            invert(msg)
        } else {
            msg
        }
    }

    fn should_activate(
        &mut self,
        _world: &World,
        _tripped_entity_id: EntityId,
        trip_flags: &TripFlags,
    ) -> bool {
        if trip_flags.contains(TripFlags::Once) && self.has_activated {
            false
        // } else if trip_flags.contains(TripFlags::Player) {
        //     is_player(world, tripped_entity_id)
        } else {
            true
        }
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

        match msg {
            MessagePayload::SensorBeginIntersect { with } => {
                if self.should_activate(world, *with, &trip_flags.trip_flags) {
                    info!("activating tripwire");
                    self.has_activated = true;
                    let was_empty = self.entity_in_trap.is_empty();

                    let did_entity_just_teleport = did_entity_just_teleport(world, *with);
                    self.entity_in_trap.insert(*with);

                    if did_entity_just_teleport {
                        self.teleported_entities_to_ignore.insert(*with);
                    }

                    if was_empty
                        && self.trip_flags.contains(TripFlags::Enter)
                        && !did_entity_just_teleport
                    {
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

                // If the entity teleported, we should disregard
                let did_teleport = did_entity_just_teleport(world, *with)
                    || self.teleported_entities_to_ignore.contains(with);

                self.teleported_entities_to_ignore.remove(with);
                self.entity_in_trap.remove(with);

                let has_keys_now = !self.entity_in_trap.is_empty();

                info!("sensor end intersect for {:?} - did_teleport: {} has_keys_now: {} had_keys_before: {} trip_flags: {:?}",
                with,
                did_teleport,
                has_keys_now,
                had_keys_before,
                trip_flags
            );

                if !did_teleport
                    && !has_keys_now
                    && had_keys_before
                    && self.trip_flags.contains(TripFlags::Exit)
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
