use std::collections::{HashMap, HashSet};

use cgmath::{InnerSpace, Vector3};
use dark::{
    SCALE_FACTOR,
    mission::{room::Room, room_database::RoomDatabase},
    properties::{Link, Links, PropElevAble, PropPosition},
};
use serde::{Deserialize, Serialize};
use shipyard::{EntityId, Get, IntoIter, IntoWithId, Unique, UniqueView, View, World};

use crate::mission::find_start_marker;

/// Authored room planes, independent of the vertically shifted room sensors.
#[derive(Unique)]
pub(crate) struct ElevatorRooms(pub RoomDatabase);

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ElevatorCargo {
    pub marker: i32,
    pub offsets: HashMap<u64, Vector3<f32>>,
}

fn contains(room: &Room, point: Vector3<f32>) -> bool {
    room.planes.iter().all(|plane| {
        let inside = plane.n.dot(room.center * SCALE_FACTOR) - plane.d;
        let side = plane.n.dot(point * SCALE_FACTOR) - plane.d;
        if inside < 0.0 {
            side <= 0.001
        } else {
            side >= -0.001
        }
    })
}

/// Overlap shallower than this is grazing contact, left to the solver.
/// Cabins differ slightly between decks (eng1's walls sit 0.02 closer).
const EMBED_TOLERANCE: f32 = 0.05;

/// Fallback directions around the arrival point, nearest-first in order.
const FALLBACK_RING: [(f32, f32); 8] = [
    (1.0, 0.0),
    (-1.0, 0.0),
    (0.0, 1.0),
    (0.0, -1.0),
    (0.7071, 0.7071),
    (0.7071, -0.7071),
    (-0.7071, 0.7071),
    (-0.7071, -0.7071),
];
/// Just outside the player capsule (radius 0.48) that spawns at the arrival
/// point after cargo is placed.
const FALLBACK_RADIUS: f32 = 0.6;
/// Stacked rings, so a large load still finds room before overlapping.
const FALLBACK_HEIGHTS: [f32; 3] = [0.0, 0.4, 0.8];

fn fallback_spots(arrival: Vector3<f32>) -> impl Iterator<Item = Vector3<f32>> {
    FALLBACK_HEIGHTS.into_iter().flat_map(move |y| {
        FALLBACK_RING
            .iter()
            .map(move |(x, z)| arrival + Vector3::new(x * FALLBACK_RADIUS, y, z * FALLBACK_RADIUS))
    })
}

/// The elevator's StartLoc marker position. Unlike a player spawn, cargo
/// needs the exact marker: a near match would move some other room's items.
fn elevator_marker(world: &World, marker: i32) -> Option<Vector3<f32>> {
    find_start_marker(world, marker, true).map(|found| found.position)
}

/// Move a restored entity before its first step, keeping its physics state in sync.
fn set_restored_position(world: &mut World, entity: EntityId, position: Vector3<f32>) {
    let Some(mut prop) = world
        .borrow::<View<PropPosition>>()
        .unwrap()
        .get(entity)
        .ok()
        .cloned()
    else {
        return;
    };
    prop.position = position;
    prop.cell = 0;
    let state = world
        .borrow::<View<dark::properties::PropPhysState>>()
        .unwrap()
        .get(entity)
        .ok()
        .cloned();
    if let Some(mut state) = state {
        state.position = position;
        world.add_component(entity, state);
    }
    world.add_component(entity, prop);
}

