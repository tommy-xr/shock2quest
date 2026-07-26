use cgmath::{Deg, EuclideanSpace, Vector3, vec4};
use dark::SCALE_FACTOR;
use dark::mission::path_database::MovementBits;
use rand::Rng;
use shipyard::{EntityId, UniqueView, World};

use crate::{
    mission::{GlobalAsyncPathfinding, GlobalPathfinding},
    pathfinding::{
        AiPathOutcome, PathfindingFrameBudget, PathfindingService, async_queries::PathQueryRequest,
    },
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
/// ...and within this height difference (7 Dark feet). Waypoint heights are
/// cell-FLOOR heights while an AI's position is its body center, which for
/// the tall grunt models sits ~4.2 Dark feet above the floor - the previous
/// 4-foot gate was at that boundary, so physics jitter could leave an AI
/// permanently "not arrived" at a waypoint it was standing on (running in
/// place through an endless stall/re-path loop). 7 feet clears the tallest
/// body-center offset while still rejecting waypoints on a stacked floor
/// (floor-to-floor separation is 10+ feet).
const WAYPOINT_ADVANCE_HEIGHT: f32 = 7.0 / SCALE_FACTOR;
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
/// After a stall, back out toward the previous waypoint for about this long
/// before re-pathing. Without the retreat, the fresh route is identical to
/// the one that just wedged (same start cell, same taut corners), so an AI
/// pressed against a door frame - or two AIs pressed against each other -
/// repeated the wedge forever (issue #481). Jittered per stall so mutually
/// blocking AIs unstick on different frames.
const STALL_RECOVERY_SECONDS: f32 = 0.8;
/// Progress smaller than this doesn't count toward un-stalling (jitter)
const STALL_PROGRESS_EPSILON: f32 = 0.25 / SCALE_FACTOR;
/// How far past the stalled waypoint (XZ) to probe for the cell on the far
/// side of the crossing when reporting a blocked link - just enough to step
/// off the shared edge without skipping a narrow destination cell (0.5
/// Dark feet)
const BLOCKED_PROBE_DISTANCE: f32 = 0.5 / SCALE_FACTOR;
/// Crowd separation: repel from living creatures within this radius (6 Dark
/// feet - about two body widths)
const SEPARATION_RADIUS: f32 = 6.0 / SCALE_FACTOR;
/// ...bending the aim point at most this far sideways (3 Dark feet). A cap
/// keeps separation a BIAS on the route, never a veto - a dense crowd can't
/// steer an AI backwards, it just bows its line around the neighbors
/// (issue #487).
const SEPARATION_MAX_OFFSET: f32 = 3.0 / SCALE_FACTOR;

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
    /// Active stall recovery: seconds left, and the point to back out toward
    recovery: Option<(f32, Vector3<f32>)>,
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
            recovery: None,
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
        // Off-thread query worker: queries are SUBMITTED here and their
        // results ADOPTED on a later frame, so A* runs in parallel with the
        // frame instead of inside it
        let async_pathfinding = world
            .borrow::<UniqueView<GlobalAsyncPathfinding>>()
            .ok()?
            .0
            .clone()?;
        // Live transform, not PropPosition - the latter lags behind
        // animation-driven movement
        let (position_point, _forward) = ai_util::get_position_and_forward(world, entity_id);
        let position = position_point.to_vec();

        self.repath_cooldown = (self.repath_cooldown - time.elapsed.as_secs_f32()).max(0.0);

        // Stall recovery: back out toward the previous waypoint (ground we
        // know we stood on) so the next route doesn't start from the wedged
        // pose and reproduce the wedge
        if let Some((seconds_left, retreat)) = self.recovery {
            let seconds_left = seconds_left - time.elapsed.as_secs_f32();
            if seconds_left <= 0.0 || xz_distance(position, retreat) < WAYPOINT_ADVANCE_DISTANCE {
                self.recovery = None;
            } else {
                self.recovery = Some((seconds_left, retreat));
                return Some((
                    Steering::turn_to_point(vec3_to_point3(position), vec3_to_point3(retreat)),
                    Effect::NoEffect,
                ));
            }
        }

        // Path is exhausted - forget it so we re-path (chase) or pick a new
        // destination (wander)
        if self.next_waypoint >= self.path.len() {
            self.clear_path();
        }

        let desired_goal = match self.target {
            // The last-known target position (frozen when sight breaks),
            // not the player's true location - breaking line of sight works
            PathTarget::Player => Some(ai_util::chase_target(world, entity_id)?),
            // Wander and Point keep their destination until the path completes
            PathTarget::Wander { .. } | PathTarget::Point(_) => None,
        };

        // Zero-dt ticks are paused introspection updates (debug runtime) -
        // adopt/submit only on advancing frames so stepped runs stay
        // reproducible frame-to-frame
        let advancing = time.elapsed.as_secs_f32() > 0.0;

        // Adopt a completed off-thread route before deciding whether to
        // re-path. A result whose goal has drifted too far from the current
        // desire is discarded (the submit below re-queries).
        if advancing {
            if let Some(response) = async_pathfinding.take_result(entity_id.inner()) {
                let goal_current = match desired_goal {
                    Some(now) => xz_distance(response.goal, now) <= REPATH_TARGET_DRIFT,
                    // Wander/Point requested this exact goal
                    None => true,
                };
                match response.outcome {
                    AiPathOutcome::Failed => {
                        self.clear_path();
                        // No route (even partially) - back off before asking
                        // again; the fallback chain is the worker's most
                        // expensive outcome
                        self.repath_cooldown =
                            REPATH_FAILURE_BACKOFF_SECONDS * rand::thread_rng().gen_range(0.8..1.2);
                    }
                    _ if goal_current => {
                        self.path = response.waypoints;
                        self.path_goal = Some(response.goal);
                        // waypoint 0 is the position the query started from
                        self.next_waypoint = 1;
                        self.reset_stall();
                    }
                    _ => {}
                }
            }
        }

        let needs_repath = match (self.path_goal, desired_goal) {
            (None, _) => true,
            (Some(prev), Some(now)) => xz_distance(prev, now) > REPATH_TARGET_DRIFT,
            (Some(_), None) => false,
        };

        // The frame budget bounds how many AIs can SUBMIT a query in one
        // frame (the worker serializes the actual searches): a deferred AI
        // keeps steering along its stale path and tries again next frame,
        // its cooldown untouched. (The unique is added alongside
        // GlobalPathfinding, whose absence already bailed above - the
        // fallback is purely defensive.)
        let budget_available = || {
            world
                .borrow::<UniqueView<PathfindingFrameBudget>>()
                .map(|budget| budget.try_acquire())
                .unwrap_or(true)
        };

        if advancing
            && needs_repath
            && self.repath_cooldown <= 0.0
            && !async_pathfinding.is_pending(entity_id.inner())
        {
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
                    // The worker computes a full route, or - when the goal
                    // is unreachable (another island, off-mesh) - a partial
                    // route to the closest reachable point, so the AI
                    // approaches instead of freezing against the nearest
                    // wall. The result is adopted (above) on a later frame;
                    // until then the current path keeps steering.
                    async_pathfinding.submit(PathQueryRequest {
                        entity: entity_id.inner(),
                        start: position,
                        goal,
                        movement_bits: MovementBits::WALK,
                        now_seconds: time.total.as_secs_f32(),
                    });
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

        service.record_ai_steering(
            entity_id.inner(),
            crate::pathfinding::AiSteeringDebug {
                next_waypoint: self.next_waypoint,
                path_len: self.path.len(),
                target: self.path.get(self.next_waypoint).copied(),
                stall_seconds: self.stall_seconds,
            },
        );

        // No route (or none yet) - let the next strategy in the chain steer
        let waypoint = *self.path.get(self.next_waypoint)?;

        // Crowd separation: bend the aim point away from nearby living
        // creatures so converging AIs pass around each other instead of
        // pushing capsule-to-capsule into a gridlock. The waypoint (and the
        // path) stay authoritative - the bias is capped well below the
        // waypoint spacing.
        let separation = ai_util::separation_bias(world, entity_id, position, SEPARATION_RADIUS);
        let aim = {
            let magnitude = (separation.x * separation.x + separation.z * separation.z).sqrt();
            if magnitude > 1e-3 {
                let capped = magnitude.min(SEPARATION_MAX_OFFSET);
                waypoint + separation * (capped / magnitude)
            } else {
                waypoint
            }
        };

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
                // Remember the crossing we could not traverse (TTL'd, per
                // AI): the mesh says the link is walkable but something
                // physical - a prop on the route, geometry the mesh doesn't
                // model - stopped us. Excluding that directed link from this
                // AI's next queries makes the post-stall re-path route
                // AROUND the obstacle; without this the fresh route is
                // identical and the stall/retreat/re-path cycle grinds
                // against the obstacle forever (issue #481). Reported even
                // when another creature is nearby: a "living blocker" can be
                // a scripted, stationary NPC (medsci1's FemaleMedsci crawl
                // scene) that never wanders off - suppressing the report for
                // it turned the retreat loop back into a permanent freeze
                // (measured). Mutual AI jams simply mark the contested
                // crossing on both sides and route apart; the TTL reopens it.
                // The probe steps just past the waypoint in the XZ plane at
                // the WAYPOINT's height (an edge-inset waypoint then
                // resolves to the cell beyond the crossing; keeping Y fixed
                // avoids blacklisting a stacked floor's cell).
                let toward = waypoint - position;
                let toward_len = (toward.x * toward.x + toward.z * toward.z).sqrt();
                if toward_len > 1e-3 {
                    let step = BLOCKED_PROBE_DISTANCE / toward_len;
                    let probe = Vector3::new(
                        waypoint.x + toward.x * step,
                        waypoint.y,
                        waypoint.z + toward.z * step,
                    );
                    let from = service.cell_from_position(position);
                    let to = service
                        .cell_from_position(probe)
                        .or_else(|| service.cell_from_position(waypoint));
                    if let (Some(from), Some(to)) = (from, to) {
                        if from != to {
                            service.report_blocked_link(
                                entity_id.inner(),
                                from,
                                to,
                                time.total.as_secs_f32(),
                            );
                        }
                    }
                }
                // ALWAYS back out toward the previous waypoint before
                // re-pathing, reported or not: the retreat both disengages
                // the body from whatever it wedged on (an AI boxed among
                // furniture that only re-paths in place never physically
                // frees itself - measured as a hard zero-movement freeze
                // when an immediate-re-path variant was tried) and staggers
                // mutually blocking AIs (jittered). The excluded crossing
                // then makes the fresh route different as well.
                let retreat = self
                    .path
                    .get(self.next_waypoint.saturating_sub(1))
                    .copied()
                    .filter(|p| xz_distance(position, *p) >= WAYPOINT_ADVANCE_DISTANCE)
                    .unwrap_or_else(|| {
                        let (_, forward) = ai_util::get_position_and_forward(world, entity_id);
                        position - forward * 2.0
                    });
                let jitter = rand::thread_rng().gen_range(0.8..1.6);
                self.recovery = Some((STALL_RECOVERY_SECONDS * jitter, retreat));
                self.clear_path();
                self.repath_cooldown = STALL_RECOVERY_SECONDS * jitter;
                return Some((
                    Steering::turn_to_point(vec3_to_point3(position), vec3_to_point3(retreat)),
                    Effect::NoEffect,
                ));
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
            Steering::turn_to_point(vec3_to_point3(position), vec3_to_point3(aim)),
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
        // Feet vs cell-floor offset (well under the gate) still reaches
        let path = vec![vec3(0.0, 0.5, 0.0), vec3(10.0, 0.0, 0.0)];
        assert_eq!(advance_waypoint(vec3(0.0, 0.0, 0.0), &path, 0), 1);
    }

    #[test]
    fn advance_tolerates_body_center_above_cell_floor() {
        // Waypoint heights are cell-floor heights; a tall creature's body
        // center rides ~1.7 units above them. Standing on the waypoint must
        // count as reached (the freeze in issue #481: it didn't, and the AI
        // ran in place through an endless stall/re-path loop).
        let path = vec![vec3(0.0, -1.7, 0.0), vec3(10.0, -1.7, 0.0)];
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
