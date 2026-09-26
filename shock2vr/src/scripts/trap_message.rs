use dark::{importers::resolve_localized_property_string, properties::PropUseMsg};
use shipyard::{EntityId, Get, UniqueView, View, World};

use crate::physics::PhysicsWorld;
use crate::string_table::StringTable;

use super::{Effect, MessagePayload, Script};

/// Resolve an entity's `P$UseMsg` to the text the player is shown: a key into
/// USEMSG.STR, or the quoted text the property carries when the key misses.
fn message_text(world: &World, entity_id: EntityId) -> Option<String> {
    let v_use_msg = world.borrow::<View<PropUseMsg>>().ok()?;
    let raw = v_use_msg.get(entity_id).ok()?;
    let strings = world
        .borrow::<UniqueView<StringTable>>()
        .ok()
        .and_then(|strings| strings.table("usemsg.str").cloned())
        .unwrap_or_default();
    let text = resolve_localized_property_string(&raw.0, &strings);
    (!text.is_empty()).then_some(text)
}

/// `TrapMessage`: on a switch-on, puts its own `P$UseMsg` text on the HUD
/// status-message line (eng2's broken lift buttons). Switch-off shows nothing,
/// and the trap has no output of its own to forward to.
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
            MessagePayload::TurnOn { from: _ } => match message_text(world, entity_id) {
                Some(text) => Effect::ShowMessage { text },
                None => Effect::NoEffect,
            },
            _ => Effect::NoEffect,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::physics::PhysicsWorld;
    use crate::string_table::StringTable;

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
