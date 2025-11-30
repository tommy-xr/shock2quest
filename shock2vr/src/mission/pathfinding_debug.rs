use crate::creature::HUMAN_HEIGHT;
/// Pathfinding debug visualization for AI navigation mesh
use cgmath::Vector3;
use dark::mission::PathDatabase;
use engine::scene::{SceneObject, VertexPosition, color_material, lines_mesh};

/// Height offset for pathfinding visualization - center of human height
const PATH_NODE_HEIGHT: f32 = HUMAN_HEIGHT / 2.0;

/// Renders pathfinding visualization as scene objects
///
/// Creates cyan lines for navigation cells and polygons, and yellow lines for cell links.
/// This provides a visual overlay of the AI navigation mesh for debugging purposes.
pub fn render_pathfinding_debug(path_database: &PathDatabase) -> Vec<SceneObject> {
    let mut visuals = Vec::new();
    let mut cyan_lines = Vec::new();
    let mut yellow_lines = Vec::new();

    // Render path cells as cyan polygons and center crosses
    for cell in &path_database.cells {
        let center = cell.center + Vector3::new(0.0, PATH_NODE_HEIGHT, 0.0);

        // Create a small cross at the center (raised to human eye level)
        let size = 0.1;
        cyan_lines.push(VertexPosition {
            position: Vector3::new(center.x - size, center.y, center.z),
        });
        cyan_lines.push(VertexPosition {
            position: Vector3::new(center.x + size, center.y, center.z),
        });
        cyan_lines.push(VertexPosition {
            position: Vector3::new(center.x, center.y - size, center.z),
        });
        cyan_lines.push(VertexPosition {
            position: Vector3::new(center.x, center.y + size, center.z),
        });
        cyan_lines.push(VertexPosition {
            position: Vector3::new(center.x, center.y, center.z - size),
        });
        cyan_lines.push(VertexPosition {
            position: Vector3::new(center.x, center.y, center.z + size),
        });

        // Draw polygon outline by connecting vertices
        if cell.vertex_indices.len() >= 3 {
            for i in 0..cell.vertex_indices.len() {
                let current_idx = cell.vertex_indices[i] as usize;
                let next_idx = cell.vertex_indices[(i + 1) % cell.vertex_indices.len()] as usize;

                if current_idx < path_database.vertices.len()
                    && next_idx < path_database.vertices.len()
                {
                    let current = &path_database.vertices[current_idx];
                    let next = &path_database.vertices[next_idx];

                    cyan_lines.push(VertexPosition {
                        position: Vector3::new(current.x, current.y, current.z),
                    });
                    cyan_lines.push(VertexPosition {
                        position: Vector3::new(next.x, next.y, next.z),
                    });
                }
            }
        }
    }

    // Render path links as yellow lines connecting cell centers
    for link in &path_database.links {
        let from_cell_idx = link.from_cell as usize;
        let to_cell_idx = link.to_cell as usize;

        if from_cell_idx < path_database.cells.len() && to_cell_idx < path_database.cells.len() {
            let from_center = path_database.cells[from_cell_idx].center
                + Vector3::new(0.0, PATH_NODE_HEIGHT, 0.0);
            let to_center =
                path_database.cells[to_cell_idx].center + Vector3::new(0.0, PATH_NODE_HEIGHT, 0.0);

            yellow_lines.push(VertexPosition {
                position: from_center,
            });
            yellow_lines.push(VertexPosition {
                position: to_center,
            });
        }
    }

    // Create cyan lines for cells
    if !cyan_lines.is_empty() {
        let cyan_material = color_material::create(Vector3::new(0.0, 1.0, 1.0));
        let cyan_mesh = SceneObject::new(cyan_material, Box::new(lines_mesh::create(cyan_lines)));
        visuals.push(cyan_mesh);
    }

    // Create yellow lines for links
    if !yellow_lines.is_empty() {
        let yellow_material = color_material::create(Vector3::new(1.0, 1.0, 0.0));
        let yellow_mesh =
            SceneObject::new(yellow_material, Box::new(lines_mesh::create(yellow_lines)));
        visuals.push(yellow_mesh);
    }

    visuals
}
