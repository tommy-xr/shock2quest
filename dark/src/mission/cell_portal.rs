use cgmath::{Matrix4, Point3, Vector2, Vector3, Vector4, point2};
use collision::{Aabb2, Sphere};

use crate::util::compute_bounding_sphere;

///
/// CellPortal
///
/// A cell portal is a portal between two cells. It defines geometry that connects two cells.
#[derive(Debug, Clone)]
pub struct CellPortal {
    pub target_cell_idx: u16,
    pub all_vertices: Vec<Point3<f32>>,
    pub bounding_sphere: Sphere<f32>,
}

impl CellPortal {
    pub fn new(vertices: Vec<Point3<f32>>, target_cell_id: u16) -> CellPortal {
        let bounding_sphere = compute_bounding_sphere(&vertices);
        CellPortal {
            target_cell_idx: target_cell_id,
            all_vertices: vertices,
            bounding_sphere,
        }
    }

    pub fn screen_space_squad(
        &self,
        projection_view: Matrix4<f32>,
        screen_width: f32,
        screen_height: f32,
    ) -> Aabb2<f32> {
        let mut min_x = f32::MAX;
        let mut min_y = f32::MAX;
        let mut max_x = f32::MIN;
        let mut max_y = f32::MIN;

        let mut in_back_of_camera_count = 0;
        let mut in_front_of_camera_count = 0;
        for vertex in &self.all_vertices {
            let homogenous_position = Vector4::new(vertex.x, vertex.y, vertex.z, 1.0);

            // Multiply by view and projection matrices
            let clip_space_position = projection_view * homogenous_position;

            // HACK: If the vertex is behind the camera, just return the whole screen
            if clip_space_position.z <= 0.1 {
                in_back_of_camera_count += 1;
            } else {
                in_front_of_camera_count += 1;
            }

            // Perspective divide
            let normalized_device_coordinates = Vector3::new(
                clip_space_position.x / clip_space_position.w,
                clip_space_position.y / clip_space_position.w,
                clip_space_position.z / clip_space_position.w,
            );

            // Map to screen-space
            let screen_space_vertex = Vector2::new(
                (normalized_device_coordinates.x * 0.5 + 0.5) * screen_width,
                (1.0 - (normalized_device_coordinates.y * 0.5 + 0.5)) * screen_height,
            );

            min_x = min_x.min(screen_space_vertex.x);
            min_y = min_y.min(screen_space_vertex.y);
            max_x = max_x.max(screen_space_vertex.x);
            max_y = max_y.max(screen_space_vertex.y);
        }

        // Portal is totally behind camera, ignore
        if in_back_of_camera_count > 0 && in_front_of_camera_count == 0 {
            Aabb2 {
                min: point2(0.0, 0.0),
                max: point2(0.0, 0.0),
            }
        // Portal spans front & back of camera...
        // HACK: Default to screen size. We don't handle this case yet.
        // A better solution is to clip the edge of the triangle on the near plane, to get
        // a proper screen space bounding box.
        } else if in_back_of_camera_count > 0 && in_front_of_camera_count > 0 {
            Aabb2 {
                min: point2(0.0, 0.0),
                max: point2(screen_width, screen_height),
            }
        } else {
            Aabb2 {
                min: point2(min_x, min_y),
                max: point2(max_x, max_y),
            }
        }
    }
}
