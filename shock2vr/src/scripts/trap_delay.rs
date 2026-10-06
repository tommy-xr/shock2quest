use dark::properties::PropDelayTime;
use serde::{Deserialize, Serialize};
use shipyard::{EntityId, Get, View, World};
use tracing::info;

use crate::{physics::PhysicsWorld, scripts::script_util::template_id_string, time::Time};

use super::{
    Effect, MessagePayload, Script, ScriptRestoreContext, ScriptState, ScriptStateError,
    script_util::send_to_all_switch_links,
};

const STATE_KEY: &str = "shock2vr.trap_delay";

#[derive(Clone, Serialize, Deserialize)]
struct PendingSwitch {
    turn_on: bool,
    // None denotes a deleted sender, never an old-world handle to be reused.
    sender: Option<u64>,
    remaining_seconds: f32,
}

pub struct TrapDelay {
    delay_time_in_seconds: f32,
    messages: Vec<PendingSwitch>,
}
impl TrapDelay {
    pub fn new() -> TrapDelay {
        TrapDelay {
            delay_time_in_seconds: 1.0,
            messages: Vec::new(),
        }
    }
}
impl Script for TrapDelay {
    fn initialize(&mut self, entity_id: EntityId, world: &World) -> Effect {
        let v_delay_time = world.borrow::<View<PropDelayTime>>().unwrap();
        let delay_time = if let Ok(v) = v_delay_time.get(entity_id) {
            v.delay.as_secs_f32()
        } else {
            1.0
        };
        self.delay_time_in_seconds = delay_time;
        Effect::NoEffect
    }

    fn handle_message(
        &mut self,
        entity_id: EntityId,
        world: &World,
        _physics: &PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        // Retail TrapDelay relays switch messages only. Transient collision,
        // hover, and animation messages are not delayed script events.
        let (turn_on, from) = match msg {
            MessagePayload::TurnOn { from } => (true, *from),
            MessagePayload::TurnOff { from } => (false, *from),
            _ => return Effect::NoEffect,
        };
        info!(
            "{}: receiving message to be delayed: {:?}",
            template_id_string(world, &entity_id),
            msg
        );
        self.messages.push(PendingSwitch {
            turn_on,
            sender: (from != EntityId::dead()).then_some(from.inner()),
            remaining_seconds: self.delay_time_in_seconds,
        });
        Effect::NoEffect
    }

    fn update(
        &mut self,
        entity_id: EntityId,
        world: &World,
        _physics: &PhysicsWorld,
        time: &Time,
    ) -> Effect {
        let delta_time = time.elapsed.as_secs_f32();

        let (remaining_messages, messages_to_dispatch): (Vec<_>, Vec<_>) = self
            .messages
            .drain(..)
            .map(|mut pending| {
                pending.remaining_seconds -= delta_time;
                pending
            })
            .partition(|pending| pending.remaining_seconds >= 0.0);

        self.messages = remaining_messages;
        let mut eff = Vec::new();
        for pending in messages_to_dispatch {
            // Stored IDs originate from a live message or validated restore.
            let from = pending
                .sender
                .and_then(EntityId::from_inner)
                .unwrap_or(EntityId::dead());
            let msg = if pending.turn_on {
                MessagePayload::TurnOn { from }
            } else {
                MessagePayload::TurnOff { from }
            };
            eff.push(send_to_all_switch_links(world, entity_id, msg));
        }
        Effect::Combined { effects: eff }
    }

