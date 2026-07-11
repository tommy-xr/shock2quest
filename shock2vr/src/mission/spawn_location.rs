use std::collections::HashMap;

use cgmath::{Quaternion, Vector3};
use dark::{
    properties::{Link, PropPosition, PropStartLoc, WrappedEntityId},
    ss2_entity_info::SystemShock2EntityInfo,
};
use num_traits::Zero;
use shipyard::{Get, IntoIter, IntoWithId, View, World};

use crate::scripts::script_util::{get_all_links_of_type, get_first_link_of_type};

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
                world.run(
                    |v_position: View<PropPosition>, v_start_loc: View<PropStartLoc>| {
                        // The best StartLoc marker that links to a LandingPoint (the
                        // intended path when the retail data provides one).
                        let mut landing_spawn_entity_id = None;
                        let mut landing_delta = u32::MAX;
                        // Fallback: the best StartLoc marker's OWN position. Some
                        // retail StartLoc markers have NO LandingPoint link at all
                        // (e.g. station.mis obj 132 "Starting_Location", the
                        // post-career recruit-deck spawn at loc 2501). Without this
                        // fallback such markers resolve to the world origin - see #454.
                        let mut marker_spawn: Option<(Vector3<f32>, Quaternion<f32>)> = None;
                        let mut marker_delta = u32::MAX;
                        for (entity_id, start_loc) in (&v_start_loc).iter().with_id() {
                            // HACK: Find the location that matches the best...
                            // aka, why is the eng dest loc 21 but the medsci 12??
                            // Ideally, this should be an exact match, but not sure why the spawn points are off in some cases...
                            let diff = start_loc.0.abs_diff(*loc);

                            if diff < marker_delta {
                                if let Ok(marker_pos) = v_position.get(entity_id) {
                                    marker_delta = diff;
                                    marker_spawn = Some((marker_pos.position, marker_pos.rotation));
                                }
                            }

                            // Prefer markers that actually link to a landing point.
                            let all_links =
                                get_all_links_of_type(world, entity_id, Link::LandingPoint);

                            if diff < landing_delta && !all_links.is_empty() {
                                landing_delta = diff;
                                landing_spawn_entity_id =
                                    get_first_link_of_type(world, entity_id, Link::LandingPoint);
                            }
                        }

                        // Landing-point link wins when present; otherwise fall back to
                        // the matched marker's own position (never the world origin).
                        if let Some(entity_id) = landing_spawn_entity_id {
                            let spawn_pos = v_position.get(entity_id).unwrap();
                            start_pos = spawn_pos.position;
                            start_rotation = spawn_pos.rotation;
                        } else if let Some((pos, rot)) = marker_spawn {
                            start_pos = pos;
                            start_rotation = rot;
                        }
                    },
                );
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
}
