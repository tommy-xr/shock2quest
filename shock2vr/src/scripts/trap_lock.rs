use shipyard::{EntityId, World};

use crate::physics::PhysicsWorld;

use super::{Effect, MessagePayload, Script, script_util::get_all_switch_links};

/// Dark's in-world lock traps (`TrapLock` / `TrapUnlock`).
///
/// When the trap is triggered it sets the lock state of every object it
/// controls through its SwitchLinks - `TrapUnlock` clears the lock,
/// `TrapLock` sets it. This is the mechanism the missions use to hand out
/// buttons and doors that have no key: e.g. eng1's "Unlock Trap" is fired by
/// a Once Router and unlocks the elevator/grav-lift call buttons, which are
/// authored `PropLocked(true)` with no `PropKeyDst` and are otherwise
/// permanently refused by `script_util::is_entity_locked`.
///
/// The lock lives in Dark's own `P$Locked` property (via `Effect::SetLocked`),
/// so it is the same state every lock check already reads and mission
/// save/load persists it with the rest of the registered properties.
///
/// Only the trigger (`TurnOn`) acts; `TurnOff` is left alone rather than
/// guessing at an inverse, matching the trigger-only traps already here
/// (`TrapSlayer`, `TrapQBSet`). Levels that want the inverse wire a
/// `TrapInverter`.
pub struct TrapLock {
    locked: bool,
}

impl TrapLock {
    /// `TrapLock`: triggering locks the linked objects.
    pub fn lock() -> TrapLock {
        TrapLock { locked: true }
    }

    /// `TrapUnlock`: triggering unlocks the linked objects.
    pub fn unlock() -> TrapLock {
        TrapLock { locked: false }
    }
}

impl Script for TrapLock {
    fn handle_message(
        &mut self,
        entity_id: EntityId,
        world: &World,
        _physics: &PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        match msg {
            MessagePayload::TurnOn { from: _ } => Effect::combine(
                get_all_switch_links(world, entity_id)
                    .into_iter()
                    .map(|target| Effect::SetLocked {
                        entity_id: target,
                        locked: self.locked,
                    })
                    .collect(),
            ),
            _ => Effect::NoEffect,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::quest_info::QuestInfo;
    use crate::scripts::ScriptWorld;
    use crate::scripts::script_util::{is_entity_locked, set_entity_locked};
    use dark::properties::{Link, Links, PropLocked, ToLink, WrappedEntityId};

    /// Resolve the trap the way the game does - by its authored script name -
    /// so the registry wiring is under test alongside the behavior.
    fn authored_script(name: &str) -> Box<dyn Script> {
        ScriptWorld::create_script(name.to_owned())
    }

    /// An eng1-style call button: `PropLocked(true)` with no `PropKeyDst`, so
    /// no key or quest state can ever satisfy it.
    fn keyless_locked_button(world: &mut World) -> EntityId {
        world.add_entity(PropLocked(true))
    }

    fn trap_with_switch_links(world: &mut World, targets: &[EntityId]) -> EntityId {
        world.add_entity(Links {
            to_links: targets
                .iter()
                .map(|target| ToLink {
                    to_template_id: 0,
                    to_entity_id: Some(WrappedEntityId(*target)),
                    link: Link::SwitchLink,
                })
                .collect(),
        })
    }

    /// Apply what the trap emitted, exactly as `Mission::handle_effects` does.
    fn apply(world: &mut World, effect: Effect) {
        for (entity_id, locked) in lock_changes(effect) {
            set_entity_locked(world, entity_id, locked);
        }
    }

    fn lock_changes(effect: Effect) -> Vec<(EntityId, bool)> {
        match effect {
            Effect::SetLocked { entity_id, locked } => vec![(entity_id, locked)],
            Effect::Combined { effects } => effects.into_iter().flat_map(lock_changes).collect(),
            _ => vec![],
        }
    }

    #[test]
    fn triggering_an_unlock_trap_unlocks_every_button_it_controls() {
        let mut world = World::new();
        world.add_unique(QuestInfo::new());
        let first = keyless_locked_button(&mut world);
        let second = keyless_locked_button(&mut world);
        let untargeted = keyless_locked_button(&mut world);
        let trap = trap_with_switch_links(&mut world, &[first, second]);

        assert!(is_entity_locked(&world, first));
        assert!(is_entity_locked(&world, second));

        let effect = authored_script("TrapUnlock").handle_message(
            trap,
            &world,
            &PhysicsWorld::new(),
            &MessagePayload::TurnOn { from: trap },
        );
        apply(&mut world, effect);

        assert!(!is_entity_locked(&world, first));
        assert!(!is_entity_locked(&world, second));
        assert!(
            is_entity_locked(&world, untargeted),
            "a button the trap does not control must stay locked"
        );
    }

    #[test]
    fn triggering_a_lock_trap_locks_the_button_it_controls() {
        let mut world = World::new();
        world.add_unique(QuestInfo::new());
        let button = world.add_entity(PropLocked(false));
        let trap = trap_with_switch_links(&mut world, &[button]);

        assert!(!is_entity_locked(&world, button));

        let effect = authored_script("TrapLock").handle_message(
            trap,
            &world,
            &PhysicsWorld::new(),
            &MessagePayload::TurnOn { from: trap },
        );
        apply(&mut world, effect);

        assert!(is_entity_locked(&world, button));
    }

    /// The unlock must outlive the session: the lock lives in the registered
    /// `P$Locked` property, so a normal mission save/load carries it.
    #[test]
    fn a_trap_unlocked_button_stays_unlocked_across_a_save_load_round_trip() {
        use crate::save_load::EntitySaveData;
        use std::collections::HashMap;
        use std::fs::File;

        let mut world = World::new();
        world.add_unique(QuestInfo::new());
        let button = keyless_locked_button(&mut world);
        let still_locked = keyless_locked_button(&mut world);
        let trap = trap_with_switch_links(&mut world, &[button]);

        let effect = authored_script("TrapUnlock").handle_message(
            trap,
            &world,
            &PhysicsWorld::new(),
            &MessagePayload::TurnOn { from: trap },
        );
        apply(&mut world, effect);

        let (all_properties, _, _) = dark::properties::get::<File>();
        let mut save = EntitySaveData::empty();
        save.all_entities = vec![button.inner(), still_locked.inner()];
        save.properties = all_properties
            .iter()
            .map(|prop| (prop.name(), prop.serialize(&world)))
            .collect::<HashMap<_, _>>();

        let mut loaded = World::new();
        loaded.add_unique(QuestInfo::new());
        let (_, old_to_new) = save.instantiate(&mut loaded);

        assert!(
            !is_entity_locked(&loaded, old_to_new[&button]),
            "the unlock must survive save/load"
        );
        assert!(is_entity_locked(&loaded, old_to_new[&still_locked]));
    }

    #[test]
    fn an_untriggered_unlock_trap_leaves_its_target_locked() {
        let mut world = World::new();
        world.add_unique(QuestInfo::new());
        let button = keyless_locked_button(&mut world);
        let trap = trap_with_switch_links(&mut world, &[button]);

        let effect = authored_script("TrapUnlock").handle_message(
            trap,
            &world,
            &PhysicsWorld::new(),
            &MessagePayload::TurnOff { from: trap },
        );
        apply(&mut world, effect);

        assert!(is_entity_locked(&world, button));
    }
}