impl ElevatorCargo {
    fn remapped<'a>(
        &'a self,
        remap: &'a HashMap<EntityId, EntityId>,
    ) -> impl Iterator<Item = (EntityId, Vector3<f32>)> + 'a {
        self.offsets.iter().filter_map(|(old, offset)| {
            let entity = EntityId::from_inner(*old).and_then(|id| remap.get(&id))?;
            Some((*entity, *offset))
        })
    }

    /// Like the original, cargo that would sink into destination geometry
    /// or leave the elevator's room falls back to the player's arrival point. Cabins differ slightly
    /// between decks; letting the solver eject an embedded item can lose it
    /// through a wall before the player gets control. Cargo overlapping other
    /// cargo kept that arrangement from the departure deck, so it is ignored.
    /// Recovered items spread over rings around the arrival point so they do
    /// not start inside each other; past the last ring they overlap there.
    pub(crate) fn recover_blocked_cargo(
        &self,
        world: &mut World,
        physics: &mut crate::physics::PhysicsWorld,
        remap: &HashMap<EntityId, EntityId>,
        arrival: Vector3<f32>,
    ) {
        let cargo: Vec<_> = self.remapped(remap).map(|(entity, _)| entity).collect();
        // Depth against the level mesh reads shallow once an item is well past
        // a wall face, so an item must also stay inside the elevator's room.
        let room = elevator_marker(world, self.marker).and_then(|origin| {
            let rooms = world.borrow::<UniqueView<ElevatorRooms>>().ok()?;
            rooms
                .0
                .rooms
                .iter()
                .find(|room| contains(room, origin))
                .cloned()
        });
        let blocked_by = |physics: &crate::physics::PhysicsWorld, entity, ignored: &[EntityId]| {
            let outside = room.as_ref().is_some_and(|room| {
                physics.entity_body_pose(entity).is_some_and(|pose| {
                    let t = pose.translation.vector;
                    !contains(room, Vector3::new(t.x, t.y, t.z))
                })
            });
            outside || physics.entity_is_embedded(entity, ignored, EMBED_TOLERANCE)
        };
        let mut blocked: Vec<_> = cargo
            .iter()
            .copied()
            .filter(|&entity| blocked_by(physics, entity, &cargo))
            .collect();
        blocked.sort_by_key(|entity| entity.inner());
        for entity in blocked {
            let rotation = world
                .borrow::<View<PropPosition>>()
                .unwrap()
                .get(entity)
                .map(|position| position.rotation)
                .unwrap_or(cgmath::Quaternion::new(1.0, 0.0, 0.0, 0.0));
            // First spot clear of terrain and everything already placed.
            let spot = fallback_spots(arrival)
                .find(|&spot| {
                    physics.set_position_rotation2(entity, spot, rotation);
                    !blocked_by(physics, entity, &[entity])
                })
                .unwrap_or(arrival);
            physics.set_position_rotation2(entity, spot, rotation);
            set_restored_position(world, entity, spot);
        }
    }

    /// The departure room is the elevator marker's room, not the player's;
    /// only ElevAble roots (plus their contained objects) travel.
    pub(crate) fn collect(world: &World, marker: i32, carried: &HashSet<u64>) -> Option<Self> {
        let rooms = world.borrow::<UniqueView<ElevatorRooms>>().ok()?;
        let origin = elevator_marker(world, marker)?;
        let room = rooms.0.rooms.iter().find(|room| contains(room, origin))?;
        let positions = world.borrow::<View<PropPosition>>().unwrap();
        let eligible = world.borrow::<View<PropElevAble>>().unwrap();
        // Contained loot can retain an old world position. Only its container
        // decides whether it travels; carried gear already has its own path.
        let links = world.borrow::<View<Links>>().unwrap();
        let contained: HashSet<_> = links
            .iter()
            .flat_map(|links| &links.to_links)
            .filter(|link| matches!(link.link, Link::Contains(_)))
            .filter_map(|link| link.to_entity_id.map(|id| id.0))
            .collect();
        let offsets = (&eligible, &positions)
            .iter()
            .with_id()
            .filter(|(id, (able, pos))| {
                able.0
                    && !carried.contains(&id.inner())
                    && !contained.contains(id)
                    && contains(room, pos.position)
            })
            .map(|(id, (_, pos))| (id.inner(), pos.position - origin))
            .collect();
        Some(Self { marker, offsets })
    }

    /// Apply before creating models/physics so no body is built at its old
    /// deck's coordinates. Retail translates offsets without rotating them.
    pub(crate) fn place(&self, world: &mut World, remap: &HashMap<EntityId, EntityId>) {
        let Some(destination) = elevator_marker(world, self.marker) else {
            tracing::warn!("Elevator destination has no StartLoc {}", self.marker);
            return;
        };
        // Retail lifts cargo half a Dark unit to keep settled objects out of
        // the destination floor. Orientation remains unchanged.
        let lift = Vector3::new(0.0, 0.5 / SCALE_FACTOR, 0.0);
        for (entity, offset) in self.remapped(remap) {
            set_restored_position(world, entity, destination + offset + lift);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cgmath::{Point3, Quaternion, vec3};
    use collision::{Aabb3, Plane};
    use dark::properties::{PropStartLoc, ToLink, WrappedEntityId};

    fn position(x: f32) -> PropPosition {
        PropPosition {
            position: vec3(x, 0.0, 0.0),
            cell: 0,
            rotation: Quaternion::new(1.0, 0.0, 0.0, 0.0),
        }
    }

    #[test]
    fn blocked_arrival_recovers_near_player_but_clear_cargo_keeps_its_offset() {
        use crate::physics::{CollisionGroup, DynamicPhysicsOptions, PhysicsShape, PhysicsWorld};
        let mut world = World::new();
        let wall = world.add_entity(());
        let mut physics = PhysicsWorld::new();
        let rotation = Quaternion::new(1.0, 0.0, 0.0, 0.0);
        // Spans x -0.5..0.5.
        physics.add_kinematic_shape(
            wall,
            vec3(0.0, 0.0, 0.0),
            rotation,
            vec3(0.0, 0.0, 0.0),
            PhysicsShape::Cuboid(vec3(1.0, 1.0, 1.0)),
            CollisionGroup::entity(),
            false,
        );
        // Two items inside the wall, one grazing its face, one in the open.
        let xs = [0.0, 0.1, 0.59, 3.0];
        let items: Vec<_> = xs
            .iter()
            .map(|&x| {
                let entity = world.add_entity(position(x));
                physics.add_dynamic(
                    entity,
                    vec3(x, 0.0, 0.0),
                    rotation,
                    vec3(0.0, 0.0, 0.0),
                    PhysicsShape::Sphere(0.1),
                    CollisionGroup::entity(),
                    false,
                    DynamicPhysicsOptions::default(),
                );
                entity
            })
            .collect();
        let cargo = ElevatorCargo {
            marker: 22,
            offsets: items
                .iter()
                .zip(xs)
                .map(|(entity, x)| (entity.inner(), vec3(x, 0.0, 0.0)))
                .collect(),
        };
        let arrival = vec3(5.0, 0.0, 0.0);
        let remap = items.iter().map(|&entity| (entity, entity)).collect();
        cargo.recover_blocked_cargo(&mut world, &mut physics, &remap, arrival);

        let positions = world.borrow::<View<PropPosition>>().unwrap();
        let at = |i: usize| positions.get(items[i]).unwrap().position;
        let (first, second) = (at(0), at(1));
        for recovered in [first, second] {
            assert!((recovered - arrival).magnitude() < 1.0);
        }
        // Radius-0.1 spheres: anything closer than 0.2 starts interpenetrating.
        assert!((first - second).magnitude() >= 0.2);
        assert_eq!(at(2), vec3(0.59, 0.0, 0.0), "grazing a wall is not blocked");
        assert_eq!(at(3), vec3(3.0, 0.0, 0.0));
        let body_x = |i: usize| {
            physics
                .entity_body_pose(items[i])
                .unwrap()
                .translation
                .vector
                .x
        };
        assert_eq!(body_x(0), first.x);
        assert_eq!(body_x(1), second.x);
    }

    /// The level mesh only reports shallow depth for an item already past a
    /// wall face; leaving the elevator's room is what catches it.
    #[test]
    fn cargo_outside_the_elevator_room_recovers_inside_it() {
        use crate::physics::{CollisionGroup, DynamicPhysicsOptions, PhysicsShape, PhysicsWorld};
        let mut world = fixture();
        let mut physics = PhysicsWorld::new();
        let (inside, outside) = (
            world.add_entity(position(1.0)),
            world.add_entity(position(3.0)),
        );
        for (entity, x) in [(inside, 1.0), (outside, 3.0)] {
            physics.add_dynamic(
                entity,
                vec3(x, 0.0, 0.0),
                Quaternion::new(1.0, 0.0, 0.0, 0.0),
                vec3(0.0, 0.0, 0.0),
                PhysicsShape::Sphere(0.1),
                CollisionGroup::entity(),
                false,
                DynamicPhysicsOptions::default(),
            );
        }
        let cargo = ElevatorCargo {
            marker: 22,
            offsets: HashMap::from([
                (inside.inner(), vec3(1.0, 0.0, 0.0)),
                (outside.inner(), vec3(3.0, 0.0, 0.0)),
            ]),
        };
        let remap = HashMap::from([(inside, inside), (outside, outside)]);
        cargo.recover_blocked_cargo(&mut world, &mut physics, &remap, vec3(0.0, 0.0, 0.0));
        let positions = world.borrow::<View<PropPosition>>().unwrap();
        assert_eq!(positions.get(inside).unwrap().position, vec3(1.0, 0.0, 0.0));
        // The room spans -2..2; the first ring spot is 0.6 from the arrival.
        assert!(positions.get(outside).unwrap().position.magnitude() < 1.0);
    }

    fn fixture() -> World {
        let mut world = World::new();
        world.add_entity((PropStartLoc(22), position(0.0)));
        world.add_unique(ElevatorRooms(RoomDatabase {
            rooms: vec![Room {
                obj_id: 1,
                room_id: 1,
                center: vec3(0.0, 0.0, 0.0),
                portals: vec![],
                bounding_box: Aabb3::new(Point3::new(-2.0, -2.0, -2.0), Point3::new(2.0, 2.0, 2.0)),
                planes: [
                    vec3(1.0, 0.0, 0.0),
                    vec3(-1.0, 0.0, 0.0),
                    vec3(0.0, 1.0, 0.0),
                    vec3(0.0, -1.0, 0.0),
                    vec3(0.0, 0.0, 1.0),
                    vec3(0.0, 0.0, -1.0),
                ]
                .into_iter()
                .map(|n| Plane::new(n, 2.0 * SCALE_FACTOR))
                .collect(),
            }],
        }));
        world
    }

    #[test]
    fn only_eligible_loose_objects_in_the_departure_room_travel() {
        let mut world = fixture();
        let cargo = world.add_entity((PropElevAble(true), position(1.0)));
        world.add_entity((PropElevAble(false), position(1.0)));
        world.add_entity((position(1.0),));
        world.add_entity((PropElevAble(true), position(3.0)));
        let held = world.add_entity((PropElevAble(true), position(1.0)));
        let loot = world.add_entity((PropElevAble(true), position(1.0)));
        world.add_entity((
            Links {
                to_links: vec![ToLink {
                    to_template_id: -1,
                    to_entity_id: Some(WrappedEntityId(loot)),
                    link: Link::Contains(0),
                }],
            },
            position(3.0),
        ));
        let selected = ElevatorCargo::collect(&world, 22, &HashSet::from([held.inner()])).unwrap();
        assert_eq!(
            selected.offsets,
            HashMap::from([(cargo.inner(), vec3(1.0, 0.0, 0.0))])
        );
        assert!(ElevatorCargo::collect(&world, 99, &HashSet::new()).is_none());
    }

    #[test]
    fn arrival_uses_exact_start_marker_and_preserves_offset_and_rotation() {
        let mut world = World::new();
        world.add_entity((PropStartLoc(21), position(-30.0)));
        world.add_entity((PropStartLoc(22), position(30.0)));
        let old = EntityId::new_from_index_and_gen(400, 2);
        let new = world.add_entity((position(1.0),));
        let cargo = ElevatorCargo {
            marker: 22,
            offsets: HashMap::from([(old.inner(), vec3(1.0, 0.0, 0.0))]),
        };
        cargo.place(&mut world, &HashMap::from([(old, new)]));
        let positions = world.borrow::<View<PropPosition>>().unwrap();
        let actual = positions.get(new).unwrap();
        assert_eq!(actual.position, vec3(31.0, 0.5 / SCALE_FACTOR, 0.0));
        assert_eq!(actual.rotation, position(1.0).rotation);
    }

    #[test]
    fn cargo_and_nested_contents_leave_source_snapshot_and_round_trip() {
        use crate::mission::{GlobalTemplateIdMap, PlayerInfo};
        use crate::save_load::{HeldItemSaveData, to_save_data, to_transition_save_data};
        use dark::properties::PropStackCount;

        let mut world = fixture();
        let inventory = world.add_entity(());
        let player = world.add_entity((crate::runtime_props::RuntimePropDoNotSerialize,));
        world.add_unique(PlayerInfo {
            pos: vec3(10.0, 0.0, 0.0),
            rotation: position(0.0).rotation,
            entity_id: player,
            left_hand_entity_id: None,
            right_hand_entity_id: None,
            inventory_entity_id: inventory,
        });
        world.add_unique(GlobalTemplateIdMap(HashMap::new()));
        let cargo = world.add_entity((PropElevAble(true), position(1.0)));
        let outside = world.add_entity((PropElevAble(true), position(3.0)));
        let saved_throw: crate::throwing::SavedThrow = serde_json::from_value(serde_json::json!({
            "mass": 1.0, "remaining": 0.0, "armed": false,
            "motion": { "linear": vec3(0.0, 0.0, 0.0), "angular": vec3(0.0, 1.0, 0.0) }
        }))
        .unwrap();
        world.add_unique(crate::throwing::SavedThrows(HashMap::from([
            (cargo.inner(), saved_throw),
            (outside.inner(), saved_throw),
        ])));
        let mut parent = cargo;
        let mut nested = vec![cargo];
        for _ in 0..4 {
            // Non-ElevAble contents outside the room still travel with their container.
            let child = world.add_entity((PropStackCount(7), position(10.0)));
            world.add_component(
                parent,
                Links {
                    to_links: vec![ToLink {
                        to_template_id: -1,
                        to_entity_id: Some(WrappedEntityId(child)),
                        link: Link::Contains(0),
                    }],
                },
            );
            nested.push(child);
            parent = child;
        }
        let (ordinary, held) = to_save_data(&world);
        assert!(held.elevator_cargo.is_none());
        assert!(
            nested
                .iter()
                .all(|id| ordinary.all_entities.contains(&id.inner()))
        );

        let (source, transferred) = to_transition_save_data(&world, None, Some(22));
        assert!(source.all_entities.contains(&outside.inner()));
        assert_eq!(source.thrown_props.len(), 1);
        assert!(source.thrown_props.contains_key(&outside.inner()));
        assert_eq!(transferred.held_entities.thrown_props.len(), 1);
        assert_eq!(
            serde_json::to_value(transferred.held_entities.thrown_props[&cargo.inner()]).unwrap(),
            serde_json::to_value(saved_throw).unwrap()
        );
        for id in &nested {
            assert!(!source.all_entities.contains(&id.inner()));
            assert!(transferred.held_entities.all_entities.contains(&id.inner()));
        }
        let decoded: HeldItemSaveData =
            serde_json::from_value(serde_json::to_value(transferred).unwrap()).unwrap();
        let mut destination = World::new();
        destination.add_entity((PropStartLoc(22), position(30.0)));
        let instantiated =
            decoded.instantiate_with_legacy_template_names(&mut destination, &HashMap::new());
        let links = destination.borrow::<View<Links>>().unwrap();
        let stacks = destination.borrow::<View<PropStackCount>>().unwrap();
        for pair in nested.windows(2) {
            let parent = instantiated.entity_id_map[&pair[0]];
            let child = instantiated.entity_id_map[&pair[1]];
            assert_eq!(
                links.get(parent).unwrap().to_links[0]
                    .to_entity_id
                    .map(|id| id.0),
                Some(child)
            );
            assert_eq!(stacks.get(child).unwrap().0, 7);
        }
    }
}
