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
                        let mut spawn_entity_id = None;
                        let mut closest_delta = u32::MAX;
                        for (entity_id, start_loc) in (&v_start_loc).iter().with_id() {
                            // HACK: Find the location that matches the best...
                            // aka, why is the eng dest loc 21 but the medsci 12??
                            // Ideally, this should be an exact match, but not sure why the spawn points are off in some cases...
                            let diff = start_loc.0.abs_diff(*loc);

                            // Only consider points that actually are linked to landing points...
                            let all_links =
                                get_all_links_of_type(world, entity_id, Link::LandingPoint);

                            if diff < closest_delta && !all_links.is_empty() {
                                closest_delta = diff;
                                spawn_entity_id =
                                    get_first_link_of_type(world, entity_id, Link::LandingPoint);
                            }
                        }

                        match spawn_entity_id {
                            None => (),
                            Some(entity_id) => {
                                let spawn_pos = v_position.get(entity_id).unwrap();
                                start_pos = spawn_pos.position;
                                start_rotation = spawn_pos.rotation;
                            }
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
