/// Pathfinding module for AI navigation using AIPATH data
///
/// This module provides A* pathfinding capabilities using the navigation mesh
/// stored in AIPATH chunks. It maintains separation from the BSP tree system
/// used for rendering/visibility queries.
pub mod path_visualization;

use cgmath::{InnerSpace, Vector3};
use dark::{
    SCALE_FACTOR,
    mission::{
        PathDatabase,
        path_database::{MovementBits, PathCell, PathCellFlags, PathCellLink},
    },
};
use std::sync::Arc;

/// Pathfinding service for AI navigation
///
/// Uses AIPATH cells for navigation mesh queries and A* pathfinding.
/// Keeps the spatial query implementation simple and swappable.
pub struct PathfindingService {
    pub path_database: Arc<PathDatabase>,
    /// Outgoing link indices for each cell, so A* expansion is O(degree)
    /// instead of a scan over every link in the mission.
    links_by_cell: Vec<Vec<u32>>,
}

impl PathfindingService {
    /// Create a new pathfinding service with the given path database
    pub fn new(path_database: Arc<PathDatabase>) -> Self {
        let mut links_by_cell = vec![Vec::new(); path_database.cells.len()];
        for (idx, link) in path_database.links.iter().enumerate() {
            if let Some(links) = links_by_cell.get_mut(link.from_cell as usize) {
                links.push(idx as u32);
            }
        }
        Self {
            path_database,
            links_by_cell,
        }
    }

    /// Find the AIPATH cell containing a world position
    ///
    /// Uses point-in-polygon tests in the XZ plane on convex AIPATH cells.
    /// Cells from different floors overlap in XZ, so among matches we pick
    /// the cell whose floor height is closest to the query position (the
    /// original engine raycasts to the floor for the same reason -
    /// AIFindClosestCell in aipthloc.cpp).
    pub fn cell_from_position(&self, pos: Vector3<f32>) -> Option<u32> {
        let mut best: Option<(u32, f32)> = None;
        for (idx, cell) in self.path_database.cells.iter().enumerate() {
            if self.point_in_cell(pos, cell) {
                let dy = (pos.y - cell.center.y).abs();
                if best.map(|(_, best_dy)| dy < best_dy).unwrap_or(true) {
                    best = Some((idx as u32, dy));
                }
            }
        }
        best.map(|(idx, _)| idx)
    }

    /// Find path from start position to goal position using A* algorithm
    ///
    /// Returns a list of waypoints to traverse, or None if no path exists.
    /// Waypoints are points on the shared edges between cells (not cell
    /// centers), pulled taut toward the goal, mirroring how the original
    /// engine follows cAIPath edges rather than cell centers.
    pub fn find_path(
        &self,
        start: Vector3<f32>,
        goal: Vector3<f32>,
        movement_bits: MovementBits,
    ) -> Option<Vec<Vector3<f32>>> {
        if let Some(path) = self.find_path_with_bits(start, goal, movement_bits) {
            return Some(path);
        }
        // Second pass: a failed pathfind is retried with the stressed
        // condition added (small creatures excepted), so a calm AI still
        // reaches goals whose only route crosses stressed-gated links.
        if !movement_bits.contains(MovementBits::SMALL_CREATURE)
            && !movement_bits.contains(MovementBits::STRESSED)
        {
            return self.find_path_with_bits(start, goal, movement_bits | MovementBits::STRESSED);
        }
        None
    }

    fn find_path_with_bits(
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

        Some(self.waypoints_for_cell_path(&result.0, start, goal, movement_bits))
    }

