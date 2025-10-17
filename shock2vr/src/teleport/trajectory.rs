use cgmath::{InnerSpace, Vector3};

/// Arc trajectory calculation for VR teleportation
#[derive(Clone, Debug)]
pub struct ArcTrajectory {
    /// Points along the arc trajectory
    pub points: Vec<Vector3<f32>>,
    /// Final landing position (if valid)
    pub landing_position: Option<Vector3<f32>>,
    /// Whether the landing position is valid
    pub is_valid: bool,
    /// Number of segments in the arc
    pub num_segments: usize,
}

impl ArcTrajectory {
    /// Calculate parabolic arc trajectory from start position and direction
    pub fn calculate(
        start_position: Vector3<f32>,
        start_direction: Vector3<f32>,
        initial_velocity: f32,
        gravity: f32,
        max_distance: f32,
        num_segments: usize,
        ground_height: f32,
    ) -> Self {
        let mut points = Vec::with_capacity(num_segments + 1);
        let mut landing_position = None;
        let mut is_valid = false;

        // Normalize the start direction
        let direction = start_direction.normalize();

        // Calculate initial velocity vector
        let velocity = direction * initial_velocity;

        // Time step for simulation
        let max_time = Self::calculate_max_flight_time(velocity, gravity, start_position.y, ground_height);
        let time_step = max_time / num_segments as f32;

        // Calculate arc points
        for i in 0..=num_segments {
            let t = i as f32 * time_step;
            let position = Self::calculate_position_at_time(start_position, velocity, gravity, t);

            // Check if we've hit the ground or gone too far
            if position.y <= ground_height {
                // Found ground intersection
                let corrected_position = Vector3::new(position.x, ground_height, position.z);

                // Check if within max distance
                let distance = (corrected_position - start_position).magnitude();
                if distance <= max_distance {
                    points.push(corrected_position);
                    landing_position = Some(corrected_position);
                    is_valid = Self::is_valid_landing_position(corrected_position, start_position);
                }
                break;
            }

            // Check max distance
            let distance = (position - start_position).magnitude();
            if distance > max_distance {
                break;
            }

            points.push(position);
        }

        ArcTrajectory {
            points,
            landing_position,
            is_valid,
            num_segments,
        }
    }

    /// Calculate position at specific time using kinematic equation
    fn calculate_position_at_time(
        start_position: Vector3<f32>,
        initial_velocity: Vector3<f32>,
        gravity: f32,
        time: f32,
    ) -> Vector3<f32> {
        Vector3::new(
            start_position.x + initial_velocity.x * time,
            start_position.y + initial_velocity.y * time - 0.5 * gravity * time * time,
            start_position.z + initial_velocity.z * time,
        )
    }

    /// Calculate maximum flight time (when projectile would hit ground level)
    fn calculate_max_flight_time(
        initial_velocity: Vector3<f32>,
        gravity: f32,
        start_height: f32,
        ground_height: f32,
    ) -> f32 {
        // Using quadratic formula to solve: start_height + v_y*t - 0.5*g*t^2 = ground_height
        // Rearranged: 0.5*g*t^2 - v_y*t + (start_height - ground_height) = 0

        let a = 0.5 * gravity;
        let b = -initial_velocity.y;
        let c = start_height - ground_height;

        let discriminant = b * b - 4.0 * a * c;

        if discriminant < 0.0 {
            // No real solution, trajectory never hits ground
            return 10.0; // Fallback time
        }

        let sqrt_discriminant = discriminant.sqrt();
        let t1 = (-b + sqrt_discriminant) / (2.0 * a);
        let t2 = (-b - sqrt_discriminant) / (2.0 * a);

        // Return the positive time that's reasonable
        if t1 > 0.0 && t2 > 0.0 {
            t1.max(t2)
        } else if t1 > 0.0 {
            t1
        } else if t2 > 0.0 {
            t2
        } else {
            10.0 // Fallback
        }
    }

    /// Check if landing position is valid for teleportation
    /// For Phase 2, this is a simple validation - will be enhanced in future phases
    fn is_valid_landing_position(landing_pos: Vector3<f32>, start_pos: Vector3<f32>) -> bool {
        // Basic validation rules for Phase 2:

        // 1. Must be on same level or reasonable height difference
        let height_diff = (landing_pos.y - start_pos.y).abs();
        if height_diff > 2.0 {
            return false;
        }

        // 2. Must be reasonable distance away (not too close)
        let horizontal_distance = Vector3::new(
            landing_pos.x - start_pos.x,
            0.0,
            landing_pos.z - start_pos.z,
        ).magnitude();

        if horizontal_distance < 1.0 {
            return false;
        }

        // 3. No obvious invalid coordinates (NaN, infinity)
        if !landing_pos.x.is_finite() || !landing_pos.y.is_finite() || !landing_pos.z.is_finite() {
            return false;
        }

        true
    }

