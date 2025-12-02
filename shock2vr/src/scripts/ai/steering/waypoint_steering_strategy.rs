use cgmath::{Deg, InnerSpace, Vector3, Vector4, Point3};
use dark::properties::PropPosition;
use shipyard::{EntityId, Get, UniqueView, View, World};

use crate::{
    mission::PlayerInfo,
    physics::PhysicsWorld,
    time::Time,
    pathfinding::PathfindingService,
    util::vec3_to_point3,
};

use super::{
    Effect,
    SteeringOutput,
    SteeringStrategy,
    Steering,
};

#[derive(Debug, Clone)]
pub enum NavigationTarget {
    Player,
    Entity(EntityId),
    Position(Vector3<f32>),
}

pub struct WaypointSteeringStrategy {
    target: NavigationTarget,
    pathfinding_service: Option<PathfindingService>,
    current_waypoints: Vec<Vector3<f32>>,
    current_waypoint_index: usize,
    waypoint_reached_distance: f32,
    direct_steering_distance: f32,
    last_path_computation_time: f32,
    path_recompute_interval: f32,
    last_target_position: Option<Vector3<f32>>,
    target_moved_threshold: f32,
}

impl WaypointSteeringStrategy {
    pub fn new(target: NavigationTarget, pathfinding_service: Option<PathfindingService>) -> Self {
        Self {
            target,
            pathfinding_service,
            current_waypoints: Vec::new(),
            current_waypoint_index: 0,
            waypoint_reached_distance: 2.0, // Increased threshold for easier waypoint advancement
            direct_steering_distance: 3.0,
            last_path_computation_time: 0.0,
            path_recompute_interval: 2.0, // Recompute every 2.0 seconds (reduced frequency)
            last_target_position: None,
            target_moved_threshold: 2.0, // Recompute if target moves >2 meters
        }
    }

    pub fn set_target(&mut self, target: NavigationTarget) {
        self.target = target;
        self.current_waypoints.clear();
        self.current_waypoint_index = 0;
    }

    fn get_target_position(&self, _world: &World) -> Option<Vector3<f32>> {
        // DEBUG: Hardcoded target position for testing specific chase behavior
        Some(Vector3::new(29.60, -0.57, -74.05))

        // Original logic (commented for testing):
        // match &self.target {
        //     NavigationTarget::Player => self.get_player_position(world),
        //     NavigationTarget::Entity(entity_id) => self.get_entity_position(world, *entity_id),
        //     NavigationTarget::Position(pos) => Some(*pos),
        // }
    }

    fn should_use_direct_steering(&self, current_pos: Vector3<f32>, target_pos: Vector3<f32>) -> bool {
        // Distance threshold (~3 meters) OR same AIPATH cell
        let distance = (target_pos - current_pos).magnitude();
        if distance < self.direct_steering_distance {
            return true;
        }

        // Check if both positions are in the same AIPATH cell
        if let Some(ref pathfinding) = self.pathfinding_service {
            let current_cell = pathfinding.cell_from_position(current_pos);
            let target_cell = pathfinding.cell_from_position(target_pos);
            return current_cell.is_some() && current_cell == target_cell;
        }

        false
    }

    fn compute_path(&mut self, start_pos: Vector3<f32>, target_pos: Vector3<f32>) -> bool {
        if let Some(ref pathfinding) = self.pathfinding_service {
            println!("[WAYPOINT] PathfindingService available, computing A* path from ({:.2}, {:.2}, {:.2}) to ({:.2}, {:.2}, {:.2})",
                    start_pos.x, start_pos.y, start_pos.z, target_pos.x, target_pos.y, target_pos.z);

            let start_time = std::time::Instant::now();

            use dark::mission::path_database::MovementBits;
            let movement_bits = MovementBits::WALK; // Default to walking movement

            if let Some(waypoints) = pathfinding.find_path(start_pos, target_pos, movement_bits) {
                let duration = start_time.elapsed();
                println!("[WAYPOINT] A* pathfinding returned {} waypoints in {:.3}ms",
                        waypoints.len(), duration.as_secs_f64() * 1000.0);

                if duration.as_millis() > 16 { // Warn if > 16ms (could cause frame drops at 60fps)
                    println!("[WAYPOINT] WARNING: Pathfinding took {:.3}ms - may cause frame drops!",
                            duration.as_secs_f64() * 1000.0);
                }

                self.current_waypoints = waypoints;
                self.current_waypoint_index = 0;
                return true;
            } else {
                let duration = start_time.elapsed();
                println!("[WAYPOINT] A* pathfinding returned no path after {:.3}ms",
                        duration.as_secs_f64() * 1000.0);
            }
        } else {
            println!("[WAYPOINT] PathfindingService NOT available");
        }

        // No path found or no pathfinding service
        self.current_waypoints.clear();
        self.current_waypoint_index = 0;
        false
    }

