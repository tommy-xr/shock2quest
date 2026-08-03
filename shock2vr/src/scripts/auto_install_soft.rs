use dark::properties::{PropSoftLevel, PropSoftType};
use shipyard::{EntityId, Get, View, World};
use tracing::warn;

use crate::physics::PhysicsWorld;
use crate::player_stats::Software;

use super::{Effect, MessagePayload, Script};

/// The retail `AutoInstallSoft` script, carried by every `Softs` archetype
/// (hack / modify / repair / research softs, V1-V3).
///
/// Softs are not carryable items: the `Softs` archetype authors a `SCRIPT`
/// world *and* inventory frob action, so frobbing one in the world - or
/// clicking it in a container's loot MFD, which routes a use-only item's click
/// to a `Frob` - installs it and consumes the object rather than moving it
/// into the backpack.
///
/// The soft to install is read from the object: `P$SoftType` selects the
/// software slot and `P$SoftLevel` its version. The compare-and-install (and
/// the consumption) is the applier's job - see `Effect::InstallSoftware` - so
/// this script stays pure.
pub struct AutoInstallSoft;

impl AutoInstallSoft {
    pub fn new() -> AutoInstallSoft {
        AutoInstallSoft
    }
}

impl Script for AutoInstallSoft {
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

        let soft_type = world
            .borrow::<View<PropSoftType>>()
            .ok()
            .and_then(|v| v.get(entity_id).map(|p| p.0).ok())
            .unwrap_or(0);
        let Some(software) = Software::from_soft_type(soft_type) else {
            // The unused `PDA Soft` archetype authors SoftType 0 - it installs
            // nothing, so leave the object alone rather than consuming it.
            warn!("AutoInstallSoft: no software slot for P$SoftType {soft_type}");
            return Effect::NoEffect;
        };

        // Every soft archetype inherits `P$SoftLevel` (the `Softs` base
        // authors 1), so a missing value means unreadable data - treat it as
        // the base version rather than dropping the pickup.
        let level = world
            .borrow::<View<PropSoftLevel>>()
            .ok()
            .and_then(|v| v.get(entity_id).map(|p| p.0).ok())
            .unwrap_or(1);

        Effect::InstallSoftware {
            entity_id,
            software,
            level,
        }
    }
}

#[cfg(test)]
mod tests {
    use dark::properties::{PropSoftLevel, PropSoftType};
    use shipyard::World;

    use crate::{
        physics::PhysicsWorld,
        player_stats::Software,
        scripts::{Effect, MessagePayload, Script},
    };

    use super::AutoInstallSoft;

    fn soft(soft_type: i32, level: Option<i32>) -> (World, shipyard::EntityId) {
        let mut world = World::new();
        let entity = world.add_entity(PropSoftType(soft_type));
        if let Some(level) = level {
            world.add_component(entity, PropSoftLevel(level));
        }
        (world, entity)
    }

    fn frob(world: &World, entity: shipyard::EntityId) -> Effect {
        AutoInstallSoft::new().handle_message(
            entity,
            world,
            &PhysicsWorld::new(),
            &MessagePayload::Frob,
        )
    }

    #[test]
    fn frobbing_a_hack_soft_v2_requests_its_install() {
        // Hack Soft V2 (template -1123): P$SoftType 1, P$SoftLevel 2.
        let (world, entity) = soft(1, Some(2));
        assert!(matches!(
            frob(&world, entity),
            Effect::InstallSoftware {
                entity_id,
                software: Software::Hack,
                level: 2,
            } if entity_id == entity
        ));
    }

    #[test]
    fn soft_type_selects_the_software_slot() {
        for (soft_type, expected) in [
            (1, Software::Hack),
            (2, Software::Modify),
            (3, Software::Repair),
            (4, Software::Research),
        ] {
            let (world, entity) = soft(soft_type, Some(3));
            let Effect::InstallSoftware { software, .. } = frob(&world, entity) else {
                panic!("expected an install for P$SoftType {soft_type}");
            };
            assert_eq!(software, expected);
        }
    }

    #[test]
    fn a_soft_without_an_authored_level_installs_the_base_version() {
        let (world, entity) = soft(4, None);
        assert!(matches!(
            frob(&world, entity),
            Effect::InstallSoftware { level: 1, .. }
        ));
    }

    #[test]
    fn a_soft_with_no_software_slot_is_left_alone() {
        // The unused PDA Soft archetype (-500) authors P$SoftType 0.
        let (world, entity) = soft(0, Some(1));
        assert!(matches!(frob(&world, entity), Effect::NoEffect));

        // An entity with no P$SoftType at all (e.g. restored from a save
        // written before the property was parsed) is likewise left alone.
        let mut world = World::new();
        let entity = world.add_entity(PropSoftLevel(3));
        assert!(matches!(frob(&world, entity), Effect::NoEffect));
    }

    #[test]
    fn unrelated_messages_do_nothing() {
        let (world, entity) = soft(1, Some(2));
        let effect = AutoInstallSoft::new().handle_message(
            entity,
            &world,
            &PhysicsWorld::new(),
            &MessagePayload::TurnOn { from: entity },
        );
        assert!(matches!(effect, Effect::NoEffect));
    }
}
