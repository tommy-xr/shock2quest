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
}

pub struct Flight {
    pub target: PullTarget,
    pub amp: EntityId,
    pub gravity: f32,
    pub age: f32,
    pub trail_age: f32,
}

pub const FLIGHT_SPEED: f32 = 6.0;
pub const ARRIVAL_DISTANCE: f32 = 0.25;
pub const FLIGHT_TIMEOUT: f32 = 5.0;

/// Recheck the route even inside the arrival radius: a thin entity door must
/// not turn the final hand/inventory transfer into a teleport through it.
pub fn route_blocked(
    physics: &PhysicsWorld,
    from: cgmath::Vector3<f32>,
    to: cgmath::Vector3<f32>,
    ignored: &[EntityId],
) -> bool {
    use cgmath::{EuclideanSpace, InnerSpace, Point3};
    let delta = to - from;
    let distance = delta.magnitude();
    distance > 0.001
        && physics
            .ray_cast2_with_entity_filter(
                Point3::from_vec(from),
                delta / distance,
                distance,
                InternalCollisionGroups::WORLD
                    | InternalCollisionGroups::ENTITIES
                    | InternalCollisionGroups::HITBOX,
                None,
                true,
                &|id| !ignored.contains(&id),
            )
            .is_some()
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
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn thin_entity_door_blocks_even_inside_the_arrival_radius() {
        use cgmath::{Quaternion, vec3};
        let mut entities = World::new();
        let door = entities.add_entity(());
        let player_id = entities.add_entity(());
        let mut physics = PhysicsWorld::new();
        physics.add_kinematic(
            door,
            vec3(0.0, 2.0, 0.0),
            Quaternion::new(1.0, 0.0, 0.0, 0.0),
            vec3(0.0, 0.0, 0.0),
            vec3(0.02, 2.0, 2.0),
            crate::physics::CollisionGroup::entity(),
            false,
        );
        let mut player = physics.create_player(vec3(100.0, 100.0, 100.0), player_id);
        physics.update(vec3(0.0, 0.0, 0.0), &mut player);
        let from = vec3(-0.1, 2.0, 0.0);
        let to = vec3(0.1, 2.0, 0.0);
        assert!(route_blocked(&physics, from, to, &[]));
        assert!(
            !route_blocked(&physics, from, to, &[door]),
            "own equipment can be excluded"
        );
    }

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
