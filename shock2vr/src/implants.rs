//! Powered equipment sockets. Base training stays in QuestInfo; effective
//! values are derived from carried, equipped, powered items on every read.
use crate::{
    player_stats::PlayerStats, quest_info::QuestInfo, runtime_props::RuntimePropImplantSlot,
};
use dark::properties::{ObjectState, PropEnergy, PropImplantDesc, PropObjState};
use shipyard::{EntityId, Get, UniqueView, View, World};

pub fn capacity(world: &World) -> usize {
    if world.borrow::<UniqueView<QuestInfo>>().is_ok_and(|q| {
        q.player_stats()
            .has_os_trait(crate::scripts::gui::TRAIT_CYBERNETICALLY_ENHANCED)
    }) {
        2
    } else {
        1
    }
}

pub fn equipped(world: &World) -> [Option<EntityId>; 2] {
    let mut items = [None; 2];
    let slots = world.borrow::<View<RuntimePropImplantSlot>>().unwrap();
    for id in crate::scripts::script_util::player_carried_items(world) {
        if let Ok(slot) = slots.get(id) {
            if usize::from(slot.0) < capacity(world) {
                items[usize::from(slot.0)] = Some(id);
            }
        }
    }
    items
}

pub fn kind(world: &World, entity: EntityId) -> Option<i32> {
    world
        .borrow::<View<PropImplantDesc>>()
        .ok()?
        .get(entity)
        .ok()
        .map(|i| i.0)
}

pub fn energy(world: &World, entity: EntityId) -> f32 {
    world
        .borrow::<View<PropEnergy>>()
        .ok()
        .and_then(|v| v.get(entity).ok().map(|e| e.0))
        .unwrap_or(0.0)
}

pub fn active(world: &World, implant_kind: i32) -> bool {
    equipped(world)
        .into_iter()
        .flatten()
        .any(|id| kind(world, id) == Some(implant_kind) && energy(world, id) > 0.0)
}

/// Original BaseImplant Recharge: 100 + ten times BASE Maintenance. This is
/// capacity, not a change to the authored one-charge-per-ten-seconds drain.
pub fn recharge_capacity(world: &World) -> f32 {
    let maintenance = world
        .borrow::<UniqueView<QuestInfo>>()
        .map(|q| q.player_stats().skills.maintenance)
        .unwrap_or(0)
        .max(0);
    100.0 + 10.0 * maintenance as f32
}

/// None means an equipped item is being removed. Distinct implant kinds only;
/// two copies of BrawnBoost cannot stack. All validation happens again in the
/// effect applier, so two Frob messages in one frame cannot overfill a socket.
pub fn toggle_slot(world: &World, entity: EntityId) -> Result<Option<u8>, &'static str> {
    if !crate::scripts::script_util::player_carried_items(world).contains(&entity) {
        return Err("Pick up the implant first.");
    }
    let implant_kind = kind(world, entity).ok_or("This item is not an implant.")?;
    let current = equipped(world);
    if current.contains(&Some(entity)) {
        return Ok(None);
    }
    if world.borrow::<View<PropObjState>>().is_ok_and(|v| {
        v.get(entity)
            .is_ok_and(|p| p.0 == ObjectState::Unresearched)
    }) {
        return Err("Research this implant before equipping it.");
    }
    if energy(world, entity) <= 0.0 {
        return Err("Recharge this implant first.");
    }
    if current
        .into_iter()
        .flatten()
        .any(|id| kind(world, id) == Some(implant_kind))
    {
        return Err("Two implants of the same type cannot be equipped together.");
    }
    current[..capacity(world)]
        .iter()
        .position(Option::is_none)
        .map(|slot| Some(slot as u8))
        .ok_or("Remove an implant before equipping another.")
}

pub fn effective_stats(world: &World) -> Option<PlayerStats> {
    let mut stats = world
        .borrow::<UniqueView<QuestInfo>>()
        .ok()?
        .player_stats()
        .clone();
    for id in equipped(world)
        .into_iter()
        .flatten()
        .filter(|id| energy(world, *id) > 0.0)
    {
        match kind(world, id) {
            Some(0) => stats.strength += 1,
            Some(1) => stats.endurance += 1,
            Some(2) => stats.agility += 1,
            Some(3) => stats.psionic_ability += 1,
            // Classic OBJLOOKS: ExperTech boosts these three rolls only. It
            // neither trains Maintenance nor satisfies minimum requirements.
            Some(7) => {
                stats.skills.hack += 1;
                stats.skills.repair += 1;
                stats.skills.modify += 1;
            }
            Some(8) => stats.skills.research += 1,
            _ => {}
        }
    }
    Some(stats)
}

#[cfg(test)]
mod tests {
    use super::*;
    use dark::properties::{Link, Links, ToLink, WrappedEntityId};
    fn fixture(enhanced: bool) -> (World, EntityId, EntityId, EntityId) {
        let mut world = World::new();
        let a = world.add_entity((PropImplantDesc(0), PropEnergy(100.0)));
        let b = world.add_entity((PropImplantDesc(1), PropEnergy(100.0)));
        let duplicate = world.add_entity((PropImplantDesc(0), PropEnergy(100.0)));
        let player = world.add_entity(Links {
            to_links: [a, b, duplicate]
                .into_iter()
                .enumerate()
                .map(|(slot, id)| ToLink {
                    to_template_id: 0,
                    to_entity_id: Some(WrappedEntityId(id)),
                    link: Link::Contains(slot as u32),
                })
                .collect(),
        });
        world.add_unique(crate::mission::PlayerInfo {
            entity_id: player,
            inventory_entity_id: player,
            left_hand_entity_id: None,
            right_hand_entity_id: None,
            pos: cgmath::vec3(0.0, 0.0, 0.0),
            rotation: cgmath::Quaternion::new(1.0, 0.0, 0.0, 0.0),
        });
        let mut quests = QuestInfo::new();
        if enhanced {
            quests.player_stats_mut().add_os_trait(7);
        }
        world.add_unique(quests);
        (world, a, b, duplicate)
    }
    #[test]
    fn second_socket_requires_trait_and_never_accepts_duplicates() {
        for enhanced in [false, true] {
            let (mut world, a, b, duplicate) = fixture(enhanced);
            assert_eq!(toggle_slot(&world, a), Ok(Some(0)));
            world.add_component(a, RuntimePropImplantSlot(0));
            assert_eq!(toggle_slot(&world, b).is_ok(), enhanced);
            assert!(toggle_slot(&world, duplicate).is_err());
            assert_eq!(toggle_slot(&world, a), Ok(None));
        }
    }
    #[test]
    fn power_and_ownership_control_bonuses_without_changing_training() {
        let (mut world, a, b, _) = fixture(true);
        world.add_component(a, RuntimePropImplantSlot(0));
        world.add_component(b, RuntimePropImplantSlot(1));
        let stats = effective_stats(&world).unwrap();
        assert_eq!((stats.strength, stats.endurance), (2, 2));
        assert_eq!(
            world
                .borrow::<UniqueView<QuestInfo>>()
                .unwrap()
                .player_stats()
                .strength,
            1
        );
        world.add_component(a, PropEnergy(0.0));
        assert_eq!(effective_stats(&world).unwrap().strength, 1);
        let player = world
            .borrow::<UniqueView<crate::mission::PlayerInfo>>()
            .unwrap()
            .entity_id;
        world.add_component(player, Links::empty());
        assert_eq!(equipped(&world), [None, None]);
        assert_eq!(effective_stats(&world).unwrap().endurance, 1);
    }
}
