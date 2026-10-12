//! Kinetic Redirection: fly loose physics bodies toward the held amp.
use crate::{
    mission::PlayerInfo,
    physics::{InternalCollisionGroups, PhysicsWorld},
};
use dark::properties::{
    FrobFlag, Link, Links, PropAI, PropFrobInfo, PropImmobile, PropPsiNotPullable, PropRenderType,
    PropRotatingDoor, PropTranslatingDoor, RenderType,
};
use shipyard::{EntityId, Get, IntoIter, UniqueView, View, World};

pub const POWER: i32 = -1022;
// Port interaction tuning, in world units. The power's data[0] is not a verified range.
pub const RANGE: f32 = 12.0;
/// Aim assist half-angle: an exact ray rarely lands on a patch lying flat a
/// few metres away, especially from a tracked hand.
pub const AIM_CONE_DEGREES: f32 = 8.0;

pub struct Flight {
    pub item: EntityId,
    pub amp: EntityId,
    pub hand: Option<usize>,
    pub gravity: f32,
    pub age: f32,
    pub trail_age: f32,
}

pub const FLIGHT_SPEED: f32 = 6.0;
pub const ARRIVAL_DISTANCE: f32 = 0.25;
pub const FLIGHT_TIMEOUT: f32 = 5.0;

/// The visible amp muzzle, in both presentations (the same transform as its
/// healing feedback). Flat casting uses a camera aim ray, not this endpoint.
pub fn amp_position(world: &World, amp: EntityId) -> Option<cgmath::Vector3<f32>> {
    use cgmath::{EuclideanSpace, Transform};
    let transforms = world
        .borrow::<View<crate::runtime_props::RuntimePropTransform>>()
        .ok()?;
    let transform = transforms.get(amp).ok()?;
    Some(
        transform
            .0
            .transform_point(crate::weapon_muzzle::resolve(world, amp).point)
            .to_vec(),
    )
}

/// Recheck the route even inside the arrival radius: a thin entity door must
/// not let the flight finish on the far side of a thin obstacle.
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
    pullable(world, item) && !contained(world, item)
}

/// The per-frame flight check. It skips the containment scan: containing an
/// item removes its physics body, which already cancels the flight.
pub(crate) fn pullable(world: &World, item: EntityId) -> bool {
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
    let (ais, immobile, blocked, render) = world
        .borrow::<(
            View<PropAI>,
            View<PropImmobile>,
            View<PropPsiNotPullable>,
            View<PropRenderType>,
        )>()
        .unwrap();
    !ais.contains(item)
        && !immobile.get(item).is_ok_and(|p| p.0)
        && !blocked.get(item).is_ok_and(|p| p.0)
        && !render
            .get(item)
            .is_ok_and(|p| matches!(p.0, RenderType::NoRender | RenderType::EditorOnly))
}

fn contained(world: &World, item: EntityId) -> bool {
    world.borrow::<View<Links>>().unwrap().iter().any(|l| {
        l.to_links.iter().any(|l| {
            matches!(l.link, Link::Contains(_)) && l.to_entity_id.is_some_and(|e| e.0 == item)
        })
    })
}

pub fn resolve(world: &World, physics: &PhysicsWorld, amp: EntityId) -> Option<EntityId> {
    let player = world.borrow::<UniqueView<PlayerInfo>>().ok()?;
    if player.left_hand_entity_id != Some(amp) && player.right_hand_entity_id != Some(amp) {
        return None;
    }
    let (origin, forward) = crate::scripts::amp_aim_ray(world, amp)?;
    resolve_ray(world, physics, amp, origin, forward)
}

pub(crate) fn resolve_ray(
    world: &World,
    physics: &PhysicsWorld,
    amp: EntityId,
    origin: cgmath::Point3<f32>,
    forward: cgmath::Vector3<f32>,
) -> Option<EntityId> {
    use cgmath::{EuclideanSpace, InnerSpace};
    let player = world.borrow::<UniqueView<PlayerInfo>>().ok()?;
    let own = [
        amp,
        player.entity_id,
        player.left_hand_entity_id.unwrap_or(player.entity_id),
        player.right_hand_entity_id.unwrap_or(player.entity_id),
    ];
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
        &|id| !own.contains(&id),
    );
    // A pickup under the exact aim wins, and a door keeps its trigger rather
    // than losing it to an item nearby. Anything else - floor, walls, set
    // dressing - falls through to the cone.
    if let Some(entity) = hit
        .and_then(|hit| hit.maybe_entity_id)
        .map(|id| crate::util::resolve_proxy_entity(world, id))
    {
        if eligible(world, entity) {
            return Some(entity);
        }
        if is_door(world, entity) {
            return None;
        }
    }
    // Otherwise take the item nearest the aim inside the cone, in plain sight
    // of the caster.
    let forward = forward.normalize();
    let min_cos = AIM_CONE_DEGREES.to_radians().cos();
    let mut candidates: Vec<_> = physics
        .dynamic_bodies()
        .filter_map(|(entity, position)| {
            let offset = position - origin.to_vec();
            let distance = offset.magnitude();
            let cos = offset.dot(forward) / distance;
            (distance <= RANGE && cos >= min_cos).then_some((entity, position, cos))
        })
        .collect();
    candidates.sort_by(|a, b| b.2.total_cmp(&a.2));
    candidates
        .into_iter()
        .find(|(entity, position, _)| {
            pullable(world, *entity)
                && !route_blocked(
                    physics,
                    origin.to_vec(),
                    *position,
                    &[own[0], own[1], own[2], own[3], *entity],
                )
                && !contained(world, *entity)
        })
        .map(|(entity, _, _)| entity)
}