    /// Get the arc length (useful for visual feedback)
    pub fn get_arc_length(&self) -> f32 {
        if self.points.len() < 2 {
            return 0.0;
        }

        let mut total_length = 0.0;
        for i in 1..self.points.len() {
            let segment_length = (self.points[i] - self.points[i - 1]).magnitude();
            total_length += segment_length;
        }

        total_length
    }

    /// Get trajectory point at normalized position (0.0 to 1.0)
    pub fn get_point_at_normalized_position(&self, t: f32) -> Option<Vector3<f32>> {
        if self.points.is_empty() {
            return None;
        }

        let t = t.clamp(0.0, 1.0);
        let index = (t * (self.points.len() - 1) as f32) as usize;

        if index >= self.points.len() {
            return self.points.last().copied();
        }

        if index == self.points.len() - 1 {
            return Some(self.points[index]);
        }

        // Linear interpolation between points
        let t_local = (t * (self.points.len() - 1) as f32) - index as f32;
        let p1 = self.points[index];
        let p2 = self.points[index + 1];

        Some(p1 + (p2 - p1) * t_local)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cgmath::Vector3;

    #[test]
    fn test_arc_calculation_basic() {
        let start_pos = Vector3::new(0.0, 2.0, 0.0);
        let direction = Vector3::new(0.0, 0.5, -1.0).normalize();
        let trajectory = ArcTrajectory::calculate(
            start_pos,
            direction,
            10.0,  // initial velocity
            9.8,   // gravity
            20.0,  // max distance
            20,    // segments
            0.0,   // ground height
        );

        assert!(!trajectory.points.is_empty());
        assert!(trajectory.landing_position.is_some());
        assert!(trajectory.is_valid);
    }

    #[test]
    fn test_arc_trajectory_physics() {
        let start_pos = Vector3::new(0.0, 5.0, 0.0);
        let direction = Vector3::new(1.0, 0.0, 0.0); // Horizontal throw
        let trajectory = ArcTrajectory::calculate(
            start_pos,
            direction,
            10.0,
            9.8,
            50.0,
            30,
            0.0,
        );

        // Should have points and land somewhere
        assert!(!trajectory.points.is_empty());
        assert!(trajectory.landing_position.is_some());

        // Landing position should be forward from start
        if let Some(landing) = trajectory.landing_position {
            assert!(landing.x > start_pos.x);
            assert_eq!(landing.y, 0.0); // Should land at ground height
        }
    }

    #[test]
    fn test_invalid_trajectory_too_close() {
        let start_pos = Vector3::new(0.0, 1.0, 0.0);
        let direction = Vector3::new(0.0, -1.0, 0.0); // Straight down
        let trajectory = ArcTrajectory::calculate(
            start_pos,
            direction,
            1.0,   // Low velocity
            9.8,
            20.0,
            20,
            0.0,
        );

        // Should land but be marked invalid (too close)
        assert!(trajectory.landing_position.is_some());
        assert!(!trajectory.is_valid); // Should be invalid due to distance
    }

    #[test]
    fn test_trajectory_max_distance() {
        let start_pos = Vector3::new(0.0, 1.0, 0.0);
        let direction = Vector3::new(1.0, 0.0, 0.0);
        let trajectory = ArcTrajectory::calculate(
            start_pos,
            direction,
            50.0,  // High velocity
            9.8,
            5.0,   // Small max distance
            20,
            0.0,
        );

        // Trajectory should be cut off by max distance
        let max_distance_reached = trajectory.points.iter()
            .any(|p| (p - start_pos).magnitude() > 5.0);
        assert!(!max_distance_reached);
    }

    #[test]
    fn test_get_arc_length() {
        let start_pos = Vector3::new(0.0, 2.0, 0.0);
        let direction = Vector3::new(1.0, 0.0, 0.0);
        let trajectory = ArcTrajectory::calculate(
            start_pos,
            direction,
            10.0,
            9.8,
            20.0,
            20,
            0.0,
        );

        let arc_length = trajectory.get_arc_length();
        assert!(arc_length > 0.0);
        assert!(arc_length < 50.0); // Should be reasonable
    }

    #[test]
    fn test_normalized_position_interpolation() {
        let start_pos = Vector3::new(0.0, 2.0, 0.0);
        let direction = Vector3::new(1.0, 0.0, 0.0);
        let trajectory = ArcTrajectory::calculate(
            start_pos,
            direction,
            10.0,
            9.8,
            20.0,
            10,
            0.0,
        );

        // Test various normalized positions
        assert!(trajectory.get_point_at_normalized_position(0.0).is_some());
        assert!(trajectory.get_point_at_normalized_position(0.5).is_some());
        assert!(trajectory.get_point_at_normalized_position(1.0).is_some());

        // Start should be close to original position
        if let Some(start_point) = trajectory.get_point_at_normalized_position(0.0) {
            let distance = (start_point - start_pos).magnitude();
            assert!(distance < 1.0);
        }
    }
}