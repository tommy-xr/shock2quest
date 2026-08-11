use engine::audio::AudioHandle;
use shipyard::{EntityId, Get, View, World};

use crate::physics::PhysicsWorld;

use super::{
    Effect, MessagePayload, Script,
    script_util::{play_environmental_sound, send_to_all_switch_links},
};

pub struct BaseButton {}
impl BaseButton {
    pub fn new() -> BaseButton {
        BaseButton {}
    }

    pub fn is_locked(&self, entity_id: EntityId, world: &World) -> bool {
        super::script_util::is_entity_locked(world, entity_id)
    }

    /// Whether Dark's authored lock is still set, independent of whether the
    /// player currently carries a key that can clear it. Remote signals must
    /// use this state: possessing a card is not the same as presenting it to
    /// the reader.
    fn lock_is_set(&self, entity_id: EntityId, world: &World) -> bool {
        world
            .borrow::<View<dark::properties::PropLocked>>()
            .unwrap()
            .get(entity_id)
            .map(|lock| lock.0)
            .unwrap_or(false)
    }

    /// The refusal a locked button gives instead of activating, or `None` when
    /// it is free to activate. Button subclasses gate their own action on this.
    pub fn locked_effect(&self, entity_id: EntityId, world: &World) -> Option<Effect> {
        self.is_locked(entity_id, world).then(|| Effect::PlaySound {
            handle: AudioHandle::new(),
            source: Some(entity_id),
            name: "hackfail".to_owned(),
            spatial: false,
        })
    }

    /// What every button does when it is pressed, apart from its own action:
    /// relay `TurnOn` to its switch links and play the activate sound. The
    /// button's *own* action is the caller's business - `BaseButton` notifies
    /// itself, subclasses run theirs directly.
    pub fn activate_effect(&self, entity_id: EntityId, world: &World) -> Effect {
        let switch_link_effect =
            send_to_all_switch_links(world, entity_id, MessagePayload::TurnOn { from: entity_id });
        let sound_effect =
            play_environmental_sound(world, entity_id, "activate", vec![], AudioHandle::new());
        Effect::combine(vec![switch_link_effect, sound_effect])
    }
}
impl Script for BaseButton {
    fn handle_message(
        &mut self,
        entity_id: EntityId,
        world: &World,
        _physics: &PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        match msg {
            MessagePayload::Frob => self.locked_effect(entity_id, world).unwrap_or_else(|| {
                // A plain button's own action is to notify itself, so proxy
                // scripts on the same entity see the press as a TurnOn.
                let notify_self = Effect::Send {
                    msg: super::Message {
                        payload: MessagePayload::TurnOn { from: entity_id },
                        to: entity_id,
                    },
                };
                let unlock = self
                    .lock_is_set(entity_id, world)
                    .then_some(Effect::SetLocked {
                        entity_id,
                        locked: false,
                    });
                Effect::combine(
                    unlock
                        .into_iter()
                        .chain([self.activate_effect(entity_id, world), notify_self])
                        .collect(),
                )
            }),

            // In some places (like the computer for the engine room in eng1), invisible buttons are used as proxies -
            // there will be an actual button that sends a 'TurnOn' message to an invisible button. Not sure why
            // this pattern is used. A remote press still honors the proxy's lock;
            // otherwise tripwires can relay through authored locked card slots.
            MessagePayload::TurnOn { from: _ } if !self.lock_is_set(entity_id, world) => {
                send_to_all_switch_links(
                    world,
                    entity_id,
                    MessagePayload::TurnOn { from: entity_id },
                )
            }

            _ => Effect::NoEffect,
        }
    }
}

#[cfg(test)]
mod tests {
    use dark::properties::{KeyCard, Link, Links, PropKeyDst, PropLocked, ToLink, WrappedEntityId};

    use crate::quest_info::QuestInfo;

    use super::*;

    fn sent_messages(effect: &Effect) -> Vec<(EntityId, MessagePayload)> {
        match effect {
            Effect::Send { msg } => vec![(msg.to, msg.payload.clone())],
            Effect::Combined { effects } => effects.iter().flat_map(sent_messages).collect(),
            _ => Vec::new(),
        }
    }

    fn lock_updates(effect: &Effect) -> Vec<(EntityId, bool)> {
        match effect {
            Effect::SetLocked { entity_id, locked } => vec![(*entity_id, *locked)],
            Effect::Combined { effects } => effects.iter().flat_map(lock_updates).collect(),
            _ => Vec::new(),
        }
    }

    fn button_with_switch_link(world: &mut World, target: EntityId, locked: bool) -> EntityId {
        world.add_entity((
            PropLocked(locked),
            Links {
                to_links: vec![ToLink {
                    to_template_id: 0,
                    to_entity_id: Some(WrappedEntityId(target)),
                    link: Link::SwitchLink,
                }],
            },
        ))
    }