    fn get_current_waypoint(&self) -> Option<Vector3<f32>> {
        if self.current_waypoint_index < self.current_waypoints.len() {
            Some(self.current_waypoints[self.current_waypoint_index])
        } else {
            None
        }
    }

    fn advance_to_next_waypoint(&mut self, current_pos: Vector3<f32>) -> bool {
        if let Some(waypoint) = self.get_current_waypoint() {
            let distance = (waypoint - current_pos).magnitude();
            println!("[WAYPOINT] Distance to waypoint {}: {:.2} (threshold: {:.2})",
                    self.current_waypoint_index + 1, distance, self.waypoint_reached_distance);

            if distance < self.waypoint_reached_distance {
                println!("[WAYPOINT] Reached waypoint {}! Advancing to next waypoint.",
                        self.current_waypoint_index + 1);
                self.current_waypoint_index += 1;
                return true;
            }
        }
        false
    }

    fn get_entity_position(&self, world: &World, entity_id: EntityId) -> Option<Vector3<f32>> {
        let v_positions = world.borrow::<View<PropPosition>>().ok()?;
        let prop_pos = v_positions.get(entity_id).ok()?;
        Some(prop_pos.position)
    }

    fn get_player_position(&self, world: &World) -> Option<Vector3<f32>> {
        let u_player = world.borrow::<UniqueView<PlayerInfo>>().ok()?;
        Some(u_player.pos)
    }

    /// Create debug visualization for the current path
    fn create_path_visualization(&self, _entity_id: EntityId, current_pos: Vector3<f32>, current_waypoint: Vector3<f32>) -> Effect {
        let mut debug_lines = Vec::new();
        const PATH_HEIGHT_OFFSET: f32 = 1.0; // Raise paths above ground for visibility

        // Different colors for different visualization components
        let path_color = Vector4::new(1.0, 0.5, 0.0, 1.0); // Orange for AI paths
        let current_segment_color = Vector4::new(0.0, 1.0, 0.0, 1.0); // Green for current segment
        let waypoint_color = Vector4::new(1.0, 1.0, 0.0, 1.0); // Yellow for waypoints

        // 1. Draw the full remaining path from current waypoint index onwards
        for i in self.current_waypoint_index..self.current_waypoints.len().saturating_sub(1) {
            let start = self.current_waypoints[i] + Vector3::new(0.0, PATH_HEIGHT_OFFSET, 0.0);
            let end = self.current_waypoints[i + 1] + Vector3::new(0.0, PATH_HEIGHT_OFFSET, 0.0);
            debug_lines.push((
                Point3::new(start.x, start.y, start.z),
                Point3::new(end.x, end.y, end.z),
                path_color
            ));
        }

        // 2. Draw line from current position to current waypoint (highlighted)
        let current_elevated = current_pos + Vector3::new(0.0, PATH_HEIGHT_OFFSET, 0.0);
        let waypoint_elevated = current_waypoint + Vector3::new(0.0, PATH_HEIGHT_OFFSET, 0.0);
        debug_lines.push((
            Point3::new(current_elevated.x, current_elevated.y, current_elevated.z),
            Point3::new(waypoint_elevated.x, waypoint_elevated.y, waypoint_elevated.z),
            current_segment_color
        ));

        // 3. Draw markers at each remaining waypoint
        for i in self.current_waypoint_index..self.current_waypoints.len() {
            let waypoint = self.current_waypoints[i] + Vector3::new(0.0, PATH_HEIGHT_OFFSET, 0.0);
            let marker_size = 0.3;

            // Draw a small cross at each waypoint
            debug_lines.push((
                Point3::new(waypoint.x - marker_size, waypoint.y, waypoint.z),
                Point3::new(waypoint.x + marker_size, waypoint.y, waypoint.z),
                waypoint_color
            ));
            debug_lines.push((
                Point3::new(waypoint.x, waypoint.y - marker_size, waypoint.z),
                Point3::new(waypoint.x, waypoint.y + marker_size, waypoint.z),
                waypoint_color
            ));
        }

        // 4. Draw entity indicator
        let entity_marker_size = 0.5;
        let entity_color = Vector4::new(0.0, 0.0, 1.0, 1.0); // Blue for entity position
        debug_lines.push((
            Point3::new(current_elevated.x - entity_marker_size, current_elevated.y, current_elevated.z),
            Point3::new(current_elevated.x + entity_marker_size, current_elevated.y, current_elevated.z),
            entity_color
        ));
        debug_lines.push((
            Point3::new(current_elevated.x, current_elevated.y, current_elevated.z - entity_marker_size),
            Point3::new(current_elevated.x, current_elevated.y, current_elevated.z + entity_marker_size),
            entity_color
        ));

        if debug_lines.is_empty() {
            Effect::NoEffect
        } else {
            Effect::DrawDebugLines { lines: debug_lines }
        }
    }
}

