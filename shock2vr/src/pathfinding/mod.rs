/// Pathfinding module for AI navigation using AIPATH data
///
/// This module provides A* pathfinding capabilities using the navigation mesh
/// stored in AIPATH chunks. It maintains separation from the BSP tree system
/// used for rendering/visibility queries.
pub mod path_visualization;

use cgmath::{InnerSpace, Vector3};
use dark::mission::{
    PathDatabase,
    path_database::{MovementBits, PathCell},
};
use std::sync::Arc;

/// Pathfinding service for AI navigation
///
/// Uses AIPATH cells for navigation mesh queries and A* pathfinding.
/// Keeps the spatial query implementation simple and swappable.
pub struct PathfindingService {
    path_database: Arc<PathDatabase>,
}

impl PathfindingService {
    /// Create a new pathfinding service with the given path database
    pub fn new(path_database: Arc<PathDatabase>) -> Self {
        Self { path_database }
    }

    /// Find the AIPATH cell containing a world position
    ///
    /// Uses simple point-in-polygon tests on convex AIPATH cells.
    /// This implementation is straightforward and can be optimized later
    /// with spatial indexing if needed.
    pub fn cell_from_position(&self, pos: Vector3<f32>) -> Option<u32> {
        // Simple linear search through all cells
        // For ~4700 cells this should be acceptable for initial implementation
        for (idx, cell) in self.path_database.cells.iter().enumerate() {
            if self.point_in_cell(pos, cell) {
                return Some(idx as u32);
            }
        }
        None
    }

    /// Find path from start position to goal position using A* algorithm
    ///
    /// Returns a list of waypoints (cell centers) to traverse, or None if no path exists.
    pub fn find_path(
        &self,
        start: Vector3<f32>,
        goal: Vector3<f32>,
        movement_bits: MovementBits,
    ) -> Option<Vec<Vector3<f32>>> {
        // Find start and goal cells
        let start_cell_id = self.cell_from_position(start)?;
        let goal_cell_id = self.cell_from_position(goal)?;

        // Use pathfinding crate for A* algorithm
        let result = pathfinding::directed::astar::astar(
            &start_cell_id,
            |&cell_id| self.get_successors(cell_id, movement_bits),
            |&cell_id| self.heuristic(cell_id, goal_cell_id),
            |&cell_id| cell_id == goal_cell_id,
        )?;

        // Convert cell path to world positions (cell centers)
        let waypoints = result
            .0
            .iter()
            .map(|&cell_id| self.path_database.cells[cell_id as usize].center)
            .collect();

        Some(waypoints)
    }

    /// Find the closest reachable cell to a goal position
    ///
    /// Useful when the exact goal position is in an unpathable area.
    /// Returns the cell ID of the closest reachable cell.
    pub fn find_closest_reachable_cell(
        &self,
        start: Vector3<f32>,
        goal: Vector3<f32>,
        movement_bits: MovementBits,
    ) -> Option<u32> {
        let start_cell_id = self.cell_from_position(start)?;

        // Find all reachable cells using Dijkstra's algorithm
        let reachable = pathfinding::directed::dijkstra::dijkstra_all(&start_cell_id, |&cell_id| {
            self.get_successors(cell_id, movement_bits)
        });

        // Find the reachable cell closest to the goal
        let mut closest_cell = Some(start_cell_id); // Start with start cell as fallback
        let start_center = self.path_database.cells[start_cell_id as usize].center;
        let mut closest_distance = (goal - start_center).magnitude();

        // Check all other reachable cells
        for (cell_id, _) in reachable {
            let cell_center = self.path_database.cells[cell_id as usize].center;
            let distance = (goal - cell_center).magnitude();

            if distance < closest_distance {
                closest_distance = distance;
                closest_cell = Some(cell_id);
            }
        }

        closest_cell
    }

    /// Get the successors of a cell for A* pathfinding
    ///
    /// Returns a list of (target_cell_id, cost) pairs for cells reachable from the given cell.
    fn get_successors(&self, cell_id: u32, movement_bits: MovementBits) -> Vec<(u32, u32)> {
        self.path_database
            .links
            .iter()
            .filter(|link| link.from_cell == cell_id)
            .filter(|link| link.ok_bits.intersects(movement_bits))
            .map(|link| (link.to_cell, link.cost as u32))
            .collect()
    }

    /// Calculate heuristic distance between two cells for A*
    ///
    /// Uses Euclidean distance between cell centers as the heuristic.
    fn heuristic(&self, from_cell: u32, to_cell: u32) -> u32 {
        let from_center = self.path_database.cells[from_cell as usize].center;
        let to_center = self.path_database.cells[to_cell as usize].center;

        (from_center - to_center).magnitude() as u32
    }

    /// Test if a point is inside a convex AIPATH cell
    ///
    /// Uses simple point-in-polygon test. Since AIPATH cells are convex,
    /// this can be done efficiently by checking that the point is on the
    /// same side of all polygon edges.
    ///
    /// Note: This is a 2D test in the XZ plane, assuming Y coordinate doesn't matter
    /// for floor-based navigation.
    fn point_in_cell(&self, point: Vector3<f32>, cell: &PathCell) -> bool {
        // Skip cells with no vertices
        if cell.vertex_indices.len() < 3 {
            return false;
        }

        // Get the vertices of this cell
        let vertices: Vec<Vector3<f32>> = cell
            .vertex_indices
            .iter()
            .filter_map(|&idx| self.path_database.vertices.get(idx as usize))
            .copied()
            .collect();

        if vertices.len() < 3 {
            return false;
        }

        // Point-in-polygon test using cross products (2D in XZ plane)
        // For a convex polygon, point is inside if it's on the same side of all edges
        let mut sign = None;

        for i in 0..vertices.len() {
            let v1 = vertices[i];
            let v2 = vertices[(i + 1) % vertices.len()];

            // Calculate cross product to determine which side of edge the point is on
            let edge = Vector3::new(v2.x - v1.x, 0.0, v2.z - v1.z);
            let to_point = Vector3::new(point.x - v1.x, 0.0, point.z - v1.z);
            let cross = edge.x * to_point.z - edge.z * to_point.x;

            if cross.abs() < f32::EPSILON {
                continue; // Point is on the edge
            }

            let current_sign = cross > 0.0;

            match sign {
                None => sign = Some(current_sign),
                Some(prev_sign) if prev_sign != current_sign => return false,
                _ => {}
            }
        }

        true
    }
}
