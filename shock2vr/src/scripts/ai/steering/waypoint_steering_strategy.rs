use cgmath::{Deg, InnerSpace, Vector3};
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
}

impl WaypointSteeringStrategy {
    pub fn new(target: NavigationTarget, pathfinding_service: Option<PathfindingService>) -> Self {
        Self {
            target,
            pathfinding_service,
            current_waypoints: Vec::new(),
            current_waypoint_index: 0,
            waypoint_reached_distance: 0.5,
            direct_steering_distance: 3.0,
        }
    }

    pub fn set_target(&mut self, target: NavigationTarget) {
        self.target = target;
        self.current_waypoints.clear();
        self.current_waypoint_index = 0;
    }

    fn get_target_position(&self, world: &World) -> Option<Vector3<f32>> {
        match &self.target {
            NavigationTarget::Player => self.get_player_position(world),
            NavigationTarget::Entity(entity_id) => self.get_entity_position(world, *entity_id),
            NavigationTarget::Position(pos) => Some(*pos),
        }
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
            use dark::mission::path_database::MovementBits;
            let movement_bits = MovementBits::WALK; // Default to walking movement

            if let Some(waypoints) = pathfinding.find_path(start_pos, target_pos, movement_bits) {
                self.current_waypoints = waypoints;
                self.current_waypoint_index = 0;
                return true;
            }
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
            if distance < self.waypoint_reached_distance {
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
        // Get current entity position
        let current_pos = self.get_entity_position(world, entity_id)?;

        // Get target position
        let target_pos = self.get_target_position(world)?;

        // Check if we should use direct steering (close distance or same cell)
        if self.should_use_direct_steering(current_pos, target_pos) {
            let steering = Steering::turn_to_point(vec3_to_point3(current_pos), vec3_to_point3(target_pos));
            return Some((steering, Effect::NoEffect));
        }

        // Check if we need to compute a new path
        if self.current_waypoints.is_empty() {
            if !self.compute_path(current_pos, target_pos) {
                // No path found - fall back to direct steering
                let steering = Steering::turn_to_point(vec3_to_point3(current_pos), vec3_to_point3(target_pos));
                return Some((steering, Effect::NoEffect));
            }
        }

        // Check if we've reached the current waypoint
        self.advance_to_next_waypoint(current_pos);

        // Get the current waypoint to navigate to
        let waypoint = if let Some(waypoint) = self.get_current_waypoint() {
            waypoint
        } else {
            // No more waypoints - use direct steering to final target
            let steering = Steering::turn_to_point(vec3_to_point3(current_pos), vec3_to_point3(target_pos));
            return Some((steering, Effect::NoEffect));
        };

        // Navigate to current waypoint
        let steering = Steering::turn_to_point(vec3_to_point3(current_pos), vec3_to_point3(waypoint));
        Some((steering, Effect::NoEffect))
    }
}