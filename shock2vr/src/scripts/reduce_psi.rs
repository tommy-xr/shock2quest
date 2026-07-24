use shipyard::{EntityId, World};

use crate::physics::PhysicsWorld;

use super::{Effect, MessagePayload, Script};

/// Earth Psionic Training's room-entry lesson.
///
/// The retail `ReducePsi` script responds to `TurnOn` by setting the player's
/// pool to five psi points. Keeping the value in the script (rather than the
/// Earth mission loader) preserves the authored Tripwire -> SwitchLink ->
/// `ReducePsi` flow and lets any mission object use the same behavior.
pub struct ReducePsi;

impl ReducePsi {
    pub fn new() -> Self {
        Self
    }
}

impl Script for ReducePsi {
    fn handle_message(
        &mut self,
        _entity_id: EntityId,
        _world: &World,
        _physics: &PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        match msg {
            MessagePayload::TurnOn { .. } => Effect::SetPsiPoints { points: 5 },
            _ => Effect::NoEffect,
        }
    }
}

#[cfg(test)]
mod tests {
    use shipyard::World;

    use crate::{
        physics::PhysicsWorld,
        scripts::{Effect, MessagePayload, Script},
    };

    use super::ReducePsi;

    #[test]
    fn turn_on_sets_the_retail_training_value() {
        let mut world = World::new();
        let marker = world.add_entity(());
        let source = world.add_entity(());

        let effect = ReducePsi::new().handle_message(
            marker,
            &world,
            &PhysicsWorld::new(),
            &MessagePayload::TurnOn { from: source },
        );

        assert!(matches!(effect, Effect::SetPsiPoints { points: 5 }));
    }

    #[test]
    fn unrelated_messages_do_nothing() {
        let mut world = World::new();
        let marker = world.add_entity(());
        let effect = ReducePsi::new().handle_message(
            marker,
            &world,
            &PhysicsWorld::new(),
            &MessagePayload::Frob,
        );
        assert!(matches!(effect, Effect::NoEffect));
    }
}
