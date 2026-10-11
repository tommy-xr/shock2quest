use std::collections::HashMap;

use cgmath::{Quaternion, Vector3};
use dark::{
    properties::{Link, PropPosition, PropStartLoc, WrappedEntityId},
    ss2_entity_info::SystemShock2EntityInfo,
};
use num_traits::Zero;
use shipyard::{EntityId, Get, IntoIter, IntoWithId, View, World};

use crate::scripts::script_util::get_first_link_of_type;

#[derive(Clone)]
pub enum SpawnLocation {
    MapDefault,
    Marker(i32),
    PositionRotation(Vector3<f32>, Quaternion<f32>),
}

impl SpawnLocation {
    pub fn calculate_start_position(
        &self,
        world: &World,
        entity_info: &SystemShock2EntityInfo,
        template_to_entity_id: &HashMap<i32, WrappedEntityId>,
    ) -> (Vector3<f32>, Quaternion<f32>) {
        let mut start_pos = Vector3::zero();
        let mut start_rotation = Quaternion {
            v: Vector3::zero(),
            s: 1.0,
        };

        match self {
            Self::PositionRotation(position, rotation) => {
                start_pos = *position;
                start_rotation = *rotation;
            }
            Self::Marker(loc) => {
                if let Some(marker) = find_start_marker(world, *loc, false) {
                    (start_pos, start_rotation) = marker.spawn;
                }
            }
            Self::MapDefault => {
                world.run(|v_position: View<PropPosition>| {
                    for link in &entity_info.link_playerfactories {
                        if let Some(entity_id) = template_to_entity_id.get(&link.src) {
                            let pos = v_position.get(entity_id.0).unwrap();
                            start_pos = pos.position;
                            start_rotation = pos.rotation;
                        }
                    }
                });
            }
        };
        (start_pos, start_rotation)
    }
}

/// A resolved `PropStartLoc` marker.
pub struct StartMarker {
    /// The marker's own position.
    pub position: Vector3<f32>,
    /// Where the player lands: the linked LandingPoint, else the marker itself.
    /// Some retail markers have no LandingPoint (e.g. station.mis obj 132, the
    /// post-career recruit-deck spawn at loc 2501) - see #454.
    pub spawn: (Vector3<f32>, Quaternion<f32>),
}

