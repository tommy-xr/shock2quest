//! Kinetic Redirection: aimed acquisition through the normal inventory/hand paths.
use crate::{
    Handedness,
    mission::PlayerInfo,
    physics::{InternalCollisionGroups, PhysicsWorld},
};
use dark::properties::{
    FrobFlag, Link, Links, PropAI, PropFrobInfo, PropImmobile, PropPsiNotPullable, PropRenderType,
    RenderType,
};
use shipyard::{EntityId, Get, IntoIter, UniqueView, View, World};

pub const POWER: i32 = -1022;
// Port interaction tuning, in world units. The power's data[0] is not a verified range.
pub const RANGE: f32 = 12.0;

#[derive(Clone, Copy)]
pub enum Destination {
    Inventory,
    Hand(Handedness),
    Script,
}

pub struct PullTarget {
    pub entity: EntityId,
    pub destination: Destination,
    pub origin: cgmath::Vector3<f32>,
}

pub(crate) fn eligible(world: &World, item: EntityId) -> bool {
    if !crate::virtual_hand::can_grab_item(world, item) || !crate::util::has_refs(world, item) {
        return false;
    }
    if world
        .borrow::<View<PropFrobInfo>>()
        .unwrap()
        .get(item)
        .is_ok_and(|p| p.world_action.contains(FrobFlag::IGNORE))
    {
        return false;
    }
    let (ais, immobile, blocked, render, links) = world
        .borrow::<(
            View<PropAI>,
            View<PropImmobile>,
            View<PropPsiNotPullable>,
            View<PropRenderType>,
            View<Links>,
        )>()
        .unwrap();
    !ais.contains(item)
        && !immobile.get(item).is_ok_and(|p| p.0)
        && !blocked.get(item).is_ok_and(|p| p.0)
        && !render
            .get(item)
            .is_ok_and(|p| matches!(p.0, RenderType::NoRender | RenderType::EditorOnly))
        && !links.iter().any(|l| {
            l.to_links.iter().any(|l| {
                matches!(l.link, Link::Contains(_)) && l.to_entity_id.is_some_and(|e| e.0 == item)
            })
        })
}

pub fn resolve(world: &World, physics: &PhysicsWorld, amp: EntityId) -> Option<PullTarget> {
    use cgmath::EuclideanSpace;
    let player = world.borrow::<UniqueView<PlayerInfo>>().ok()?;
    if player.left_hand_entity_id != Some(amp) && player.right_hand_entity_id != Some(amp) {
        return None;
    }
    let (origin, forward) = crate::scripts::amp_aim_ray(world, amp)?;
    // The first obstruction wins: ineligible props and world walls cannot be skipped.
    let hit = physics.ray_cast2_with_entity_filter(
        origin,
        forward,
        RANGE,
        InternalCollisionGroups::WORLD
            | InternalCollisionGroups::ENTITIES
            | InternalCollisionGroups::SELECTABLE
            | InternalCollisionGroups::HITBOX,
        Some(amp),
        true,
        &|id| {
            id != player.entity_id
                && Some(id) != player.left_hand_entity_id
                && Some(id) != player.right_hand_entity_id
        },
    )?;
    let entity = crate::util::resolve_proxy_entity(world, hit.maybe_entity_id?);
    if !eligible(world, entity) {
        return None;
    }
    let destination = if crate::virtual_hand::uses_scripted_world_frob(world, entity) {
        Destination::Script
    } else if !crate::mission::presentation_is_vr(world) {
        Destination::Inventory
    } else if player.left_hand_entity_id.is_none() {
        Destination::Hand(Handedness::Left)
    } else if player.right_hand_entity_id.is_none() {
        Destination::Hand(Handedness::Right)
    } else {
        return None;
    };
    Some(PullTarget {
        entity,
        destination,
        origin: hit.hit_point.to_vec(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_loose_authored_pullable_items_are_eligible() {
        let mut world = World::new();
        let item = world.add_entity(PropFrobInfo {
            world_action: FrobFlag::MOVE,
            inventory_action: FrobFlag::empty(),
            tool_action: FrobFlag::empty(),
        });
        assert!(eligible(&world, item));
        world.add_component(item, PropPsiNotPullable(true));
        assert!(!eligible(&world, item));
        world.add_component(item, PropPsiNotPullable(false));
        world.add_component(item, PropImmobile(true));
        assert!(!eligible(&world, item));
        world.add_component(item, PropImmobile(false));
        world.add_component(item, PropAI("Human".into()));
        assert!(!eligible(&world, item));
        world.remove::<PropAI>(item);
        world.add_component(item, PropRenderType(RenderType::NoRender));
        assert!(!eligible(&world, item));
        world.remove::<PropRenderType>(item);
        world.add_component(item, dark::properties::PropHasRefs(false));
        assert!(!eligible(&world, item));
        world.add_component(item, dark::properties::PropHasRefs(true));
        world.add_entity(Links {
            to_links: vec![dark::properties::ToLink {
                link: Link::Contains(0),
                to_template_id: 0,
                to_entity_id: Some(dark::properties::WrappedEntityId(item)),
            }],
        });
        assert!(!eligible(&world, item));
    }
}
