use dark::properties::{PropDestLevel, PropDestLoc};

use shipyard::{EntityId, Get, View, World};

use crate::physics::PhysicsWorld;

use super::{BaseButton, Effect, MessagePayload, Script};

/// The level-change bulkhead button. The original's script class derives from
/// the base button class, so it *also* behaves like a button: frobbing it
/// relays `TurnOn` over its switch links, and the level change itself happens
/// on `TurnOn`. (The shipped `PropScripts` sets `inherits: false`, which drops
/// the archetype's `BaseButton`, so the port has to compose it explicitly.)
///
/// The `TurnOn` half matters because bulkheads are commonly driven
/// *indirectly*: the button the player touches relays through a quest-bit
/// filter to a hidden co-located changer object, which only ever receives
/// `TurnOn`.
///
/// A frob changes level immediately rather than by way of a self-addressed
/// `TurnOn`: sibling scripts on the same entity can consume the frob too (the
/// rick3 shuttle button also runs `FrobQB`, which destroys the entity), and a
/// deferred message would never reach it.
pub struct LevelChangeButton {
    base_button: BaseButton,
}
impl LevelChangeButton {
    pub fn new() -> LevelChangeButton {
        LevelChangeButton {
            base_button: BaseButton::new(),
        }
    }

    fn transition_effect(&self, entity_id: EntityId, world: &World) -> Effect {
        let v_dest_level = world.borrow::<View<PropDestLevel>>().unwrap();
        let Ok(level_file) = v_dest_level.get(entity_id) else {
            return Effect::NoEffect;
        };

        let v_dest_loc = world.borrow::<View<PropDestLoc>>().unwrap();
        let maybe_dest_loc = v_dest_loc.get(entity_id).ok().map(|dest_loc| dest_loc.0);
        Effect::GlobalEffect(super::GlobalEffect::TransitionLevel {
            level_file: format!("{}.mis", level_file.0),
            loc: maybe_dest_loc,
            entities_to_trigger: vec![],
            vitals_transition: super::PlayerVitalsTransition::Preserve,
        })
    }
}

impl Script for LevelChangeButton {
    fn handle_message(
        &mut self,
        entity_id: EntityId,
        world: &World,
        _physics: &PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        match msg {
            // The activate half is the shared button press (relay + sound). No
            // shipped level-change object has outgoing switch links, and a
            // relay queued alongside a transition would be dropped with the
            // outgoing scene anyway - it is here so the button behaves like a
            // button, not because any mission depends on it.
            MessagePayload::Frob => self
                .base_button
                .locked_effect(entity_id, world)
                .unwrap_or_else(|| {
                    Effect::combine(vec![
                        self.base_button.activate_effect(entity_id, world),
                        self.transition_effect(entity_id, world),
                    ])
                }),
            MessagePayload::TurnOn { from: _ } => self.transition_effect(entity_id, world),
            _ => Effect::NoEffect,
        }
    }
}

#[cfg(test)]
mod tests {
    use dark::properties::{Link, Links, PropDestLevel, ToLink, WrappedEntityId};

    use super::*;

    fn transition_target(effect: &Effect) -> Option<String> {
        match effect {
            Effect::GlobalEffect(super::super::GlobalEffect::TransitionLevel {
                level_file,
                ..
            }) => Some(level_file.clone()),
            Effect::Combined { effects } => effects.iter().find_map(transition_target),
            _ => None,
        }
    }

    fn sent_messages(effect: &Effect) -> Vec<(EntityId, MessagePayload)> {
        match effect {
            Effect::Send { msg } => vec![(msg.to, msg.payload.clone())],
            Effect::Combined { effects } => effects.iter().flat_map(sent_messages).collect(),
            _ => Vec::new(),
        }
    }

    #[test]
    fn turn_on_over_a_switch_link_transitions_the_level() {
        let mut world = World::new();
        let entity_id = world.add_entity(PropDestLevel("ops3".to_owned()));
        let physics = PhysicsWorld::new();
        let mut button = LevelChangeButton::new();

        let effect = button.handle_message(
            entity_id,
            &world,
            &physics,
            &MessagePayload::TurnOn { from: entity_id },
        );

        assert_eq!(transition_target(&effect), Some("ops3.mis".to_owned()));
    }

    #[test]
    fn frob_relays_turn_on_to_switch_links_and_transitions_immediately() {
        let mut world = World::new();
        let changer = world.add_entity(PropDestLevel("ops3".to_owned()));
        let entity_id = world.add_entity((
            PropDestLevel("ops3".to_owned()),
            Links {
                to_links: vec![ToLink {
                    to_template_id: 0,
                    to_entity_id: Some(WrappedEntityId(changer)),
                    link: Link::SwitchLink,
                }],
            },
        ));
        let physics = PhysicsWorld::new();
        let mut button = LevelChangeButton::new();

        let effect = button.handle_message(entity_id, &world, &physics, &MessagePayload::Frob);

        let sent = sent_messages(&effect);
        assert!(
            sent.iter().any(|(to, payload)| *to == changer
                && matches!(payload, MessagePayload::TurnOn { from: _ })),
            "frob must relay TurnOn over switch links, got {sent:?}"
        );
        // The transition is in the frob's own effect, not deferred behind a
        // self-addressed message - a sibling script (rick3's FrobQB) can
        // destroy the entity on the same frob.
        assert_eq!(
            transition_target(&effect),
            Some("ops3.mis".to_owned()),
            "frob must transition immediately, got {sent:?}"
        );
        assert!(
            !sent.iter().any(|(to, _)| *to == entity_id),
            "frob must not defer its own action behind a self-message, got {sent:?}"
        );
    }
}