    #[test]
    fn turn_on_only_relays_when_the_button_is_unlocked() {
        let mut world = World::new();
        world.add_unique(QuestInfo::new());
        let target = world.add_entity(());
        let locked = button_with_switch_link(&mut world, target, true);
        let unlocked = button_with_switch_link(&mut world, target, false);
        let physics = PhysicsWorld::new();
        let mut button = BaseButton::new();

        let locked_effect = button.handle_message(
            locked,
            &world,
            &physics,
            &MessagePayload::TurnOn { from: locked },
        );
        assert!(
            sent_messages(&locked_effect).is_empty(),
            "a locked button must not relay a scripted TurnOn"
        );

        let unlocked_effect = button.handle_message(
            unlocked,
            &world,
            &physics,
            &MessagePayload::TurnOn { from: unlocked },
        );
        assert!(
            sent_messages(&unlocked_effect)
                .iter()
                .any(|(to, payload)| *to == target
                    && matches!(payload, MessagePayload::TurnOn { from } if *from == unlocked)),
            "an unlocked proxy button must keep relaying scripted TurnOn"
        );
    }

    #[test]
    fn card_reader_does_not_relay_turn_on_until_it_is_frobbed() {
        let mut world = World::new();
        let key = KeyCard {
            is_master: false,
            region_id: 256,
            lock_id: 0,
        };
        let mut quest = QuestInfo::new();
        quest.add_key_card(key.clone());
        world.add_unique(quest);
        let target = world.add_entity(());
        let reader = world.add_entity((
            PropLocked(true),
            PropKeyDst(key),
            Links {
                to_links: vec![ToLink {
                    to_template_id: 0,
                    to_entity_id: Some(WrappedEntityId(target)),
                    link: Link::SwitchLink,
                }],
            },
        ));
        let physics = PhysicsWorld::new();
        let mut button = BaseButton::new();

        let relayed = button.handle_message(
            reader,
            &world,
            &physics,
            &MessagePayload::TurnOn { from: reader },
        );

        assert!(
            sent_messages(&relayed).is_empty(),
            "possessing a matching card must not bypass a still-locked reader"
        );
    }

    #[test]
    fn frobbing_a_reader_without_its_card_keeps_it_locked_and_quiet() {
        let mut world = World::new();
        world.add_unique(QuestInfo::new());
        let target = world.add_entity(());
        let reader = world.add_entity((
            PropLocked(true),
            PropKeyDst(KeyCard {
                is_master: false,
                region_id: 128,
                lock_id: 0,
            }),
            Links {
                to_links: vec![ToLink {
                    to_template_id: 0,
                    to_entity_id: Some(WrappedEntityId(target)),
                    link: Link::SwitchLink,
                }],
            },
        ));
        let physics = PhysicsWorld::new();
        let mut button = BaseButton::new();

        let effect = button.handle_message(reader, &world, &physics, &MessagePayload::Frob);

        assert!(lock_updates(&effect).is_empty());
        assert!(sent_messages(&effect).is_empty());
        assert!(
            matches!(effect, Effect::PlaySound { ref name, .. } if name == "hackfail"),
            "an invalid reader frob must give the locked-button refusal"
        );
    }

    #[test]
    fn frobbing_a_reader_with_its_card_unlocks_and_activates_it() {
        let mut world = World::new();
        let key = KeyCard {
            is_master: false,
            region_id: 128,
            lock_id: 0,
        };
        let mut quest = QuestInfo::new();
        quest.add_key_card(key.clone());
        world.add_unique(quest);
        let target = world.add_entity(());
        let reader = world.add_entity((
            PropLocked(true),
            PropKeyDst(key.clone()),
            Links {
                to_links: vec![ToLink {
                    to_template_id: 0,
                    to_entity_id: Some(WrappedEntityId(target)),
                    link: Link::SwitchLink,
                }],
            },
        ));
        let physics = PhysicsWorld::new();
        let mut button = BaseButton::new();

        let effect = button.handle_message(reader, &world, &physics, &MessagePayload::Frob);

        assert!(
            lock_updates(&effect).contains(&(reader, false)),
            "a legitimate card-reader frob must clear the authored lock"
        );
        assert!(
            sent_messages(&effect)
                .iter()
                .any(|(to, payload)| *to == target
                    && matches!(payload, MessagePayload::TurnOn { from } if *from == reader)),
            "the legitimate frob must still activate the reader's switch links"
        );
        assert!(
            world
                .borrow::<shipyard::UniqueView<QuestInfo>>()
                .unwrap()
                .can_unlock(&key),
            "unlocking a reader must retain the key card"
        );
    }
}
