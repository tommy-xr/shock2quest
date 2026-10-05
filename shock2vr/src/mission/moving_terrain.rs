//! Dark's native MovingTerrain service (`physics/phmterr.cpp`). Its route and
//! pause state live in the registered TPathNext/TPath links, so native movers
//! resume from a save without object scripts or a second serialized clock.

use std::collections::HashMap;

use cgmath::InnerSpace;
use dark::properties::{Link, Links, PropMovingTerrain, PropPosition, PropScripts, ToLink};
use shipyard::{EntityId, Get, IntoIter, IntoWithId, View, ViewMut, World};

use crate::time::Time;

fn script_owns_motion(scripts: Option<&PropScripts>) -> bool {
    scripts.is_some_and(|scripts| {
        scripts.scripts.iter().any(|name| {
            name.eq_ignore_ascii_case("BaseElevator")
                || name.eq_ignore_ascii_case("DontStopElevator")
        })
    })
}

fn destination(link: &ToLink) -> Option<EntityId> {
    link.to_entity_id.as_ref().map(|id| id.0)
}

fn path_data(link: &ToLink) -> dark::properties::TPathData {
    match link.link {
        Link::TPath(data) => data,
        _ => unreachable!("only path edges enter the route map"),
    }
}

fn reset_pause(link: &mut ToLink) {
    if let Link::TPath(data) = &mut link.link {
        data.current_pause_ms = if data.pause_ms > 0 { 0 } else { -1 };
    }
}

/// Return the next kinematic poses. Script-controlled elevators retain their
/// existing sole motion owner; this service drives property-controlled terrain.
/// The caller applies these targets before the physics step, which publishes
/// the resulting PropPosition/RuntimePropTransform for rendering and saving.
pub(super) fn update(
    world: &World,
    time: &Time,
    is_physical: impl Fn(EntityId) -> bool,
) -> Vec<(EntityId, PropPosition)> {
    if time.elapsed.is_zero() {
        return Vec::new();
    }
    let (positions, scripts, mut terrain, mut links) = world
        .borrow::<(
            View<PropPosition>,
            View<PropScripts>,
            ViewMut<PropMovingTerrain>,
            ViewMut<Links>,
        )>()
        .unwrap();
    let mut edges = HashMap::new();
    let mut incoming = HashMap::new();
    for (source, node_links) in (&links).iter().with_id() {
        if let Some(edge) = node_links
            .to_links
            .iter()
            .find(|link| matches!(link.link, Link::TPath(_)) && destination(link).is_some())
        {
            edges.insert(source, edge.clone());
            incoming.entry(destination(edge).unwrap()).or_insert(source);
        }
    }
    // Use the simulation clock's millisecond boundaries, rather than truncating
    // 16.666 ms to 16 on every fixed frame and stretching authored pauses.
    let elapsed_ms = time
        .total
        .as_millis()
        .saturating_sub(time.total.saturating_sub(time.elapsed).as_millis())
        .min(i32::MAX as u128) as i32;
    let mut poses = Vec::new();
    for (entity, property) in (&mut terrain).iter().with_id() {
        if !is_physical(entity) || script_owns_motion(scripts.get(entity).ok()) {
            continue;
        }
        let activating = property.active && !property.previous_active;
        property.previous_active = property.active;
        if !property.active {
            continue;
        }
        let Ok(mut pose) = positions.get(entity).cloned() else {
            continue;
        };
        let Ok(entity_links) = (&mut links).get(entity) else {
            continue;
        };
        let next = entity_links
            .to_links
            .iter()
            .find(|link| matches!(link.link, Link::TPathNext))
            .cloned();
        let mut next = if let Some(next) = next {
            next
        } else {
            let Some(initial) = entity_links
                .to_links
                .iter()
                .find(|link| matches!(link.link, Link::TPathInit))
            else {
                continue;
            };
            let Some(initial_id) = destination(initial) else {
                continue;
            };
            let Ok(initial_pose) = positions.get(initial_id) else {
                continue;
            };
            // Native activation without a TPathNext starts at the initial node.
            pose = initial_pose.clone();
            let Some(edge) = edges.get_mut(&initial_id) else {
                continue;
            };
            reset_pause(edge);
            edge.clone()
        };
        next.link = Link::TPathNext;
        let Some(mut target) = destination(&next) else {
            continue;
        };
        let Some(mut source) = incoming.get(&target).copied() else {
            continue;
        };
        if activating {
            reset_pause(edges.get_mut(&source).unwrap());
        }
        if let Link::TPath(data) = &mut edges.get_mut(&source).unwrap().link {
            if data.current_pause_ms >= 0 {
                data.current_pause_ms = data.current_pause_ms.saturating_add(elapsed_ms);
                if data.current_pause_ms < data.pause_ms {
                    // Persist the selected edge even if initialization begins
                    // in a pause, and place the body at its initial waypoint.
                    replace_next(entity_links, next);
                    poses.push((entity, pose));
                    continue;
                }
                data.current_pause_ms = -1;
            }
        }
        let mut remaining = time.elapsed.as_secs_f32();
        // A malformed zero-length cycle must not hang the mission update.
        for _ in 0..128 {
            let data = path_data(&edges[&source]);
            let Ok(target_pose) = positions.get(target) else {
                break;
            };
            if !data.speed.is_finite() || data.speed <= 0.0 {
                break;
            }
            let direction = target_pose.position - pose.position;
            let distance = direction.magnitude();
            if distance > data.speed * remaining {
                pose.position += direction * (data.speed * remaining / distance);
                break;
            }
            pose.position = target_pose.position;
            remaining = (remaining - distance / data.speed).max(0.0);
            let Some(edge) = edges.get_mut(&target) else {
                // Keep the terminal target as the saved stopped state. Removing
                // it would make a reload indistinguishable from first activation.
                break;
            };
            reset_pause(edge);
            next = edge.clone();
            next.link = Link::TPathNext;
            source = target;
            target = destination(&next).unwrap();
            let next_data = path_data(edge);
            if remaining <= 0.0 || next_data.path_limit || next_data.pause_ms > 0 {
                break;
            }
        }
        replace_next(entity_links, next);
        poses.push((entity, pose));
    }
    // The original stores the timer on the path edge, not on its moving body.
    for (source, edge) in edges {
        if let Ok(node_links) = (&mut links).get(source) {
            if let Some(stored) = node_links
                .to_links
                .iter_mut()
                .find(|link| matches!(link.link, Link::TPath(_)))
            {
                *stored = edge;
            }
        }
    }
    poses
}

