use dark::properties::PropLog;
use shipyard::{EntityId, Get, View, World};

use crate::physics::PhysicsWorld;

use super::{
    Effect, MessagePayload, Script,
    script_util::{send_to_all_switch_links, set_quest_bit_effect},
};

/// Empty `PropLog` bitmasks decode as `trailing_zeros + 1 == 33`.
const EMAIL_UNSET: u32 = 33;

pub struct TrapEmail {}
impl TrapEmail {
    pub fn new() -> TrapEmail {
        TrapEmail {}
    }
}

impl Script for TrapEmail {
    fn handle_message(
        &mut self,
        entity_id: EntityId,
        world: &World,
        _physics: &PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        match msg {
            MessagePayload::TurnOn { from: _ } => {
                let v_log = world.borrow::<View<PropLog>>().unwrap();
                let email_effect = match v_log.get(entity_id) {
                    Ok(log) if log.deck > 0 && log.email > 0 && log.email != EMAIL_UNSET => {
                        Effect::PlayEmail {
                            deck: log.deck,
                            email: log.email,
                            force: false,
                        }
                    }
                    _ => Effect::NoEffect,
                };

                // The email trap also carries the objective it hands out
                // (PropQuestBitName/PropQuestBitValue) - the email and the
                // objective it describes are authored on the same entity.
                let quest_bit_effect =
                    set_quest_bit_effect(world, entity_id).unwrap_or(Effect::NoEffect);

                let switchlink_effects = send_to_all_switch_links(
                    world,
                    entity_id,
                    MessagePayload::TurnOn { from: entity_id },
                );

                // The trap is consumed once it fires. The tripwires that feed
                // these traps fire on every crossing, and re-applying the quest
                // bit would knock an already-COMPLETE objective back to
                // INCOMPLETE (QuestInfo::set_quest_bit_value overwrites).
                // has_played_email only suppresses the audio, not the objective
                // or the switch-link relay.
                Effect::Combined {
                    effects: vec![
                        email_effect,
                        quest_bit_effect,
                        switchlink_effects,
                        Effect::DestroyEntity { entity_id },
                    ],
                }
            }
            // Does turn off need to be done for email?
            _ => Effect::NoEffect,
        }
    }
}

#[cfg(test)]
mod tests {
    use dark::properties::{PropQuestBitName, PropQuestBitValue, QuestBitValue};

    use super::*;

    fn quest_bits(effect: &Effect) -> Vec<(String, QuestBitValue)> {
        match effect {
            Effect::SetQuestBit {
                quest_bit_name,
                quest_bit_value,
            } => vec![(quest_bit_name.clone(), *quest_bit_value)],
            Effect::Combined { effects } => effects.iter().flat_map(quest_bits).collect(),
            _ => Vec::new(),
        }
    }

    fn plays_email(effect: &Effect) -> bool {
        match effect {
            Effect::PlayEmail { .. } => true,
            Effect::Combined { effects } => effects.iter().any(plays_email),
            _ => false,
        }
    }

    #[test]
    fn turn_on_grants_the_authored_objective() {
        let mut world = World::new();
        let entity_id = world.add_entity((
            PropLog {
                deck: 4,
                email: 1,
                log: 33,
                note: 0,
                video: 0,
            },
            PropQuestBitName("Note_4_2".to_owned()),
            PropQuestBitValue(QuestBitValue::INCOMPLETE),
        ));
        let physics = PhysicsWorld::new();
        let mut trap = TrapEmail::new();

        let effect = trap.handle_message(
            entity_id,
            &world,
            &physics,
            &MessagePayload::TurnOn { from: entity_id },
        );

        assert_eq!(
            quest_bits(&effect),
            vec![("Note_4_2".to_owned(), QuestBitValue::INCOMPLETE)]
        );
        assert!(
            plays_email(&effect),
            "the objective must be granted alongside the email, not instead of it"
        );
    }

    #[test]
    fn unset_email_sentinel_does_not_play_email_33() {
        let mut world = World::new();
        let entity_id = world.add_entity(PropLog {
            deck: 2,
            email: EMAIL_UNSET,
            log: 1,
            note: 0,
            video: 0,
        });
        let physics = PhysicsWorld::new();

        let effect = TrapEmail::new().handle_message(
            entity_id,
            &world,
            &physics,
            &MessagePayload::TurnOn { from: entity_id },
        );

        assert!(!plays_email(&effect));
    }
}