    /// Convert a cell-id path into world-space waypoints.
    ///
    /// Each cell crossing contributes the point on the shared edge closest to
    /// the next waypoint, computed backward from the goal so the path is
    /// pulled taut instead of zigzagging through cell centers ("diamond"
    /// patterns where many small cells converge at intersections).
    fn waypoints_for_cell_path(
        &self,
        cell_path: &[u32],
        start: Vector3<f32>,
        goal: Vector3<f32>,
        movement_bits: MovementBits,
    ) -> Vec<Vector3<f32>> {
        // Collect the shared edge for each crossing
        let mut edges = Vec::with_capacity(cell_path.len().saturating_sub(1));
        for pair in cell_path.windows(2) {
            let edge = self
                .link_between(pair[0], pair[1], movement_bits)
                .and_then(|link| {
                    let a = *self
                        .path_database
                        .vertices
                        .get(link.edge_vertex_a as usize)?;
                    let b = *self
                        .path_database
                        .vertices
                        .get(link.edge_vertex_b as usize)?;
                    Some((a, b))
                });
            edges.push(edge.unwrap_or_else(|| {
                // No usable edge data; fall back to the destination cell center
                let center = self.path_database.cells[pair[1] as usize].center;
                (center, center)
            }));
        }

        // Pull the path taut: walk the edges backward, aiming each crossing
        // point at the next waypoint (starting from the goal).
        let mut points = vec![Vector3::new(0.0, 0.0, 0.0); edges.len()];
        let mut next_point = goal;
        for (i, (a, b)) in edges.iter().enumerate().rev() {
            let point = closest_point_on_segment(*a, *b, next_point);
            points[i] = point;
            next_point = point;
        }

        let mut waypoints = Vec::with_capacity(edges.len() + 2);
        waypoints.push(start);
        waypoints.extend(points);
        waypoints.push(goal);
        waypoints
    }

