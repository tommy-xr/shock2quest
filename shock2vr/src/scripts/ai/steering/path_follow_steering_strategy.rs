use cgmath::{Deg, EuclideanSpace, Vector3, vec4};
use dark::SCALE_FACTOR;
use dark::mission::path_database::MovementBits;
use rand::Rng;
use shipyard::{EntityId, UniqueView, World};

use crate::{
    mission::{GlobalPathfinding, PlayerInfo},
    pathfinding::{PathfindingFrameBudget, PathfindingService},
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
    /// Path to a fixed point (e.g. a last-known position). The owning
    /// behavior decides when "arrived" - this just keeps routing there.
    Point(Vector3<f32>),
}

/// A waypoint counts as reached within this XZ distance (2 Dark feet)
const WAYPOINT_ADVANCE_DISTANCE: f32 = 2.0 / SCALE_FACTOR;
/// ...and within this height difference (4 Dark feet): feet-vs-cell-floor
/// offsets are small, a waypoint on the floor above/below is not "reached"
const WAYPOINT_ADVANCE_HEIGHT: f32 = 4.0 / SCALE_FACTOR;
/// Re-path when a moving target strays this far from the path's goal
const REPATH_TARGET_DRIFT: f32 = 6.0 / SCALE_FACTOR;
/// Minimum seconds between A* queries per strategy instance (behaviors are
/// recreated on transitions, so a combat flip can re-path sooner - that's
/// fine, those paths are short)
const REPATH_COOLDOWN_SECONDS: f32 = 0.5;
/// After a failed pathfind, wait longer before retrying: failure is the
/// worst case (the search exhausts the reachable component, twice with the
/// stressed second pass) and rarely resolves within one cooldown
const REPATH_FAILURE_BACKOFF_SECONDS: f32 = 2.0;
/// Drop the path when we can't get closer to the current waypoint for this
/// long (blocked by a prop, another AI, or unreachable geometry)
const STALL_SECONDS: f32 = 3.0;
/// Progress smaller than this doesn't count toward un-stalling (jitter)
const STALL_PROGRESS_EPSILON: f32 = 0.25 / SCALE_FACTOR;

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
    /// Stall tracking: closest we've been to the current waypoint, and how
    /// long since that improved
    stall_waypoint: usize,
    stall_best: f32,
    stall_seconds: f32,
}

impl PathFollowSteeringStrategy {
    pub fn chase_player() -> PathFollowSteeringStrategy {
        PathFollowSteeringStrategy::new(PathTarget::Player)
    }

    pub fn wander(radius: f32) -> PathFollowSteeringStrategy {
        PathFollowSteeringStrategy::new(PathTarget::Wander { radius })
    }

    pub fn to_point(goal: Vector3<f32>) -> PathFollowSteeringStrategy {
        PathFollowSteeringStrategy::new(PathTarget::Point(goal))
    }

    fn new(target: PathTarget) -> PathFollowSteeringStrategy {
        PathFollowSteeringStrategy {
            target,
            path: Vec::new(),
            next_waypoint: 0,
            path_goal: None,
            repath_cooldown: 0.0,
            stall_waypoint: usize::MAX,
            stall_best: f32::INFINITY,
            stall_seconds: 0.0,
        }
    }

    fn clear_path(&mut self) {
        self.path.clear();
        self.next_waypoint = 0;
        self.path_goal = None;
        self.reset_stall();
    }

    fn reset_stall(&mut self) {
        self.stall_waypoint = usize::MAX;
        self.stall_best = f32::INFINITY;
        self.stall_seconds = 0.0;
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
            // Wander and Point keep their destination until the path completes
            PathTarget::Wander { .. } | PathTarget::Point(_) => None,
        };

        let needs_repath = match (self.path_goal, desired_goal) {
            (None, _) => true,
            (Some(prev), Some(now)) => xz_distance(prev, now) > REPATH_TARGET_DRIFT,
            (Some(_), None) => false,
        };

        // The frame budget bounds how many AIs can re-path in one frame: a
        // deferred AI keeps steering along its stale path (or falls through
        // the chain) and tries again next frame, its cooldown untouched.
        // (The unique is added alongside GlobalPathfinding, whose absence
        // already bailed above - the fallback is purely defensive.)
        let budget_available = || {
            world
                .borrow::<UniqueView<PathfindingFrameBudget>>()
                .map(|budget| budget.try_acquire())
                .unwrap_or(true)
        };

        // Zero-dt ticks are paused introspection updates (debug runtime) -
        // no pathfinding work there, so query counts stay deterministic
        // per stepped frame
        let advancing = time.elapsed.as_secs_f32() > 0.0;