    fn script_state_key(&self) -> Option<&'static str> {
        Some(STATE_KEY)
    }

    fn save_state(&self) -> Result<ScriptState, ScriptStateError> {
        ScriptState::encode(1, &self.messages, STATE_KEY)
    }

    fn restore_state(
        &mut self,
        state: &ScriptState,
        context: &ScriptRestoreContext<'_>,
    ) -> Result<(), ScriptStateError> {
        let mut messages: Vec<PendingSwitch> = state.decode(1, STATE_KEY)?;
        for pending in &mut messages {
            pending.sender = match pending.sender {
                Some(sender) => match context.remap_entity(sender) {
                    Ok(sender) => Some(sender.inner()),
                    // A slay-triggered delay commonly outlives its source.
                    // Keep the event, without aliasing a newly loaded entity.
                    Err(ScriptStateError::MissingEntityReference(_)) => None,
                    Err(error) => return Err(error),
                },
                None => None,
            };
        }
        self.messages = messages;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, time::Duration};

    use dark::properties::{Link, Links, ToLink, WrappedEntityId};

    use super::*;

    fn fixture() -> (World, EntityId, EntityId, EntityId) {
        let mut world = World::new();
        let sender = world.add_entity(());
        let target = world.add_entity(());
        let trap = world.add_entity((
            PropDelayTime {
                delay: Duration::from_secs(5),
            },
            Links {
                to_links: vec![ToLink {
                    to_template_id: 1,
                    to_entity_id: Some(WrappedEntityId(target)),
                    link: Link::SwitchLink,
                }],
            },
        ));
        (world, trap, sender, target)
    }

    fn tick(
        script: &mut TrapDelay,
        world: &World,
        trap: EntityId,
        seconds: f32,
    ) -> Vec<(bool, EntityId, EntityId)> {
        Effect::flatten(vec![script.update(
            trap,
            world,
            &PhysicsWorld::new(),
            &Time {
                elapsed: Duration::from_secs_f32(seconds),
                total: Duration::ZERO,
            },
        )])
        .into_iter()
        .filter_map(|effect| match effect {
            Effect::Send { msg } => match msg.payload {
                MessagePayload::TurnOn { from } => Some((true, from, msg.to)),
                MessagePayload::TurnOff { from } => Some((false, from, msg.to)),
                _ => panic!("unexpected delayed payload"),
            },
            _ => None,
        })
        .collect()
    }

    fn round_trip(
        script: &TrapDelay,
        world: &World,
        trap: EntityId,
        remap: &HashMap<EntityId, EntityId>,
    ) -> TrapDelay {
        let bytes = serde_json::to_vec(&script.save_state().unwrap()).unwrap();
        let saved = serde_json::from_slice(&bytes).unwrap();
        let mut loaded = TrapDelay::new();
        loaded.initialize(trap, world);
        loaded
            .restore_state(&saved, &ScriptRestoreContext::new(remap))
            .unwrap();
        loaded
    }

    #[test]
    fn trap_delay_round_trip_preserves_remaining_time_order_and_exactly_once_dispatch() {
        let (world, trap, sender, target) = fixture();
        let physics = PhysicsWorld::new();
        let mut script = TrapDelay::new();
        script.initialize(trap, &world);
        script.handle_message(
            trap,
            &world,
            &physics,
            &MessagePayload::TurnOn { from: sender },
        );
        script.handle_message(
            trap,
            &world,
            &physics,
            &MessagePayload::TurnOff { from: sender },
        );
        assert!(tick(&mut script, &world, trap, 1.0).is_empty());
        script.handle_message(
            trap,
            &world,
            &physics,
            &MessagePayload::TurnOn { from: sender },
        );
        // A different ID makes a stale-handle restore fail the assertion.
        let remap = HashMap::from([(sender, target)]);
        let mut loaded = round_trip(&script, &world, trap, &remap);
        assert!(tick(&mut loaded, &world, trap, 3.9).is_empty());
        assert_eq!(
            tick(&mut loaded, &world, trap, 0.2),
            vec![(true, target, target), (false, target, target)]
        );
        assert!(tick(&mut loaded, &world, trap, 0.8).is_empty());
        assert_eq!(
            tick(&mut loaded, &world, trap, 0.2),
            vec![(true, target, target)]
        );
        assert!(tick(&mut loaded, &world, trap, 10.0).is_empty());
        let mut exhausted = round_trip(&loaded, &world, trap, &HashMap::new());
        assert!(tick(&mut exhausted, &world, trap, 10.0).is_empty());
    }

    #[test]
    fn trap_delay_deleted_sender_survives_repeated_saves_without_stale_handle() {
        let (world, trap, sender, target) = fixture();
        let mut script = TrapDelay::new();
        script.initialize(trap, &world);
        script.handle_message(
            trap,
            &world,
            &PhysicsWorld::new(),
            &MessagePayload::TurnOn { from: sender },
        );
        assert!(tick(&mut script, &world, trap, 1.0).is_empty());
        let loaded = round_trip(&script, &world, trap, &HashMap::new());
        let mut loaded_again = round_trip(&loaded, &world, trap, &HashMap::new());
        assert!(tick(&mut loaded_again, &world, trap, 3.9).is_empty());
        assert_eq!(
            tick(&mut loaded_again, &world, trap, 0.2),
            vec![(true, EntityId::dead(), target)]
        );
    }

    #[test]
    fn trap_delay_ignores_non_switch_messages() {
        let (world, trap, _, _) = fixture();
        let mut script = TrapDelay::new();
        script.initialize(trap, &world);
        script.handle_message(trap, &world, &PhysicsWorld::new(), &MessagePayload::Frob);
        assert!(script.messages.is_empty());
        assert!(tick(&mut script, &world, trap, 10.0).is_empty());
    }

    #[test]
    fn trap_delay_rejects_unknown_saved_version() {
        let mut script = TrapDelay::new();
        let mut saved = script.save_state().unwrap();
        saved.version = 2;
        assert!(matches!(
            script.restore_state(&saved, &ScriptRestoreContext::new(&HashMap::new())),
            Err(ScriptStateError::UnsupportedVersion { .. })
        ));
    }
}
