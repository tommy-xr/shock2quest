use super::{Effect, MessagePayload, Script};
use crate::physics::PhysicsWorld;
use dark::properties::QuestBitValue;
use shipyard::{EntityId, World};

/// Earth training temporarily grants the interface through its authored links.
pub struct ChangeInterface;
impl Script for ChangeInterface {
    fn handle_message(
        &mut self,
        _: EntityId,
        _: &World,
        _: &PhysicsWorld,
        message: &MessagePayload,
    ) -> Effect {
        let hidden = match message {
            MessagePayload::TurnOn { .. } => false,
            MessagePayload::TurnOff { .. } => true,
            _ => return Effect::NoEffect,
        };
        Effect::SetQuestBit {
            quest_bit_name: crate::cyber_interface::QUEST_NAME.into(),
            quest_bit_value: if hidden {
                QuestBitValue::INCOMPLETE
            } else {
                QuestBitValue::UNKNOWN
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn authored_switches_enable_and_disable_the_interface() {
        let world = World::new();
        let physics = PhysicsWorld::new();
        let id = EntityId::dead();
        for (message, expected) in [
            (MessagePayload::TurnOn { from: id }, 0),
            (MessagePayload::TurnOff { from: id }, 1),
        ] {
            let Effect::SetQuestBit {
                quest_bit_name,
                quest_bit_value,
            } = ChangeInterface.handle_message(id, &world, &physics, &message)
            else {
                panic!("missing quest effect");
            };
            assert_eq!(quest_bit_name, "HideInterface");
            assert_eq!(quest_bit_value.bits(), expected);
        }
        assert!(matches!(
            ChangeInterface.handle_message(id, &world, &physics, &MessagePayload::Frob),
            Effect::NoEffect
        ));
    }
}