fn is_door(world: &World, entity: EntityId) -> bool {
    let (translating, rotating) = world
        .borrow::<(View<PropTranslatingDoor>, View<PropRotatingDoor>)>()
        .unwrap();
    translating.contains(entity) || rotating.contains(entity)
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

    /// A small loose item 4 units down -X, a wall slot at x=-2, and a player
    /// parked far away so its capsule cannot intercept the rays.
    fn cone_fixture() -> (World, PhysicsWorld, EntityId, EntityId, EntityId) {
        use crate::physics::{CollisionGroup, DynamicPhysicsOptions, PhysicsShape};
        use cgmath::{Quaternion, vec3};
        let mut world = World::new();
        let amp = world.add_entity(());
        let player_id = world.add_entity(());
        world.add_unique(PlayerInfo {
            pos: vec3(100.0, 100.0, 100.0),
            rotation: Quaternion::new(1.0, 0.0, 0.0, 0.0),
            entity_id: player_id,
            left_hand_entity_id: Some(amp),
            right_hand_entity_id: None,
            inventory_entity_id: player_id,
        });
        let item = world.add_entity(PropFrobInfo {
            world_action: FrobFlag::MOVE,
            inventory_action: FrobFlag::empty(),
            tool_action: FrobFlag::empty(),
        });
        let mut physics = PhysicsWorld::new();
        physics.add_dynamic(
            item,
            vec3(-4.0, 0.0, 0.0),
            Quaternion::new(1.0, 0.0, 0.0, 0.0),
            vec3(0.0, 0.0, 0.0),
            PhysicsShape::Cuboid(vec3(0.1, 0.02, 0.1)),
            CollisionGroup::entity(),
            false,
            DynamicPhysicsOptions::default(),
        );
        let mut player = physics.create_player(vec3(100.0, 100.0, 100.0), player_id);
        physics.update(vec3(0.0, 0.0, 0.0), &mut player);
        (world, physics, amp, item, player_id)
    }

    fn aim(physics: &PhysicsWorld, world: &World, amp: EntityId, degrees: f32) -> Option<EntityId> {
        // From 1 unit up, the item sits ~14 degrees below level; aim that much
        // down, then swing `degrees` off to the side.
        let down = (1.0f32 / 4.0).atan();
        let side = degrees.to_radians();
        let forward = cgmath::vec3(
            -down.cos() * side.cos(),
            -down.sin(),
            down.cos() * side.sin(),
        );
        resolve_ray(
            world,
            physics,
            amp,
            cgmath::Point3::new(0.0, 1.0, 0.0),
            forward,
        )
    }

    #[test]
    fn a_near_miss_inside_the_cone_still_finds_a_flat_item() {
        let (world, physics, amp, item, _) = cone_fixture();
        assert_eq!(aim(&physics, &world, amp, 0.0), Some(item));
        assert_eq!(
            aim(&physics, &world, amp, AIM_CONE_DEGREES - 2.0),
            Some(item)
        );
        assert_eq!(aim(&physics, &world, amp, AIM_CONE_DEGREES + 4.0), None);
    }

    fn add_slab(
        world: &mut World,
        physics: &mut PhysicsWorld,
        player_id: EntityId,
        center: cgmath::Vector3<f32>,
        size: cgmath::Vector3<f32>,
    ) -> EntityId {
        let slab = world.add_entity(());
        physics.add_kinematic(
            slab,
            center,
            cgmath::Quaternion::new(1.0, 0.0, 0.0, 0.0),
            cgmath::vec3(0.0, 0.0, 0.0),
            size,
            crate::physics::CollisionGroup::entity(),
            false,
        );
        let mut player = physics.create_player(cgmath::vec3(100.0, 100.0, 100.0), player_id);
        physics.update(cgmath::vec3(0.0, 0.0, 0.0), &mut player);
        slab
    }

    #[test]
    fn the_cone_cannot_see_through_cover() {
        use cgmath::vec3;
        let (mut world, mut physics, amp, item, player_id) = cone_fixture();
        assert_eq!(aim(&physics, &world, amp, -5.0), Some(item));
        // A slab beside the aim: the exact ray passes it, the sight line does not.
        add_slab(
            &mut world,
            &mut physics,
            player_id,
            vec3(-3.0, 0.5, 0.3),
            vec3(0.1, 1.0, 0.8),
        );
        assert_eq!(aim(&physics, &world, amp, -5.0), None);
    }

    #[test]
    fn a_door_under_the_aim_keeps_the_trigger() {
        use cgmath::vec3;
        let (mut world, mut physics, amp, item, player_id) = cone_fixture();
        // A slab beyond the item, under a 6 degree aim: set dressing there
        // falls through to the cone, a door does not.
        let slab = add_slab(
            &mut world,
            &mut physics,
            player_id,
            vec3(-6.0, -0.5, 0.6),
            vec3(0.1, 1.0, 0.6),
        );
        assert_eq!(aim(&physics, &world, amp, 6.0), Some(item));
        let rest = vec3(-6.0, -0.5, 0.6);
        let identity = cgmath::Quaternion::new(1.0, 0.0, 0.0, 0.0);
        world.add_component(
            slab,
            PropRotatingDoor {
                door_type: 0,
                closed: 0.0,
                open: 90.0,
                speed: 1.0,
                axis: 1,
                state: 0,
                clockwise: false,
                base_closed_location: rest,
                base_open_location: rest,
                base_location: rest,
                base_rotation: identity,
                base_closed_rotation: identity,
                base_open_rotation: identity,
                progress: 0.0,
            },
        );
        assert_eq!(aim(&physics, &world, amp, 6.0), None);
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
