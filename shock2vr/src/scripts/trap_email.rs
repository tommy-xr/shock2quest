use dark::properties::PropLog;
use shipyard::{EntityId, Get, UniqueView, View, World};

use crate::{physics::PhysicsWorld, quest_info::QuestInfo};

use super::{
    Effect, MessagePayload, Script,
    script_util::{send_to_all_switch_links, set_quest_bit_effect},
};

pub struct TrapEmail {}
impl TrapEmail {
    pub fn new() -> TrapEmail {
        TrapEmail {}
    }
}

/// Whether the trap's authored objective effect would move existing campaign
/// progress backwards. Late-arriving legacy saves can legitimately contain a
/// completed objective while this one-shot email trap is still alive.
fn is_quest_bit_downgrade(world: &World, effect: &Effect) -> bool {
    let Effect::SetQuestBit {
        quest_bit_name,
        quest_bit_value,
    } = effect
    else {
        return false;
    };
    world
        .borrow::<UniqueView<QuestInfo>>()
        .map(|quests| quests.read_quest_bit_value(quest_bit_name).bits() > quest_bit_value.bits())
        .unwrap_or(false)
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
                    Ok(log) if log.deck > 0 && log.email > 0 => Effect::PlayEmail {
                        deck: log.deck,
                        email: log.email,
                        force: false,
                    },
                    _ => Effect::NoEffect,
                };

                // The email trap also carries the objective it hands out
                // (PropQuestBitName/PropQuestBitValue) - the email and the
                // objective it describes are authored on the same entity.
                let quest_bit_effect = set_quest_bit_effect(world, entity_id)
                    .filter(|effect| !is_quest_bit_downgrade(world, effect))
                    .unwrap_or(Effect::NoEffect);

                let switchlink_effects = send_to_all_switch_links(
                    world,
                    entity_id,
                    MessagePayload::TurnOn { from: entity_id },
                );

                // The trap is consumed once it fires. A late legacy save can
                // already have completed this objective while the email trap
                // remains alive, so suppress only the backward quest-bit write;
                // the email, switch-link relay, and one-shot consumption still
                // occur normally.
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
    use dark::properties::{
        Link, Links, PropQuestBitName, PropQuestBitValue, QuestBitValue, ToLink, WrappedEntityId,
    };

    use crate::quest_info::QuestInfo;

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
    fn completed_objective_is_not_downgraded_but_email_still_fires_and_consumes() {
        let mut world = World::new();
        let mut quests = QuestInfo::new();
        quests.set_quest_bit_value("Note_5_7", QuestBitValue::COMPLETE);
        world.add_unique(quests);
        let destination = world.add_entity(());
        let entity_id = world.add_entity((
            PropLog {
                deck: 5,
                email: 9,
                log: 33,
                note: 0,
                video: 0,
            },
            PropQuestBitName("Note_5_7".to_owned()),
            PropQuestBitValue(QuestBitValue::INCOMPLETE),
            Links {
                to_links: vec![ToLink {
                    to_template_id: 27,
                    to_entity_id: Some(WrappedEntityId(destination)),
                    link: Link::SwitchLink,
                }],
            },
        ));

        let effect = TrapEmail::new().handle_message(
            entity_id,
            &world,
            &PhysicsWorld::new(),
            &MessagePayload::TurnOn { from: entity_id },
        );
        let flattened = Effect::flatten(vec![effect.clone()]);

        assert!(
            quest_bits(&effect).is_empty(),
            "an arrival email must not regress a completed objective"
        );
        assert!(
            plays_email(&effect),
            "the late arrival email must still play"
        );
        assert!(
            flattened
                .iter()
                .any(|effect| matches!(effect, Effect::Send { msg } if msg.to == destination))
        );
        assert!(flattened.iter().any(
            |effect| matches!(effect, Effect::DestroyEntity { entity_id: destroyed } if *destroyed == entity_id)
        ));
    }
}