fn replace_next(links: &mut Links, next: ToLink) {
    links
        .to_links
        .retain(|link| !matches!(link.link, Link::TPathNext));
    links.to_links.push(next);
}

#[cfg(test)]
mod tests {
    use super::*;
    use cgmath::{Quaternion, vec3};
    use dark::properties::{TPathData, WrappedEntityId};
    use std::time::Duration;

    fn pose(x: f32) -> PropPosition {
        PropPosition {
            position: vec3(x, 0.0, 0.0),
            rotation: Quaternion::new(1.0, 0.0, 0.0, 0.0),
            cell: 0,
        }
    }
    fn link(target: EntityId, kind: Link) -> ToLink {
        ToLink {
            to_template_id: 0,
            to_entity_id: Some(WrappedEntityId(target)),
            link: kind,
        }
    }
    fn route() -> (World, EntityId, EntityId, EntityId) {
        let mut world = World::new();
        let first = world.add_entity(pose(0.0));
        let second = world.add_entity(pose(2.0));
        world.add_component(
            first,
            Links {
                to_links: vec![link(
                    second,
                    Link::TPath(TPathData {
                        speed: 2.0,
                        current_pause_ms: -1,
                        ..Default::default()
                    }),
                )],
            },
        );
        world.add_component(
            second,
            Links {
                to_links: vec![link(
                    first,
                    Link::TPath(TPathData {
                        speed: 1.0,
                        current_pause_ms: -1,
                        ..Default::default()
                    }),
                )],
            },
        );
        let terrain = world.add_entity((
            pose(0.0),
            PropMovingTerrain {
                active: true,
                previous_active: true,
            },
            Links {
                to_links: vec![link(first, Link::TPathInit), link(second, Link::TPathNext)],
            },
        ));
        (world, terrain, first, second)
    }
    fn step(world: &mut World, seconds: f64) {
        let duration = Duration::from_secs_f64(seconds);
        for (entity, position) in update(
            world,
            &Time {
                elapsed: duration,
                total: duration,
            },
            |_| true,
        ) {
            world.add_component(entity, position);
        }
    }
    fn x(world: &World, entity: EntityId) -> f32 {
        world
            .borrow::<View<PropPosition>>()
            .unwrap()
            .get(entity)
            .unwrap()
            .position
            .x
    }
    fn edit_edge(world: &World, node: EntityId, edit: impl FnOnce(&mut TPathData)) {
        let mut links = world.borrow::<ViewMut<Links>>().unwrap();
        let Link::TPath(data) = &mut (&mut links).get(node).unwrap().to_links[0].link else {
            panic!()
        };
        edit(data);
    }

