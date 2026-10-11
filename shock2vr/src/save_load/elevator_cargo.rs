use std::collections::{HashMap, HashSet};

use cgmath::{InnerSpace, Vector3};
use dark::{
    SCALE_FACTOR,
    mission::{room::Room, room_database::RoomDatabase},
    properties::{Link, Links, PropDestLoc, PropElevAble, PropPosition, PropStartLoc},
};
use serde::{Deserialize, Serialize};
use shipyard::{EntityId, Get, IntoIter, IntoWithId, Unique, UniqueView, View, World};

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

impl ElevatorCargo {
    /// Retail's ElevatorFixup falls back to the player's arrival point when
    /// translated cargo intersects destination geometry. Cabins differ slightly
    /// between decks; letting the solver eject an embedded item can lose it
    /// through a wall before the player gets control.
    pub(crate) fn recover_blocked_cargo(
        &self,
        world: &mut World,
        physics: &mut crate::physics::PhysicsWorld,
        remap: &HashMap<EntityId, EntityId>,
        arrival: Vector3<f32>,
    ) {
        for old in self.offsets.keys() {
            let Some(&entity) = EntityId::from_inner(*old).and_then(|id| remap.get(&id)) else {
                continue;
            };
            let Some(bounds) = physics.entity_world_aabb(entity) else {
                continue;
            };
            let size = bounds.max - bounds.min;
            let center = Vector3::new(bounds.min.x, bounds.min.y, bounds.min.z) + size * 0.5;
            if physics.placement_box_is_clear(
                center,
                cgmath::Quaternion::new(1.0, 0.0, 0.0, 0.0),
                size,
                &[entity],
            ) {
                continue;
            }
            let mut position = world
                .borrow::<View<PropPosition>>()
                .unwrap()
                .get(entity)
                .unwrap()
                .clone();
            position.position = arrival;
            physics.set_position_rotation2(entity, arrival, position.rotation);
            let state = world
                .borrow::<View<dark::properties::PropPhysState>>()
                .unwrap()
                .get(entity)
                .ok()
                .cloned();
            if let Some(mut state) = state {
                state.position = arrival;
                world.add_component(entity, state);
            }
            world.add_component(entity, position);
        }
    }

    /// Retail shkmulti.cpp TransportCore selects the room of DestLoc, not the
    /// player, and carries only ElevAble roots (plus their contained objects).
    pub(crate) fn collect(world: &World, marker: i32, carried: &HashSet<u64>) -> Option<Self> {
        let rooms = world.borrow::<UniqueView<ElevatorRooms>>().ok()?;
        let positions = world.borrow::<View<PropPosition>>().unwrap();
        let destinations = world.borrow::<View<PropDestLoc>>().unwrap();
        let origin = (&destinations, &positions)
            .iter()
            .find(|(loc, _)| loc.0 == marker)?
            .1
            .position;
        let room = rooms.0.rooms.iter().find(|room| contains(room, origin))?;
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
        let destination = {
            let starts = world.borrow::<View<PropStartLoc>>().unwrap();
            let positions = world.borrow::<View<PropPosition>>().unwrap();
            (&starts, &positions)
                .iter()
                .find(|(loc, _)| loc.0 == self.marker)
                .map(|(_, pos)| pos.position)
        };
        let Some(destination) = destination else {
            tracing::warn!("Elevator destination has no StartLoc {}", self.marker);
            return;
        };
        for (old, offset) in &self.offsets {
            let Some(entity) = EntityId::from_inner(*old).and_then(|id| remap.get(&id)) else {
                continue;
            };
            let position = world
                .borrow::<View<PropPosition>>()
                .unwrap()
                .get(*entity)
                .ok()
                .cloned();
            if let Some(mut position) = position {
                // Retail lifts cargo half a Dark unit to keep settled objects
                // out of the destination floor. Orientation remains unchanged.
                position.position =
                    destination + offset + Vector3::new(0.0, 0.5 / SCALE_FACTOR, 0.0);
                position.cell = 0;
                let state = world
                    .borrow::<View<dark::properties::PropPhysState>>()
                    .unwrap()
                    .get(*entity)
                    .ok()
                    .cloned();
                if let Some(mut state) = state {
                    state.position = position.position;
                    world.add_component(*entity, state);
                }
                world.add_component(*entity, position);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cgmath::{Point3, Quaternion, vec3};
    use collision::{Aabb3, Plane};
    use dark::properties::{ToLink, WrappedEntityId};

    fn position(x: f32) -> PropPosition {
        PropPosition {
            position: vec3(x, 0.0, 0.0),
            cell: 0,
            rotation: Quaternion::new(1.0, 0.0, 0.0, 0.0),
        }
    }

    #[test]
    fn blocked_arrival_recovers_to_player_but_clear_cargo_keeps_its_offset() {
        use crate::physics::{CollisionGroup, DynamicPhysicsOptions, PhysicsShape, PhysicsWorld};
        let mut world = World::new();
        let wall = world.add_entity(());
        let blocked = world.add_entity(position(0.0));
        let clear = world.add_entity(position(3.0));
        let mut physics = PhysicsWorld::new();
        let rotation = Quaternion::new(1.0, 0.0, 0.0, 0.0);
        physics.add_kinematic_shape(
            wall,
            vec3(0.0, 0.0, 0.0),
            rotation,
            vec3(0.0, 0.0, 0.0),
            PhysicsShape::Cuboid(vec3(1.0, 1.0, 1.0)),
            CollisionGroup::entity(),
            false,
        );
        for (entity, x) in [(blocked, 0.0), (clear, 3.0)] {
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
        }
        let cargo = ElevatorCargo {
            marker: 22,
            offsets: HashMap::from([
                (blocked.inner(), vec3(0.0, 0.0, 0.0)),
                (clear.inner(), vec3(3.0, 0.0, 0.0)),
            ]),
        };
        let arrival = vec3(5.0, 0.0, 0.0);
        cargo.recover_blocked_cargo(
            &mut world,
            &mut physics,
            &HashMap::from([(blocked, blocked), (clear, clear)]),
            arrival,
        );
        let positions = world.borrow::<View<PropPosition>>().unwrap();
        assert_eq!(positions.get(blocked).unwrap().position, arrival);
        assert_eq!(positions.get(clear).unwrap().position, vec3(3.0, 0.0, 0.0));
        assert_eq!(
            physics
                .entity_body_pose(blocked)
                .unwrap()
                .translation
                .vector
                .x,
            5.0
        );
    }

    fn fixture() -> World {
        let mut world = World::new();
        world.add_entity((PropDestLoc(22), position(0.0)));
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
