use dark::properties::{ObjectState, PropObjState, PropReplicatorHackedContents};
use shipyard::{EntityId, Get, View, World};

use crate::physics::PhysicsWorld;

use super::{Effect, MessagePayload, Script};

const RESONATOR_TEMPLATE_NAME: &str = "big bomb";
const RESONATOR_COST: i32 = 100;

/// Retail's objective-specific catalog augmentation, expressed generically as
/// a script on whichever replicator receives `TurnOn`.
///
/// The original writes hacked slot 1 to `Big Bomb` at 100 nanites. If the
/// machine was hacked before the objective fired, it resets the object to
/// Normal so the newly authored catalog can be reached by hacking it again.
pub struct PutBombInReplicator;

impl PutBombInReplicator {
    pub fn new() -> Self {
        Self
    }
}

impl Script for PutBombInReplicator {
    fn handle_message(
        &mut self,
        entity_id: EntityId,
        world: &World,
        _physics: &PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        if !matches!(msg, MessagePayload::TurnOn { .. }) {
            return Effect::NoEffect;
        }

        let hacked_contents = world
            .borrow::<View<PropReplicatorHackedContents>>()
            .ok()
            .and_then(|contents| contents.get(entity_id).ok().cloned());
        let Some(mut hacked_contents) = hacked_contents else {
            return Effect::NoEffect;
        };

        hacked_contents.object_names[0] = RESONATOR_TEMPLATE_NAME.to_owned();
        hacked_contents.costs[0] = RESONATOR_COST;

        let reset_early_hack = world
            .borrow::<View<PropObjState>>()
            .ok()
            .and_then(|states| states.get(entity_id).ok().copied())
            .is_some_and(|state| state.0 == ObjectState::Hacked);

        let mut effects = vec![Effect::SetReplicatorHackedContents {
            entity_id,
            contents: hacked_contents,
        }];
        if reset_early_hack {
            effects.push(Effect::SetObjectState {
                entity_id,
                state: ObjectState::Normal,
            });
        }
        Effect::combine(effects)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scripts::Effect;

    fn authored_contents() -> PropReplicatorHackedContents {
        PropReplicatorHackedContents {
            costs: [100, 75, 100, 45, 0, 0],
            object_names: [
                "emp grenade".to_owned(),
                "timed grenade".to_owned(),
                "incend. grenade".to_owned(),
                "maintenance tool".to_owned(),
                String::new(),
                String::new(),
            ],
        }
    }

    fn activate(world: &World, replicator: EntityId) -> Vec<Effect> {
        let mut script = PutBombInReplicator::new();
        Effect::flatten(vec![script.handle_message(
            replicator,
            world,
            &PhysicsWorld::new(),
            &MessagePayload::TurnOn { from: replicator },
        )])
    }

    #[test]
    fn turn_on_replaces_only_the_first_hacked_catalog_slot() {
        let mut world = World::new();
        let original = authored_contents();
        let replicator = world.add_entity(original.clone());

        let effects = activate(&world, replicator);

        assert_eq!(effects.len(), 1);
        let Effect::SetReplicatorHackedContents {
            entity_id,
            contents,
        } = &effects[0]
        else {
            panic!("expected a persistent hacked-catalog update, got {effects:?}");
        };
        assert_eq!(*entity_id, replicator);
        assert_eq!(contents.object_names[0], RESONATOR_TEMPLATE_NAME);
        assert_eq!(contents.costs[0], RESONATOR_COST);
        assert_eq!(contents.object_names[1..], original.object_names[1..]);
        assert_eq!(contents.costs[1..], original.costs[1..]);
    }

    #[test]
    fn turn_on_resets_an_already_hacked_replicator_for_rehacking() {
        let mut world = World::new();
        let replicator = world.add_entity((authored_contents(), PropObjState(ObjectState::Hacked)));

        let effects = activate(&world, replicator);

        assert!(effects.iter().any(|effect| matches!(
            effect,
            Effect::SetReplicatorHackedContents { entity_id, .. } if *entity_id == replicator
        )));
        assert!(effects.iter().any(|effect| matches!(
            effect,
            Effect::SetObjectState {
                entity_id,
                state: ObjectState::Normal,
            } if *entity_id == replicator
        )));
    }

    #[test]
    fn ignores_non_turn_on_messages_and_entities_without_a_hacked_catalog() {
        let mut world = World::new();
        let replicator = world.add_entity(authored_contents());
        let missing_catalog = world.add_entity(());
        let mut script = PutBombInReplicator::new();

        assert!(matches!(
            script.handle_message(
                replicator,
                &world,
                &PhysicsWorld::new(),
                &MessagePayload::TurnOff { from: replicator },
            ),
            Effect::NoEffect
        ));
        assert!(activate(&world, missing_catalog).is_empty());
    }
}
