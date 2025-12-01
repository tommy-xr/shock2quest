use crate::pathfinding::{PathfindingService, path_visualization::PathVisualizationSystem};
/// Interactive pathfinding test system for debugging AI navigation
///
/// Provides P-key cycling through pathfinding test states and HTTP commands
/// for the debug runtime to test A* pathfinding with visual feedback.
use cgmath::Vector3;

/// State machine for interactive pathfinding testing
#[derive(Debug, Clone, PartialEq)]
pub enum PathfindingTestState {
    /// Waiting for user to set start position
    WaitingForStart,
    /// Waiting for user to set goal position (start already set)
    WaitingForGoal,
    /// Showing computed path between start and goal
    ShowingPath,
}

/// Interactive pathfinding test system
pub struct PathfindingTest {
    pub state: PathfindingTestState,
}

impl PathfindingTest {
    /// Create a new pathfinding test system
    pub fn new() -> Self {
        Self {
            state: PathfindingTestState::WaitingForStart,
        }
    }

    /// Handle pathfinding test action and return status message
    ///
    /// Actions:
    /// - "cycle": Cycle through states based on current state
    /// - "set_start": Set start position explicitly
    /// - "set_goal": Set goal position explicitly
    /// - "reset": Reset to initial state
    pub fn handle_action(
        &mut self,
        action: &str,
        player_position: Vector3<f32>,
        pathfinding_service: &Option<PathfindingService>,
        path_visualization: &mut PathVisualizationSystem,
    ) -> String {
        // If a specific action is requested, use it
        if action != "cycle" {
            match action {
                "set_start" => self.set_start(player_position, path_visualization),
                "set_goal" => {
                    self.set_goal(player_position, pathfinding_service, path_visualization)
                }
                "reset" => self.reset(path_visualization),
                _ => format!("Unknown pathfinding test action: {}", action),
            }
        } else {
            // Cycle through states based on current state
            match self.state {
                PathfindingTestState::WaitingForStart => {
                    self.set_start(player_position, path_visualization)
                }
                PathfindingTestState::WaitingForGoal => {
                    self.set_goal(player_position, pathfinding_service, path_visualization)
                }
                PathfindingTestState::ShowingPath => self.reset(path_visualization),
            }
        }
    }

    /// Set start position for pathfinding test
    fn set_start(
        &mut self,
        position: Vector3<f32>,
        path_visualization: &mut PathVisualizationSystem,
    ) -> String {
        // Clear any existing test path
        path_visualization.remove_path("test_path");

        // Store start position for future path computation
        use crate::pathfinding::path_visualization::{
            ComputedPath, MarkerType, PathMarker, colors,
        };

        let mut path = ComputedPath::new("test_start".to_string(), vec![], colors::TEST_PATH);

        path.add_marker(PathMarker {
            position,
            marker_type: MarkerType::Start,
            color: colors::START_MARKER,
        });

        path_visualization.set_path("test_start".to_string(), path);

        // Update state to wait for goal
        self.state = PathfindingTestState::WaitingForGoal;

        format!(
            "Set pathfinding test start at ({:.2}, {:.2}, {:.2}). Press P again to set goal.",
            position.x, position.y, position.z
        )
    }

    /// Set goal position and compute pathfinding route
    fn set_goal(
        &mut self,
        position: Vector3<f32>,
        pathfinding_service: &Option<PathfindingService>,
        path_visualization: &mut PathVisualizationSystem,
    ) -> String {
        if !path_visualization.has_path("test_start") {
            return "No start position set. Press P once to set start first.".to_string();
        }

        // Get start position from existing marker
        let start_pos = if let Some(start_path) = path_visualization.paths.get("test_start") {
            if let Some(marker) = start_path.markers.first() {
                marker.position
            } else {
                return "Error: Could not find start position".to_string();
            }
        } else {
            return "Error: Could not find start position".to_string();
        };

        // Compute pathfinding route
        match pathfinding_service {
            Some(service) => {
                use dark::mission::path_database::MovementBits;
                let movement_bits = MovementBits::WALK; // Human movement

                match service.find_path(start_pos, position, movement_bits) {
                    Some(waypoints) => {
                        use crate::pathfinding::path_visualization::ComputedPath;

                        // Create complete test path with start/goal markers
                        let test_path =
                            ComputedPath::test_path(start_pos, position, waypoints.clone());

                        // Remove old markers and set complete path
                        path_visualization.remove_path("test_start");
                        path_visualization.set_path("test_path".to_string(), test_path);

                        // Update state to show path
                        self.state = PathfindingTestState::ShowingPath;

                        format!(
                            "Computed path with {} waypoints from ({:.2}, {:.2}, {:.2}) to ({:.2}, {:.2}, {:.2}). Press P again to reset.",
                            waypoints.len(),
                            start_pos.x,
                            start_pos.y,
                            start_pos.z,
                            position.x,
                            position.y,
                            position.z
                        )
                    }
                    None => {
                        // No path found - try closest reachable cell
                        self.try_fallback_path(start_pos, position, service, path_visualization)
                    }
                }
            }
            None => "Pathfinding service not available (no AIPATH data)".to_string(),
        }
    }

    /// Try to find fallback path to closest reachable cell
    fn try_fallback_path(
        &mut self,
        start_pos: Vector3<f32>,
        goal_pos: Vector3<f32>,
        service: &PathfindingService,
        path_visualization: &mut PathVisualizationSystem,
    ) -> String {
        use dark::mission::path_database::MovementBits;
        let movement_bits = MovementBits::WALK;

        match service.find_closest_reachable_cell(start_pos, goal_pos, movement_bits) {
            Some(closest_cell_id) => {
                let closest_center = service.path_database.cells[closest_cell_id as usize].center;
                match service.find_path(start_pos, closest_center, movement_bits) {
                    Some(waypoints) => {
                        use crate::pathfinding::path_visualization::ComputedPath;

                        let mut test_path =
                            ComputedPath::test_path(start_pos, closest_center, waypoints.clone());

                        // Add fallback waypoints if pathfinding returns empty result
                        if waypoints.is_empty() {
                            let mid_point = start_pos + (closest_center - start_pos) * 0.5;
                            test_path.waypoints = vec![start_pos, mid_point, closest_center];
                        }

                        path_visualization.remove_path("test_start");
                        path_visualization.set_path("test_path".to_string(), test_path);

                        // Update state to show path
                        self.state = PathfindingTestState::ShowingPath;

                        format!(
                            "No direct path found. Computed fallback path with {} waypoints to closest reachable position ({:.2}, {:.2}, {:.2}). Press P again to reset.",
                            waypoints.len(),
                            closest_center.x,
                            closest_center.y,
                            closest_center.z
                        )
                    }
                    None => "Error: Could not compute path to closest reachable cell".to_string(),
                }
            }
            None => "No path possible - start position may be unreachable".to_string(),
        }
    }

    /// Reset pathfinding test to initial state
    fn reset(&mut self, path_visualization: &mut PathVisualizationSystem) -> String {
        path_visualization.remove_path("test_start");
        path_visualization.remove_path("test_path");

        // Reset state to waiting for start
        self.state = PathfindingTestState::WaitingForStart;

        "Cleared pathfinding test data. Press P to set start position.".to_string()
    }
}
