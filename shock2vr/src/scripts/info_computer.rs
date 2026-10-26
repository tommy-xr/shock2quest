use dark::properties::PropHelpText;
use engine::audio::AudioHandle;
use shipyard::{EntityId, Get, View, World};

use crate::physics::PhysicsWorld;

use super::{Effect, MessagePayload, Script, script_util::play_environmental_sound};

/// A TriOptimum information terminal. Using one plays its activate schema and
/// unlocks the help topics in its `P$HelpText` mask; a terminal with no topics
/// toggles the map instead, as the MAP utility button does.
pub struct InfoComputer;

impl Script for InfoComputer {
    fn handle_message(
        &mut self,
        entity_id: EntityId,
        world: &World,
        _physics: &PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        if !matches!(msg, MessagePayload::Frob) {
            return Effect::NoEffect;
        }
        let bits = world
            .borrow::<View<PropHelpText>>()
            .unwrap()
            .get(entity_id)
            .map_or(0, |help| help.0);
        let action = if bits == 0 {
            Effect::ToggleMap
        } else {
            Effect::UnlockHelp { bits }
        };
        Effect::combine(vec![
            play_environmental_sound(world, entity_id, "activate", vec![], AudioHandle::new()),
            action,
        ])
    }
}

#[cfg(test)]
mod tests {
    use cgmath::{Matrix4, SquareMatrix};
    use dark::properties::PropClassTag;

    use super::*;
    use crate::runtime_props::RuntimePropTransform;

    fn frob(world: &World, terminal: EntityId) -> Vec<Effect> {
        Effect::flatten(vec![InfoComputer.handle_message(
            terminal,
            world,
            &PhysicsWorld::new(),
            &MessagePayload::Frob,
        )])
    }

    fn terminal(world: &mut World, help: Option<u32>) -> EntityId {
        let terminal = world.add_entity((
            RuntimePropTransform(Matrix4::identity()),
            PropClassTag::from_string("devicetype cmptr"),
        ));
        if let Some(bits) = help {
            world.add_component(terminal, PropHelpText(bits));
        }
        terminal
    }

    fn plays_activate(effects: &[Effect]) -> bool {
        effects.iter().any(|effect| {
            matches!(effect, Effect::PlayEnvironmentalSound { query, .. }
                if format!("{query:?}").contains("activate"))
        })
    }

    #[test]
    fn frob_unlocks_the_terminal_topics_with_its_activate_sound() {
        let mut world = World::new();
        let terminal = terminal(&mut world, Some(1 << 4));
        let effects = frob(&world, terminal);
        assert!(plays_activate(&effects));
        assert!(
            effects
                .iter()
                .any(|effect| matches!(effect, Effect::UnlockHelp { bits: 16 }))
        );
        assert!(!effects.iter().any(|e| matches!(e, Effect::ToggleMap)));
    }

    #[test]
    fn a_terminal_without_topics_opens_the_map() {
        let mut world = World::new();
        for help in [None, Some(0)] {
            let terminal = terminal(&mut world, help);
            let effects = frob(&world, terminal);
            assert!(plays_activate(&effects));
            assert!(effects.iter().any(|e| matches!(e, Effect::ToggleMap)));
            assert!(
                !effects
                    .iter()
                    .any(|e| matches!(e, Effect::UnlockHelp { .. }))
            );
        }
    }
}
