use crate::{
    SCALE_FACTOR,
    ss2_common::{read_i16, read_i32, read_plane, read_single, read_u32, read_vec3},
};

use cgmath::{Point3, Vector3};
use collision::{Aabb3, Continuous, Plane};

use std::io;

#[derive(Debug, Clone)]
pub struct Room {
    pub obj_id: i32,
    pub room_id: i16,
    pub center: Vector3<f32>,
    pub planes: Vec<Plane<f32>>,
    pub portals: Vec<RoomPortal>,
    pub bounding_box: Aabb3<f32>,
}

impl Room {
    // Read the ROOM_DB chunk to get a list of rooms
    pub fn read<T: io::Read + io::Seek>(reader: &mut T) -> Room {
        let obj_id = read_i32(reader);
        let room_id = read_i16(reader);

        let center = read_vec3(reader) / SCALE_FACTOR;

        let mut planes = Vec::new();

        for _ in 0..6 {
            let plane = read_plane(reader);

            planes.push(Plane {
                n: plane.n,
                d: -plane.d,
            })
        }

        let portal_count = read_u32(reader);

        // TODO: https://github.com/Kernvirus/SystemShock2VR/blob/5f0f7d054e79c2e36d9661f4ca62ab95ae69de0b/Assets/Scripts/Editor/DarkEngine/Rooms/Room.cs

        let mut portals = Vec::new();
        for _ in 0..portal_count {
            portals.push(RoomPortal::read(reader));
        }

        let bounding_box = bounding_box_from_planes(&planes);

        let portal_distance_count = portal_count * portal_count;

        let mut portal_distances = Vec::new();
        for _ in 0..portal_distance_count {
            portal_distances.push(read_single(reader));
        }

        let num_lists = read_u32(reader);

        for _ in 0..num_lists {
            let count = read_u32(reader);

            for _0 in 0..count {
                let _id = read_i32(reader);
            }
        }

        Room {
            obj_id,
            room_id,
            center,
            planes,
            portals,
            bounding_box,
        }
    }
}

#[derive(Debug, Clone)]
pub struct RoomPortal {
    pub id: i32,
    pub index: u32,
    pub plane: Plane<f32>,
    pub src_room: i32,
    pub dest_room: i32,
    pub center: Vector3<f32>,
    pub dest_portal: i32,
}

impl RoomPortal {
    pub fn read<T: io::Seek + io::Read>(reader: &mut T) -> RoomPortal {
        let id = read_i32(reader);
        let index = read_u32(reader);
        let plane = read_plane(reader);
        let edge_count = read_u32(reader);

        //let mut _edges = Vec::new();
        for _ in 0..edge_count {
            //   edges.push(read_plane(reader))
            let _0 = read_plane(reader);
        }

        let src_room = read_i32(reader);
        let dest_room = read_i32(reader);

        let center = read_vec3(reader);
        let dest_portal = read_i32(reader);
        RoomPortal {
            id,
            index,
            plane,
            src_room,
            dest_room,
            center,
            dest_portal,
        }
    }
}

// Define a function to compute the bounding box from a set of 6 planes.
fn bounding_box_from_planes(planes: &Vec<Plane<f32>>) -> Aabb3<f32> {
    // Initialize the min and max corners of the bounding box.
    let mut min_corner = Point3::new(std::f32::INFINITY, std::f32::INFINITY, std::f32::INFINITY);
    let mut max_corner = Point3::new(
        std::f32::NEG_INFINITY,
        std::f32::NEG_INFINITY,
        std::f32::NEG_INFINITY,
    );

    // Loop over each plane and compute its intersection with the other planes.
    let len = planes.len();
    for i in 0..len {
        let plane_i = planes[i];

        // Compute the intersection of plane i with the other planes.
        for j in (i + 1)..len {
            let plane_j = planes[j];

            for k in (j + 1)..len {
                let plane_k = planes[k];

                if let Some(point) = plane_i.intersection(&(plane_j, plane_k)) {
                    min_corner.x = min_corner.x.min(point.x);
                    min_corner.y = min_corner.y.min(point.y);
                    min_corner.z = min_corner.z.min(point.z);
                    max_corner.x = max_corner.x.max(point.x);
                    max_corner.y = max_corner.y.max(point.y);
                    max_corner.z = max_corner.z.max(point.z);
                }
            }
        }
    }

    Aabb3::new(min_corner / SCALE_FACTOR, max_corner / SCALE_FACTOR)
}
