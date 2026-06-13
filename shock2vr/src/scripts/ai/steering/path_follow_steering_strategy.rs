use cgmath::{Deg, EuclideanSpace, Vector3, vec4};
use dark::SCALE_FACTOR;
use dark::mission::path_database::MovementBits;
use rand::Rng;
use shipyard::{EntityId, UniqueView, World};

use crate::{
    mission::{GlobalPathfinding, PlayerInfo},
    pathfinding::PathfindingService,
    physics::PhysicsWorld,
    scripts::{Effect, ai::ai_util},
    time::Time,
    util::vec3_to_point3,
};

use super::{Steering, SteeringOutput, SteeringStrategy};

/// How the strategy picks its destination
pub enum PathTarget {
    /// Follow the player, re-pathing as they move
    Player,
    /// Roam to random reachable points within a radius of the AI
    Wander { radius: f32 },
}

/// A waypoint counts as reached within this XZ distance (2 Dark feet)
const WAYPOINT_ADVANCE_DISTANCE: f32 = 2.0 / SCALE_FACTOR;
/// Re-path when a moving target strays this far from the path's goal
const REPATH_TARGET_DRIFT: f32 = 6.0 / SCALE_FACTOR;
/// Minimum seconds between A* queries per AI
const REPATH_COOLDOWN_SECONDS: f32 = 0.5;

/// Steers along a route computed by the PathfindingService, the counterpart
/// of the original engine's cAIPath following (Advance / UpdateTargetEdge in
/// aipath.cpp). Returns None when no pathfinding data or no route exists so
/// chained fallback strategies (e.g. direct chase) take over.
pub struct PathFollowSteeringStrategy {
    target: PathTarget,
    path: Vec<Vector3<f32>>,
    next_waypoint: usize,
    /// Goal position the current path was computed against
    path_goal: Option<Vector3<f32>>,
    repath_cooldown: f32,
}

impl PathFollowSteeringStrategy {
    pub fn chase_player() -> PathFollowSteeringStrategy {
        PathFollowSteeringStrategy::new(PathTarget::Player)
    }

    pub fn wander(radius: f32) -> PathFollowSteeringStrategy {
        PathFollowSteeringStrategy::new(PathTarget::Wander { radius })
    }

    fn new(target: PathTarget) -> PathFollowSteeringStrategy {
        PathFollowSteeringStrategy {
            target,
            path: Vec::new(),
            next_waypoint: 0,
            path_goal: None,
            repath_cooldown: 0.0,
        }
    }

    fn clear_path(&mut self) {
        self.path.clear();
        self.next_waypoint = 0;
        self.path_goal = None;
    }
}

