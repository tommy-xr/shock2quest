use shipyard::{EntityId, World};

use crate::physics::PhysicsWorld;

use super::{Effect, MessagePayload, Script, script_util::get_all_switch_links};

/// Dark's in-world lock trap (`TrapUnlock`).
///
/// The trap sets the lock state of every object it controls through its
/// SwitchLinks: `TurnOn` unlocks them, `TurnOff` locks them again. This is how
/// the missions hand out buttons that have no key - eng1's "Unlock Trap" is
/// fired by a Once Router when main power comes back and unlocks the
/// elevator / grav-lift call buttons, which are authored `PropLocked(true)`
/// with no `PropKeyDst` and are otherwise permanently refused by
/// `script_util::is_entity_locked`. Both edges are authored: rec2 drives its
/// unlock trap from a button (unlock) *and* from an inverter on the dining
/// ambush (re-lock), a lock/unlock cycle on the same card slot.
///
/// The lock lives in Dark's own `P$Locked` property (via `Effect::SetLocked`),
/// so it is the same state every lock check already reads and mission
/// save/load persists it with the rest of the registered properties.
pub struct TrapUnlock {}

impl TrapUnlock {
    pub fn new() -> TrapUnlock {
        TrapUnlock {}
    }
}

impl Script for TrapUnlock {
    fn handle_message(
        &mut self,
        entity_id: EntityId,
        world: &World,
        _physics: &PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        let locked = match msg {
            MessagePayload::TurnOn { from: _ } => false,
            MessagePayload::TurnOff { from: _ } => true,
            _ => return Effect::NoEffect,
        };

        Effect::combine(
            get_all_switch_links(world, entity_id)
                .into_iter()
                .map(|target| Effect::SetLocked {
                    entity_id: target,
                    locked,
                })
                .collect(),
        )
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
    fn authored_trap() -> Box<dyn Script> {
        ScriptWorld::create_script("TrapUnlock".to_owned())
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

    /// Deliver a message to the trap and apply what it emitted, exactly as
    /// `Mission::handle_effects` does: the shared flatten, then the shared
    /// `SetLocked` application.
    fn trigger(world: &mut World, trap: EntityId, msg: MessagePayload) {
        let effect = authored_trap().handle_message(trap, world, &PhysicsWorld::new(), &msg);
        for effect in Effect::flatten(vec![effect]) {
            if let Effect::SetLocked { entity_id, locked } = effect {
                set_entity_locked(world, entity_id, locked);
            }
        }
    }

    #[test]
    fn turning_the_trap_on_unlocks_every_button_it_controls() {
        let mut world = World::new();
        world.add_unique(QuestInfo::new());
        let first = keyless_locked_button(&mut world);
        let second = keyless_locked_button(&mut world);
        let untargeted = keyless_locked_button(&mut world);
        let trap = trap_with_switch_links(&mut world, &[first, second]);

        assert!(is_entity_locked(&world, first));
        assert!(is_entity_locked(&world, second));

        trigger(&mut world, trap, MessagePayload::TurnOn { from: trap });

        assert!(!is_entity_locked(&world, first));
        assert!(!is_entity_locked(&world, second));
        assert!(
            is_entity_locked(&world, untargeted),
            "a button the trap does not control must stay locked"
        );
    }

    /// The mirror edge, authored in rec2: an inverter delivers `TurnOff` and
    /// the card slot locks again.
    #[test]
    fn turning_the_trap_off_locks_the_button_again() {
        let mut world = World::new();
        world.add_unique(QuestInfo::new());
        let button = keyless_locked_button(&mut world);
        let trap = trap_with_switch_links(&mut world, &[button]);

        trigger(&mut world, trap, MessagePayload::TurnOn { from: trap });
        assert!(!is_entity_locked(&world, button));

        trigger(&mut world, trap, MessagePayload::TurnOff { from: trap });
        assert!(is_entity_locked(&world, button));
    }

    #[test]
    fn a_message_that_is_not_a_switch_edge_leaves_the_lock_alone() {
        let mut world = World::new();
        world.add_unique(QuestInfo::new());
        let button = keyless_locked_button(&mut world);
        let trap = trap_with_switch_links(&mut world, &[button]);

        trigger(&mut world, trap, MessagePayload::Frob);

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

        trigger(&mut world, trap, MessagePayload::TurnOn { from: trap });

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
}