    /// Find a traversable link from one cell to an adjacent cell
    fn link_between(
        &self,
        from_cell: u32,
        to_cell: u32,
        movement_bits: MovementBits,
    ) -> Option<&PathCellLink> {
        self.links_by_cell
            .get(from_cell as usize)?
            .iter()
            .map(|&idx| &self.path_database.links[idx as usize])
            .find(|link| link.to_cell == to_cell && self.can_use_link(link, movement_bits))
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

    /// Port of the original engine's AICanUseLink (aipthfnd.cpp): a link is
    /// traversable when the destination cell is not blocked, any condition
    /// bits on the link (stressed / high-strike) are satisfied by the AI,
    /// and the movement medium matches. Door and app-callback gating from
    /// the original are not modeled yet.
    fn can_use_link(&self, link: &PathCellLink, movement_bits: MovementBits) -> bool {
        let dest = match self.path_database.cells.get(link.to_cell as usize) {
            Some(dest) => dest,
            None => return false,
        };
        if dest
            .flags
            .intersects(PathCellFlags::UNPATHABLE | PathCellFlags::BLOCKING_OBB)
        {
            return false;
        }

        // Small creatures may only use small-creature links
        if movement_bits.contains(MovementBits::SMALL_CREATURE)
            && !link.ok_bits.contains(MovementBits::SMALL_CREATURE)
        {
            return false;
        }

        // Condition-gated links (stressed / high-strike) require the AI to be
        // in that condition
        let link_conditions = link.ok_bits & MovementBits::CONDITION_MASK;
        if !link_conditions.is_empty() && (link_conditions & movement_bits).is_empty() {
            return false;
        }

        // Movement medium must match (condition bits alone don't qualify)
        link.ok_bits
            .intersects(movement_bits & !MovementBits::CONDITION_MASK)
    }

    /// Get the successors of a cell for A* pathfinding
    ///
    /// Returns a list of (target_cell_id, cost) pairs for cells reachable from the given cell.
    fn get_successors(&self, cell_id: u32, movement_bits: MovementBits) -> Vec<(u32, u32)> {
        let Some(link_indices) = self.links_by_cell.get(cell_id as usize) else {
            return Vec::new();
        };
        link_indices
            .iter()
            .map(|&idx| &self.path_database.links[idx as usize])
            .filter(|link| self.can_use_link(link, movement_bits))
            .map(|link| (link.to_cell, link.cost as u32))
            .collect()
    }

    /// Calculate heuristic distance between two cells for A*
    ///
    /// Link costs are stored in original Dark units (ComputeCell2CellCost in
    /// aipathdb.cpp), while cell centers were divided by SCALE_FACTOR at
    /// parse time - scale back up so the heuristic and the edge costs use
    /// the same units.
    fn heuristic(&self, from_cell: u32, to_cell: u32) -> u32 {
        let from_center = self.path_database.cells[from_cell as usize].center;
        let to_center = self.path_database.cells[to_cell as usize].center;

        ((from_center - to_center).magnitude() * SCALE_FACTOR) as u32
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

/// Closest point to `target` on the segment from `a` to `b`
fn closest_point_on_segment(
    a: Vector3<f32>,
    b: Vector3<f32>,
    target: Vector3<f32>,
) -> Vector3<f32> {
    let ab = b - a;
    let len_sq = ab.magnitude2();
    if len_sq < 1e-8 {
        return a;
    }
    let t = ((target - a).dot(ab) / len_sq).clamp(0.0, 1.0);
    a + ab * t
}

#[cfg(test)]
mod tests {
    use super::*;
    use cgmath::vec3;
    use dark::mission::path_database::{PathCell, PathCellLink};

    /// Three unit-square cells in a row along X: 0 -> 1 -> 2.
    /// The 1 -> 2 link is gated by STRESSED; everything else is plain WALK.
    fn three_cell_db(last_cell_flags: PathCellFlags) -> PathDatabase {
        let vertices = vec![
            vec3(0.0, 0.0, 0.0),
            vec3(2.0, 0.0, 0.0),
            vec3(2.0, 0.0, 2.0),
            vec3(0.0, 0.0, 2.0),
            vec3(4.0, 0.0, 0.0),
            vec3(4.0, 0.0, 2.0),
            vec3(6.0, 0.0, 0.0),
            vec3(6.0, 0.0, 2.0),
        ];
        let cells = vec![
            PathCell {
                id: 0,
                center: vec3(1.0, 0.0, 1.0),
                vertex_indices: vec![0, 1, 2, 3],
                flags: PathCellFlags::empty(),
            },
            PathCell {
                id: 1,
                center: vec3(3.0, 0.0, 1.0),
                vertex_indices: vec![1, 4, 5, 2],
                flags: PathCellFlags::empty(),
            },
            PathCell {
                id: 2,
                center: vec3(5.0, 0.0, 1.0),
                vertex_indices: vec![4, 6, 7, 5],
                flags: last_cell_flags,
            },
        ];
        let links = vec![
            PathCellLink {
                from_cell: 0,
                to_cell: 1,
                edge_vertex_a: 1,
                edge_vertex_b: 2,
                ok_bits: MovementBits::WALK,
                cost: 5,
            },
            PathCellLink {
                from_cell: 1,
                to_cell: 2,
                edge_vertex_a: 4,
                edge_vertex_b: 5,
                ok_bits: MovementBits::WALK | MovementBits::STRESSED,
                cost: 5,
            },
        ];
        PathDatabase {
            cells,
            vertices,
            links,
        }
    }

    fn service(db: PathDatabase) -> PathfindingService {
        PathfindingService::new(Arc::new(db))
    }

    #[test]
    fn condition_gated_link_rejected_for_plain_walk() {
        let service = service(three_cell_db(PathCellFlags::empty()));
        let gated = &service.path_database.links[1];
        assert!(
            !service.can_use_link(gated, MovementBits::WALK),
            "STRESSED-gated link must not be usable by a calm AI"
        );
        assert!(
            service.can_use_link(gated, MovementBits::WALK | MovementBits::STRESSED),
            "stressed AI can use the gated link"
        );
    }

    #[test]
    fn calm_walk_falls_back_to_stressed_route() {
        // The only route to cell 2 crosses a STRESSED-gated link. A calm WALK
        // query should still find it via the stressed second pass.
        let service = service(three_cell_db(PathCellFlags::empty()));
        let path = service.find_path(vec3(1.0, 0.0, 1.0), vec3(5.0, 0.0, 1.0), MovementBits::WALK);
        assert!(
            path.is_some(),
            "calm AI should route through stressed links when no calm route exists"
        );
    }

    #[test]
    fn small_creatures_get_no_stressed_fallback() {
        let mut db = three_cell_db(PathCellFlags::empty());
        for link in &mut db.links {
            link.ok_bits |= MovementBits::SMALL_CREATURE;
        }
        let service = service(db);
        let path = service.find_path(
            vec3(1.0, 0.0, 1.0),
            vec3(5.0, 0.0, 1.0),
            MovementBits::WALK | MovementBits::SMALL_CREATURE,
        );
        assert!(
            path.is_none(),
            "small creatures must not take the stressed fallback route"
        );
    }

    #[test]
    fn condition_gated_link_usable_when_condition_matches() {
        let service = service(three_cell_db(PathCellFlags::empty()));
        let path = service.find_path(
            vec3(1.0, 0.0, 1.0),
            vec3(5.0, 0.0, 1.0),
            MovementBits::WALK | MovementBits::STRESSED,
        );
        assert!(path.is_some(), "stressed AI should traverse the gated link");
    }

    #[test]
    fn link_into_blocked_cell_rejected() {
        let service = service(three_cell_db(PathCellFlags::UNPATHABLE));
        let path = service.find_path(vec3(1.0, 0.0, 1.0), vec3(3.0, 0.0, 1.0), MovementBits::WALK);
        assert!(path.is_some(), "path to the middle cell still works");
        // Goal resolves to the unpathable cell; A* must refuse to enter it.
        let blocked = service.find_path(
            vec3(1.0, 0.0, 1.0),
            vec3(5.0, 0.0, 1.0),
            MovementBits::WALK | MovementBits::STRESSED,
        );
        assert!(
            blocked.is_none(),
            "unpathable destination cell must be rejected"
        );
    }

    #[test]
    fn waypoints_cross_shared_edges_not_cell_centers() {
        let service = service(three_cell_db(PathCellFlags::empty()));
        let start = vec3(1.0, 0.0, 1.0);
        let goal = vec3(5.0, 0.0, 1.0);
        let path = service
            .find_path(start, goal, MovementBits::WALK | MovementBits::STRESSED)
            .unwrap();
        assert_eq!(path.len(), 4, "start + 2 edge crossings + goal");
        assert_eq!(path[0], start);
        assert_eq!(path[3], goal);
        // Crossing points sit on the shared edges (x = 2 and x = 4)
        assert!((path[1].x - 2.0).abs() < 1e-5);
        assert!((path[2].x - 4.0).abs() < 1e-5);
        // Straight corridor: the taut path stays on the straight line z = 1
        assert!((path[1].z - 1.0).abs() < 1e-5);
        assert!((path[2].z - 1.0).abs() < 1e-5);
    }

    #[test]
    fn cell_lookup_prefers_matching_floor() {
        let mut db = three_cell_db(PathCellFlags::empty());
        // A second floor directly above cell 0
        let base = db.vertices.len() as u32;
        db.vertices.extend([
            vec3(0.0, 10.0, 0.0),
            vec3(2.0, 10.0, 0.0),
            vec3(2.0, 10.0, 2.0),
            vec3(0.0, 10.0, 2.0),
        ]);
        db.cells.push(PathCell {
            id: 3,
            center: vec3(1.0, 10.0, 1.0),
            vertex_indices: vec![base, base + 1, base + 2, base + 3],
            flags: PathCellFlags::empty(),
        });
        let service = service(db);
        assert_eq!(service.cell_from_position(vec3(1.0, 0.5, 1.0)), Some(0));
        assert_eq!(service.cell_from_position(vec3(1.0, 9.5, 1.0)), Some(3));
    }
}