impl SteeringStrategy for PathFollowSteeringStrategy {
    fn steer(
        &mut self,
        _current_heading: Deg<f32>,
        world: &World,
        _physics: &PhysicsWorld,
        entity_id: EntityId,
        time: &Time,
    ) -> Option<(SteeringOutput, Effect)> {
        let service = world
            .borrow::<UniqueView<GlobalPathfinding>>()
            .ok()?
            .0
            .clone()?;
        // Live transform, not PropPosition - the latter lags behind
        // animation-driven movement
        let (position_point, _forward) = ai_util::get_position_and_forward(world, entity_id);
        let position = position_point.to_vec();

        self.repath_cooldown = (self.repath_cooldown - time.elapsed.as_secs_f32()).max(0.0);

        // Path is exhausted - forget it so we re-path (chase) or pick a new
        // destination (wander)
        if self.next_waypoint >= self.path.len() {
            self.clear_path();
        }

        let desired_goal = match self.target {
            PathTarget::Player => Some(world.borrow::<UniqueView<PlayerInfo>>().ok()?.pos),
            // Wander keeps its current destination until the path completes
            PathTarget::Wander { .. } => None,
        };

        let needs_repath = match (self.path_goal, desired_goal) {
            (None, _) => true,
            (Some(prev), Some(now)) => xz_distance(prev, now) > REPATH_TARGET_DRIFT,
            (Some(_), None) => false,
        };

        if needs_repath && self.repath_cooldown <= 0.0 {
            self.repath_cooldown = REPATH_COOLDOWN_SECONDS;
            let goal = match self.target {
                PathTarget::Player => desired_goal,
                PathTarget::Wander { radius } => {
                    pick_wander_goal(&service, position, radius, &mut rand::thread_rng())
                }
            };
            match goal.and_then(|goal| {
                service
                    .find_path(position, goal, MovementBits::WALK)
                    .map(|path| (goal, path))
            }) {
                Some((goal, path)) => {
                    self.path = path;
                    self.path_goal = Some(goal);
                    // waypoint 0 is our own position
                    self.next_waypoint = 1;
                }
                None => self.clear_path(),
            }
        }

        self.next_waypoint = advance_waypoint(position, &self.path, self.next_waypoint);

        // No route (or none yet) - let the next strategy in the chain steer
        let waypoint = *self.path.get(self.next_waypoint)?;

        let mut lines = vec![(
            vec3_to_point3(position),
            vec3_to_point3(waypoint),
            vec4(0.2, 0.9, 1.0, 1.0),
        )];
        for pair in self.path[self.next_waypoint..].windows(2) {
            lines.push((
                vec3_to_point3(pair[0]),
                vec3_to_point3(pair[1]),
                vec4(0.2, 0.5, 1.0, 1.0),
            ));
        }

        Some((
            Steering::turn_to_point(vec3_to_point3(position), vec3_to_point3(waypoint)),
            Effect::DrawDebugLines { lines },
        ))
    }
}

/// Skip every waypoint already within reach, returning the new index
fn advance_waypoint(position: Vector3<f32>, path: &[Vector3<f32>], mut index: usize) -> usize {
    while index < path.len() && xz_distance(position, path[index]) < WAYPOINT_ADVANCE_DISTANCE {
        index += 1;
    }
    index
}

/// Horizontal distance; AI position and waypoint heights differ (feet vs
/// cell floor), so Y is ignored for "have we reached it" checks
fn xz_distance(a: Vector3<f32>, b: Vector3<f32>) -> f32 {
    let dx = a.x - b.x;
    let dz = a.z - b.z;
    (dx * dx + dz * dz).sqrt()
}

/// Pick a random pathable cell center within `radius` of `position` to roam
/// to. Reachability is verified by the find_path call that follows.
fn pick_wander_goal(
    service: &PathfindingService,
    position: Vector3<f32>,
    radius: f32,
    rng: &mut impl Rng,
) -> Option<Vector3<f32>> {
    let cells = &service.path_database.cells;
    if cells.is_empty() {
        return None;
    }
    for _ in 0..8 {
        let cell = &cells[rng.gen_range(0..cells.len())];
        let center = cell.center;
        let distance = xz_distance(position, center);
        if cell.flags.is_empty()
            && distance > WAYPOINT_ADVANCE_DISTANCE
            && distance <= radius
            && (position.y - center.y).abs() <= radius
        {
            return Some(center);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use cgmath::vec3;

    #[test]
    fn advance_skips_reached_waypoints() {
        let path = vec![
            vec3(0.0, 0.0, 0.0),
            vec3(0.1, 0.0, 0.0),
            vec3(0.2, 0.0, 0.0),
            vec3(10.0, 0.0, 0.0),
        ];
        // Standing at the origin: the first three are all within reach
        assert_eq!(advance_waypoint(vec3(0.0, 0.0, 0.0), &path, 1), 3);
        // The far waypoint is not
        assert_eq!(advance_waypoint(vec3(0.0, 0.0, 0.0), &path, 3), 3);
        // Index past the end stays put
        assert_eq!(advance_waypoint(vec3(0.0, 0.0, 0.0), &path, 4), 4);
    }

    #[test]
    fn advance_ignores_height_differences() {
        let path = vec![vec3(0.0, 5.0, 0.0), vec3(10.0, 0.0, 0.0)];
        // Waypoint directly overhead still counts as reached
        assert_eq!(advance_waypoint(vec3(0.0, 0.0, 0.0), &path, 0), 1);
    }
}