        if advancing && needs_repath && self.repath_cooldown <= 0.0 {
            // Pick the goal before touching the budget: a failed (cheap)
            // wander goal pick must not consume a query slot
            let goal = match self.target {
                PathTarget::Player => desired_goal,
                PathTarget::Wander { radius } => {
                    pick_wander_goal(&service, position, radius, &mut rand::thread_rng())
                }
                PathTarget::Point(point) => Some(point),
            };
            match goal {
                // TODO: derive movement bits from the creature (small
                // creature / fly / swim) - everything walks for now
                Some(goal) if budget_available() => {
                    // Jitter the cooldown so AIs alerted in the same moment
                    // (e.g. an alarm or DebugAlertAll) don't re-path on the
                    // same frames forever
                    self.repath_cooldown =
                        REPATH_COOLDOWN_SECONDS * rand::thread_rng().gen_range(0.8..1.2);
                    match service.find_path(position, goal, MovementBits::WALK) {
                        Some(path) => {
                            self.path = path;
                            self.path_goal = Some(goal);
                            // waypoint 0 is our own position
                            self.next_waypoint = 1;
                            self.reset_stall();
                        }
                        None => {
                            self.clear_path();
                            // Failure exhausted the reachable component
                            // (twice, with the stressed retry) and won't
                            // resolve immediately - back off harder than the
                            // normal cooldown (jittered, as above)
                            self.repath_cooldown = REPATH_FAILURE_BACKOFF_SECONDS
                                * rand::thread_rng().gen_range(0.8..1.2);
                        }
                    }
                }
                // Budget exhausted: defer to a later frame, cooldown untouched
                Some(_) => {}
                None => {
                    self.clear_path();
                    // No goal to path to (e.g. every wander pick missed) -
                    // back off before sampling again
                    self.repath_cooldown =
                        REPATH_FAILURE_BACKOFF_SECONDS * rand::thread_rng().gen_range(0.8..1.2);
                }
            }
        }

        self.next_waypoint = advance_waypoint(position, &self.path, self.next_waypoint);

        // No route (or none yet) - let the next strategy in the chain steer
        let waypoint = *self.path.get(self.next_waypoint)?;

        // Stall escape: if we stop making progress toward the current
        // waypoint (blocked by a prop, another AI, or bad geometry), drop
        // the path so the next re-path - or wander goal - starts fresh
        // instead of pushing into the obstacle forever.
        let distance = xz_distance(position, waypoint);
        if self.next_waypoint != self.stall_waypoint
            || distance < self.stall_best - STALL_PROGRESS_EPSILON
        {
            self.stall_waypoint = self.next_waypoint;
            self.stall_best = distance;
            self.stall_seconds = 0.0;
        } else {
            self.stall_seconds += time.elapsed.as_secs_f32();
            if self.stall_seconds >= STALL_SECONDS {
                self.clear_path();
                return None;
            }
        }

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
    while index < path.len() && waypoint_reached(position, path[index]) {
        index += 1;
    }
    index
}

/// Reached = close in XZ *and* roughly at the same height. Feet-vs-floor
/// offsets are tolerated; a waypoint on a floor stacked above/below in XZ
/// (stairwells, walkways) is not reached just by standing under/over it.
fn waypoint_reached(position: Vector3<f32>, waypoint: Vector3<f32>) -> bool {
    xz_distance(position, waypoint) < WAYPOINT_ADVANCE_DISTANCE
        && (position.y - waypoint.y).abs() < WAYPOINT_ADVANCE_HEIGHT
}

/// Horizontal distance; AI position and waypoint heights differ slightly
/// (feet vs cell floor), so Y is checked separately with its own tolerance
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
    fn advance_tolerates_small_height_offsets() {
        // Feet vs cell-floor offset (well under 4 Dark feet) still reaches
        let path = vec![vec3(0.0, 0.5, 0.0), vec3(10.0, 0.0, 0.0)];
        assert_eq!(advance_waypoint(vec3(0.0, 0.0, 0.0), &path, 0), 1);
    }

    #[test]
    fn advance_does_not_reach_waypoints_on_other_floors() {
        // A waypoint 5 units directly overhead (a stacked floor) is NOT
        // reached by standing beneath it
        let path = vec![vec3(0.0, 5.0, 0.0), vec3(10.0, 5.0, 0.0)];
        assert_eq!(advance_waypoint(vec3(0.0, 0.0, 0.0), &path, 0), 0);
    }
}