impl SteeringStrategy for WaypointSteeringStrategy {
    fn steer(
        &mut self,
        _current_heading: Deg<f32>,
        world: &World,
        _physics: &PhysicsWorld,
        entity_id: EntityId,
        _time: &Time,
    ) -> Option<(SteeringOutput, Effect)> {
        println!("[WAYPOINT] Entity {} steering called", entity_id.inner());

        // Get current entity position
        let current_pos = self.get_entity_position(world, entity_id)?;
        println!("[WAYPOINT] Entity {} current position: ({:.2}, {:.2}, {:.2})",
                entity_id.inner(), current_pos.x, current_pos.y, current_pos.z);


        // Get target position
        let target_pos = self.get_target_position(world)?;
        println!("[WAYPOINT] Entity {} target position: ({:.2}, {:.2}, {:.2})",
                entity_id.inner(), target_pos.x, target_pos.y, target_pos.z);

        // Check if we should use direct steering (close distance or same cell)
        if self.should_use_direct_steering(current_pos, target_pos) {
            println!("[WAYPOINT] Entity {} using DIRECT steering (close or same cell)", entity_id.inner());
            let steering = Steering::turn_to_point(vec3_to_point3(current_pos), vec3_to_point3(target_pos));
            return Some((steering, Effect::NoEffect));
        }

        // Check if we need to compute a new path - ONLY compute once when empty
        let needs_new_path = self.current_waypoints.is_empty();

        if needs_new_path {
            println!("[WAYPOINT] Entity {} computing new path (first time only)", entity_id.inner());

            if !self.compute_path(current_pos, target_pos) {
                // No path found - fall back to direct steering
                println!("[WAYPOINT] Entity {} NO PATH FOUND - fallback to direct steering", entity_id.inner());
                let steering = Steering::turn_to_point(vec3_to_point3(current_pos), vec3_to_point3(target_pos));
                return Some((steering, Effect::NoEffect));
            }
            println!("[WAYPOINT] Entity {} path computed with {} waypoints",
                    entity_id.inner(), self.current_waypoints.len());
        }

        // Check if we've reached the current waypoint
        self.advance_to_next_waypoint(current_pos);

        // Get the current waypoint to navigate to
        let waypoint = if let Some(waypoint) = self.get_current_waypoint() {
            println!("[WAYPOINT] Entity {} navigating to waypoint {}/{}: ({:.2}, {:.2}, {:.2})",
                    entity_id.inner(), self.current_waypoint_index + 1, self.current_waypoints.len(),
                    waypoint.x, waypoint.y, waypoint.z);
            waypoint
        } else {
            // No more waypoints - use direct steering to final target
            let distance_to_target = (target_pos - current_pos).magnitude();
            println!("[WAYPOINT] Entity {} reached end of path - direct steering to target (distance: {:.2}, waypoint_index: {}, total_waypoints: {})",
                    entity_id.inner(), distance_to_target, self.current_waypoint_index, self.current_waypoints.len());

            // DEBUG: Log the full path for analysis
            if !self.current_waypoints.is_empty() {
                println!("[WAYPOINT] Entity {} DEBUG: Path was: start->{}->end", entity_id.inner(),
                        self.current_waypoints.iter()
                            .take(5) // Show first 5 waypoints
                            .map(|w| format!("({:.1},{:.1},{:.1})", w.x, w.y, w.z))
                            .collect::<Vec<_>>()
                            .join("->"));
            } else {
                println!("[WAYPOINT] Entity {} DEBUG: No waypoints were ever computed!", entity_id.inner());
            }

            let steering = Steering::turn_to_point(vec3_to_point3(current_pos), vec3_to_point3(target_pos));
            return Some((steering, Effect::NoEffect));
        };

        // Navigate to current waypoint
        let steering = Steering::turn_to_point(vec3_to_point3(current_pos), vec3_to_point3(waypoint));

        // Create debug visualization for the path
        let visualization_effect = self.create_path_visualization(entity_id, current_pos, waypoint);

        Some((steering, visualization_effect))
    }
}