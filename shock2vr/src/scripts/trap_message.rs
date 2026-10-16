use std::time::Duration;

use dark::properties::PropMsgTime;
use shipyard::{EntityId, Get, View, World};

use crate::physics::PhysicsWorld;

use super::{Effect, MessagePayload, Script, world_frob::use_message_text};

/// `TrapMessage`: on a switch-on, puts its own `P$UseMsg` text on the HUD
/// status-message line (eng2's broken lift buttons, station's mission
/// postings) for its `P$MsgTime`, or the default time without one. Switch-off
/// shows nothing, and the trap has no output of its own to forward to.
pub struct TrapMessage;

impl TrapMessage {
    pub fn new() -> TrapMessage {
        TrapMessage
    }
}

impl Script for TrapMessage {
    fn handle_message(
        &mut self,
        entity_id: EntityId,
        world: &World,
        _physics: &PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        match msg {
            MessagePayload::TurnOn { from: _ } => {
                let Some(text) = use_message_text(world, entity_id) else {
                    return Effect::NoEffect;
                };
                let millis = world
                    .borrow::<View<PropMsgTime>>()
                    .ok()
                    .and_then(|v| v.get(entity_id).ok().map(|time| time.0))
                    .and_then(|ms| u64::try_from(ms).ok())
                    .filter(|ms| *ms > 0);
                match millis {
                    Some(ms) => Effect::ShowMessageFor {
                        text,
                        duration: Duration::from_millis(ms),
                    },
                    None => Effect::ShowMessage { text },
                }
            }
            _ => Effect::NoEffect,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::physics::PhysicsWorld;
    use crate::string_table::StringTable;
    use dark::properties::PropUseMsg;

    fn world_with(raw: &str, strings: &[(&str, &str)]) -> (World, EntityId) {
        let mut world = World::new();
        let entity_id = world.add_entity((PropUseMsg(raw.to_string()),));
        world.add_unique(StringTable::from_table(
            "usemsg.str",
            strings
                .iter()
                .map(|(key, value)| (key.to_string(), value.to_string()))
                .collect(),
        ));
        (world, entity_id)
    }

    fn turn_on(world: &World, entity_id: EntityId) -> Effect {
        TrapMessage::new().handle_message(
            entity_id,
            world,
            &PhysicsWorld::new(),
            &MessagePayload::TurnOn {
                from: EntityId::dead(),
            },
        )
    }

    /// eng2's "Message Trap": `P$UseMsg` is the bare key `OutOfOrder`.
    #[test]
    fn turn_on_shows_the_text_its_key_resolves_to() {
        let (world, entity_id) = world_with(
            "OutOfOrder",
            &[(
                "outoforder",
                "This lift has been taken offline for repairs.",
            )],
        );

        match turn_on(&world, entity_id) {
            Effect::ShowMessage { text } => {
                assert_eq!(text, "This lift has been taken offline for repairs.")
            }
            effect => panic!("expected a message, got {effect:?}"),
        }
    }

    /// Station's posting traps author `P$MsgTime` 10000: their line stays up
    /// 10 s, not the default 5 s. The posting's escaped line break is real.
    #[test]
    fn an_authored_message_time_sets_how_long_the_line_stays_up() {
        let (mut world, entity_id) = world_with(
            "Post2",
            &[("post2", r"Survival Training\n(Gain: +2 Endurance)")],
        );
        world.add_component(entity_id, PropMsgTime(10000));

        match turn_on(&world, entity_id) {
            Effect::ShowMessageFor { text, duration } => {
                assert_eq!(text, "Survival Training\n(Gain: +2 Endurance)");
                assert_eq!(duration, Duration::from_secs(10));
            }
            effect => panic!("expected a timed message, got {effect:?}"),
        }
    }

    /// eng2 entity 393 pastes the whole STR line in as the value; with no
    /// table entry the quoted text it carries is what the player sees.
    #[test]
    fn a_pasted_str_line_falls_back_to_its_own_quoted_text() {
        let (world, entity_id) = world_with(
            r#"Broken: "This lift is malfunctioning.""#,
            &[("outoforder", "unrelated")],
        );

        match turn_on(&world, entity_id) {
            Effect::ShowMessage { text } => assert_eq!(text, "This lift is malfunctioning."),
            effect => panic!("expected a message, got {effect:?}"),
        }
    }

    #[test]
    fn a_key_that_resolves_to_nothing_shows_nothing() {
        let (world, entity_id) = world_with("exp1", &[]);

        assert!(matches!(
            turn_on(&world, entity_id),
            Effect::NoEffect | Effect::Multiple(_)
        ));
    }

    #[test]
    fn turn_off_shows_nothing() {
        let (world, entity_id) = world_with("OutOfOrder", &[("outoforder", "Offline.")]);

        let effect = TrapMessage::new().handle_message(
            entity_id,
            &world,
            &PhysicsWorld::new(),
            &MessagePayload::TurnOff {
                from: EntityId::dead(),
            },
        );

        assert!(matches!(effect, Effect::NoEffect));
    }
}
