use crate::creature::HUMAN_HEIGHT;
use cgmath::Vector3;
use engine::scene::{SceneObject, VertexPosition, color_material, lines_mesh};
/// Path visualization system for debugging and AI path display
///
/// Supports multiple simultaneous paths with different colors and markers.
/// Designed to work with the interactive pathfinding test system.
use std::collections::HashMap;

/// Height offset for path visualization - center of human height
const PATH_NODE_HEIGHT: f32 = HUMAN_HEIGHT / 2.0;

/// System for managing and rendering multiple pathfinding visualizations
pub struct PathVisualizationSystem {
    pub paths: HashMap<String, ComputedPath>,
}

impl PathVisualizationSystem {
    /// Create a new empty path visualization system
    pub fn new() -> Self {
        Self {
            paths: HashMap::new(),
        }
    }

    /// Add or update a path in the visualization system
    pub fn set_path(&mut self, name: String, path: ComputedPath) {
        self.paths.insert(name, path);
    }

    /// Remove a path from the visualization system
    pub fn remove_path(&mut self, name: &str) {
        self.paths.remove(name);
    }

    /// Clear all paths from the visualization system
    pub fn clear_all(&mut self) {
        self.paths.clear();
    }

    /// Check if a path exists
    pub fn has_path(&self, name: &str) -> bool {
        self.paths.contains_key(name)
    }

    /// Render all paths and markers as scene objects
    pub fn render(&self) -> Vec<SceneObject> {
        let mut visuals = Vec::new();

        for path in self.paths.values() {
            visuals.extend(self.render_path(path));
        }

        visuals
    }

    /// Render a single path as scene objects
    fn render_path(&self, path: &ComputedPath) -> Vec<SceneObject> {
        let mut visuals = Vec::new();

        // Render path waypoints as lines
        if path.waypoints.len() >= 2 {
            let mut path_lines = Vec::new();

            for i in 0..path.waypoints.len() - 1 {
                let current = path.waypoints[i] + Vector3::new(0.0, PATH_NODE_HEIGHT, 0.0);
                let next = path.waypoints[i + 1] + Vector3::new(0.0, PATH_NODE_HEIGHT, 0.0);

                path_lines.push(VertexPosition { position: current });
                path_lines.push(VertexPosition { position: next });
            }

            if !path_lines.is_empty() {
                let material = color_material::create(path.color);
                let mesh = SceneObject::new(material, Box::new(lines_mesh::create(path_lines)));
                visuals.push(mesh);
            }
        }

        // Render markers (start, goal, waypoints)
        for marker in &path.markers {
            visuals.extend(self.render_marker(marker));
        }

        visuals
    }

    /// Render a single marker as scene objects
    fn render_marker(&self, marker: &PathMarker) -> Vec<SceneObject> {
        let marker_pos = marker.position + Vector3::new(0.0, PATH_NODE_HEIGHT, 0.0);

        match marker.marker_type {
            MarkerType::Start | MarkerType::Goal => {
                // Render as a small cross
                let size = 0.2;
                let mut marker_lines = Vec::new();

                // X axis
                marker_lines.push(VertexPosition {
                    position: marker_pos + Vector3::new(-size, 0.0, 0.0),
                });
                marker_lines.push(VertexPosition {
                    position: marker_pos + Vector3::new(size, 0.0, 0.0),
                });

                // Y axis
                marker_lines.push(VertexPosition {
                    position: marker_pos + Vector3::new(0.0, -size, 0.0),
                });
                marker_lines.push(VertexPosition {
                    position: marker_pos + Vector3::new(0.0, size, 0.0),
                });

                // Z axis
                marker_lines.push(VertexPosition {
                    position: marker_pos + Vector3::new(0.0, 0.0, -size),
                });
                marker_lines.push(VertexPosition {
                    position: marker_pos + Vector3::new(0.0, 0.0, size),
                });

                let material = color_material::create(marker.color);
                let mesh = SceneObject::new(material, Box::new(lines_mesh::create(marker_lines)));
                vec![mesh]
            }
            MarkerType::Waypoint => {
                // Render as a small dot (single point)
                let marker_lines = vec![
                    VertexPosition {
                        position: marker_pos,
                    },
                    VertexPosition {
                        position: marker_pos,
                    },
                ];

                let material = color_material::create(marker.color);
                let mesh = SceneObject::new(material, Box::new(lines_mesh::create(marker_lines)));
                vec![mesh]
            }
        }
    }
}

/// A computed path with visualization properties
#[derive(Clone, Debug)]
pub struct ComputedPath {
    /// Path waypoints in world coordinates
    pub waypoints: Vec<Vector3<f32>>,
    /// Color for rendering this path
    pub color: Vector3<f32>,
    /// Human-readable name for this path
    pub name: String,
    /// Start/goal markers for this path
    pub markers: Vec<PathMarker>,
}

impl ComputedPath {
    /// Create a new computed path
    pub fn new(name: String, waypoints: Vec<Vector3<f32>>, color: Vector3<f32>) -> Self {
        Self {
            waypoints,
            color,
            name,
            markers: Vec::new(),
        }
    }

    /// Add a marker to this path
    pub fn add_marker(&mut self, marker: PathMarker) {
        self.markers.push(marker);
    }

    /// Create a test path with start and goal markers
    pub fn test_path(
        start: Vector3<f32>,
        goal: Vector3<f32>,
        waypoints: Vec<Vector3<f32>>,
    ) -> Self {
        let mut path = Self::new(
            "test_path".to_string(),
            waypoints,
            Vector3::new(0.0, 1.0, 0.0), // Green color
        );

        // Add start marker (green)
        path.add_marker(PathMarker {
            position: start,
            marker_type: MarkerType::Start,
            color: Vector3::new(0.0, 1.0, 0.0),
        });

        // Add goal marker (red)
        path.add_marker(PathMarker {
            position: goal,
            marker_type: MarkerType::Goal,
            color: Vector3::new(1.0, 0.0, 0.0),
        });

        path
    }
}

/// A marker for visualizing important points along paths
#[derive(Clone, Debug)]
pub struct PathMarker {
    /// Position in world coordinates
    pub position: Vector3<f32>,
    /// Type of marker
    pub marker_type: MarkerType,
    /// Color for rendering this marker
    pub color: Vector3<f32>,
}

/// Types of path markers
#[derive(Clone, Debug, PartialEq)]
pub enum MarkerType {
    /// Starting position marker
    Start,
    /// Goal position marker
    Goal,
    /// Intermediate waypoint marker
    Waypoint,
}

/// Predefined colors for common path types
pub mod colors {
    use cgmath::Vector3;

    /// Green for test paths
    pub const TEST_PATH: Vector3<f32> = Vector3::new(0.0, 1.0, 0.0);

    /// Blue for player paths
    pub const PLAYER_PATH: Vector3<f32> = Vector3::new(0.0, 0.5, 1.0);

    /// Orange for AI paths
    pub const AI_PATH: Vector3<f32> = Vector3::new(1.0, 0.5, 0.0);

    /// Purple for patrol paths
    pub const PATROL_PATH: Vector3<f32> = Vector3::new(0.8, 0.0, 1.0);

    /// Green for start markers
    pub const START_MARKER: Vector3<f32> = Vector3::new(0.0, 1.0, 0.0);

    /// Red for goal markers
    pub const GOAL_MARKER: Vector3<f32> = Vector3::new(1.0, 0.0, 0.0);

    /// Yellow for waypoint markers
    pub const WAYPOINT_MARKER: Vector3<f32> = Vector3::new(1.0, 1.0, 0.0);
}