    #[test]
    fn cyclic_paths_use_each_edge_speed_and_consume_nonlimited_overshoot() {
        let (mut world, terrain, _, _) = route();
        step(&mut world, 1.5);
        assert!((x(&world, terrain) - 1.5).abs() < 1e-5);
        step(&mut world, 1.75);
        assert!((x(&world, terrain) - 0.5).abs() < 1e-5);
    }

    #[test]
    fn authored_pause_is_waited_and_hard_limits_stop_the_arrival_step() {
        let (mut world, terrain, first, second) = route();
        edit_edge(&world, first, |data| {
            data.pause_ms = 500;
            data.current_pause_ms = 0;
        });
        step(&mut world, 0.25);
        assert_eq!(x(&world, terrain), 0.0);
        step(&mut world, 0.25);
        assert_eq!(x(&world, terrain), 0.5);
        edit_edge(&world, second, |data| data.path_limit = true);
        step(&mut world, 1.0);
        assert_eq!(x(&world, terrain), 2.0);
        step(&mut world, 0.25);
        assert_eq!(x(&world, terrain), 1.75);
    }

    #[test]
    fn inactive_and_script_owned_terrain_do_not_integrate_twice() {
        for owner in [None, Some("BaseElevator"), Some("dontstopelevator")] {
            let (mut world, terrain, _, _) = route();
            if let Some(owner) = owner {
                world.add_component(
                    terrain,
                    PropScripts {
                        scripts: vec![owner.into()],
                        inherits: false,
                    },
                );
            } else {
                world.add_component(
                    terrain,
                    PropMovingTerrain {
                        active: false,
                        previous_active: true,
                    },
                );
            }
            step(&mut world, 1.0);
            assert_eq!(x(&world, terrain), 0.0);
        }
    }

    #[test]
    fn initialization_uses_the_initial_node_and_terminal_routes_stay_stopped() {
        let (mut world, terrain, first, second) = route();
        world.add_component(
            terrain,
            (
                pose(-10.0),
                Links {
                    to_links: vec![link(first, Link::TPathInit)],
                },
            ),
        );
        world.add_component(second, Links::empty());
        step(&mut world, 0.25);
        assert_eq!(x(&world, terrain), 0.5);
        step(&mut world, 1.0);
        assert_eq!(x(&world, terrain), 2.0);
        step(&mut world, 10.0);
        assert_eq!(x(&world, terrain), 2.0);
    }

    #[test]
    fn paused_simulation_does_not_mutate_route_or_pose() {
        let (mut world, terrain, _, _) = route();
        let before =
            serde_json::to_value(world.borrow::<View<Links>>().unwrap().get(terrain).unwrap())
                .unwrap();
        step(&mut world, 0.0);
        let after =
            serde_json::to_value(world.borrow::<View<Links>>().unwrap().get(terrain).unwrap())
                .unwrap();
        assert_eq!(before, after);
        assert_eq!(x(&world, terrain), 0.0);
    }
}