/// The StartLoc marker for `loc`: one linked to a LandingPoint wins, then the
/// closest value (only `loc` itself when `exact`). Decks can author unlinked
/// copies of a marker (ops2 has five StartLoc 22s), so the link picks the one
/// the player actually spawns from.
pub fn find_start_marker(world: &World, loc: i32, exact: bool) -> Option<StartMarker> {
    let v_position = world.borrow::<View<PropPosition>>().unwrap();
    let v_start_loc = world.borrow::<View<PropStartLoc>>().unwrap();
    let mut best: Option<((bool, u32), Option<EntityId>, PropPosition)> = None;
    for (entity_id, start_loc) in v_start_loc.iter().with_id() {
        if exact && start_loc.0 != loc {
            continue;
        }
        let Ok(position) = v_position.get(entity_id) else {
            continue;
        };
        // HACK: closest rather than exact - why is the eng dest loc 21 but
        // the medsci 12? Ideally this would be an exact match.
        let landing = get_first_link_of_type(world, entity_id, Link::LandingPoint);
        let rank = (landing.is_none(), start_loc.0.abs_diff(loc));
        if best
            .as_ref()
            .is_none_or(|(best_rank, ..)| rank < *best_rank)
        {
            best = Some((rank, landing, position.clone()));
        }
    }
    let (_, landing, marker) = best?;
    let spawn = landing
        .and_then(|id| v_position.get(id).ok())
        .unwrap_or(&marker);
    Some(StartMarker {
        position: marker.position,
        spawn: (spawn.position, spawn.rotation),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use dark::properties::{Links, ToLink};

    fn quat_identity() -> Quaternion<f32> {
        Quaternion {
            v: Vector3::zero(),
            s: 1.0,
        }
    }

    fn position(pos: Vector3<f32>) -> PropPosition {
        PropPosition {
            position: pos,
            cell: 0,
            rotation: quat_identity(),
        }
    }

    /// A StartLoc marker with NO LandingPoint link must spawn at the marker's
    /// own position, not the world origin (regression test for #454).
    #[test]
    fn marker_without_landing_point_falls_back_to_marker_position() {
        let mut world = World::new();
        let marker_pos = Vector3::new(81.78, -3.6, 16.54);
        world.add_entity((PropStartLoc(2501), position(marker_pos)));

        let (start_pos, _) = SpawnLocation::Marker(2501).calculate_start_position(
            &world,
            &SystemShock2EntityInfo::empty(),
            &HashMap::new(),
        );

        assert_eq!(start_pos, marker_pos);
    }

    /// When a StartLoc marker DOES link to a LandingPoint, that linked point's
    /// position wins over the marker's own position.
    #[test]
    fn marker_with_landing_point_prefers_linked_position() {
        let mut world = World::new();
        let marker_pos = Vector3::new(1.0, 0.0, 1.0);
        let landing_pos = Vector3::new(50.0, 5.0, 60.0);

        let landing_entity = world.add_entity((position(landing_pos),));
        world.add_entity((
            PropStartLoc(2501),
            position(marker_pos),
            Links {
                to_links: vec![ToLink {
                    to_template_id: 0,
                    to_entity_id: Some(WrappedEntityId(landing_entity)),
                    link: Link::LandingPoint,
                }],
            },
        ));

        let (start_pos, _) = SpawnLocation::Marker(2501).calculate_start_position(
            &world,
            &SystemShock2EntityInfo::empty(),
            &HashMap::new(),
        );

        assert_eq!(start_pos, landing_pos);
    }

    /// Decks can author several StartLocs with the same value (ops2 has five
    /// 22s); the one linked to a LandingPoint is the marker, whatever the order.
    #[test]
    fn landing_linked_marker_wins_over_unlinked_duplicates() {
        let mut world = World::new();
        world.add_entity((PropStartLoc(22), position(Vector3::new(-3.0, 0.0, 0.0))));
        let landing = world.add_entity((position(Vector3::new(9.0, 0.0, 0.0)),));
        let marker_pos = Vector3::new(1.0, 0.0, 0.0);
        world.add_entity((
            PropStartLoc(22),
            position(marker_pos),
            Links {
                to_links: vec![ToLink {
                    to_template_id: 0,
                    to_entity_id: Some(WrappedEntityId(landing)),
                    link: Link::LandingPoint,
                }],
            },
        ));
        world.add_entity((PropStartLoc(22), position(Vector3::new(3.0, 0.0, 0.0))));

        let marker = find_start_marker(&world, 22, false).unwrap();
        assert_eq!(marker.position, marker_pos);
        assert_eq!(marker.spawn.0, Vector3::new(9.0, 0.0, 0.0));
    }

    /// Elevator cargo needs the exact marker even when a linked one with a
    /// different value would win a player spawn.
    #[test]
    fn exact_lookup_ignores_linked_markers_of_other_values() {
        let mut world = World::new();
        let landing = world.add_entity((position(Vector3::new(9.0, 0.0, 0.0)),));
        world.add_entity((
            PropStartLoc(21),
            position(Vector3::new(-3.0, 0.0, 0.0)),
            Links {
                to_links: vec![ToLink {
                    to_template_id: 0,
                    to_entity_id: Some(WrappedEntityId(landing)),
                    link: Link::LandingPoint,
                }],
            },
        ));
        let exact_pos = Vector3::new(1.0, 0.0, 0.0);
        world.add_entity((PropStartLoc(22), position(exact_pos)));

        assert_eq!(
            find_start_marker(&world, 22, false).unwrap().position.x,
            -3.0
        );
        assert_eq!(
            find_start_marker(&world, 22, true).unwrap().position,
            exact_pos
        );
        assert!(find_start_marker(&world, 23, true).is_none());
    }
}
