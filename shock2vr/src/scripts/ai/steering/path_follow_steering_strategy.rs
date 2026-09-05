use cgmath::{Deg, EuclideanSpace, InnerSpace, Vector3, vec3, vec4};
use dark::SCALE_FACTOR;
use dark::mission::path_database::MovementBits;
use rand::Rng;
use shipyard::{EntityId, Get, UniqueView, View, World};

use crate::{
    mission::{GlobalAsyncPathfinding, GlobalPathfinding},
    pathfinding::{
        AiPathOutcome, MovementHold, PathfindingFrameBudget, PathfindingService,
        async_queries::PathQueryRequest,
    },
    physics::{InternalCollisionGroups, PhysicsWorld},
    scripts::{Effect, ai::ai_util},
    time::Time,
    util::vec3_to_point3,
};

use super::{Steering, SteeringOutput, SteeringStrategy, WhiskerAvoidance};

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
pub(crate) const STALL_SECONDS: f32 = 3.0;
/// After a stall, back out toward the previous waypoint for about this long
/// before re-pathing. Without the retreat, the fresh route is identical to
/// the one that just wedged (same start cell, same taut corners), so an AI
/// pressed against a door frame - or two AIs pressed against each other -
/// repeated the wedge forever (issue #481). Jittered per stall so mutually
/// blocking AIs unstick on different frames.
pub(crate) const STALL_RECOVERY_SECONDS: f32 = 0.8;
/// Progress smaller than this doesn't count toward un-stalling (jitter)
const STALL_PROGRESS_EPSILON: f32 = 0.25 / SCALE_FACTOR;
/// Displacement watchdog: with an active waypoint, failing to move this far
/// (XZ, 1 Dark foot) ...
const DISPLACEMENT_STALL_DISTANCE: f32 = 1.0 / SCALE_FACTOR;
/// ...within this long also counts as a stall. Micro-sliding around a
/// blocking capsule (another creature, a prop corner) can keep improving
/// the waypoint distance by more than the epsilon, resetting the progress
/// check forever while the body stays effectively in place - net
/// displacement is the ground truth (issue #481's last freeze pocket, an
/// AI pinned behind a scripted NPC beside a desk). Slightly longer than
/// STALL_SECONDS so the progress check stays the common path.
const DISPLACEMENT_STALL_SECONDS: f32 = 4.0;
/// Physical unstick, last resort: after TWO consecutive stalls with no
/// displacement between them - the body is PINNED (e.g. overlapping a
/// scripted NPC's capsule beside furniture; steering, retreats and
/// re-paths all command motion the solver cancels) - nudge the body this
/// far (2 Dark feet, about one body radius) toward the retreat point to
/// break the equilibrium. A rare small pop beats a monster frozen forever
/// (issue #481's terminal pocket).
const UNSTICK_NUDGE_DISTANCE: f32 = 2.0 / SCALE_FACTOR;
/// How far below a nudge landing to look for a floor (8 Dark feet - more
/// than a body's origin sits above its own floor)
const NUDGE_FLOOR_PROBE: f32 = 8.0 / SCALE_FACTOR;
/// ...and how far that floor may sit from the one the body is standing on
/// (2 Dark feet, a step). A landing over a ledge or a stairwell is refused.
const NUDGE_MAX_STEP: f32 = 2.0 / SCALE_FACTOR;
/// Height of the second (knee) probe ray, below the body's origin: the
/// whiskers' own knee fraction, so the two cannot drift apart. A fraction of
/// the creature's height rather than fixed feet, so a monkey's second ray is
/// not put underground by a hybrid's offset.
use super::whisker_avoidance::WHISKER_KNEE_FRACTION as NUDGE_KNEE_FRACTION;
/// A probe hit closer than this to its own origin means the ray STARTED
/// inside a collider (the queries are solid), which a body embedded in
/// geometry always does - and that body is exactly what the unstick is for,
/// so the probe steps this far past such a reading and looks again.
const NUDGE_EMBEDDED_DISTANCE: f32 = 0.1 / SCALE_FACTOR;
/// A loop guard on that stepping, so a ray inside a pile of colliders cannot
/// spin: a probe that has not got clear of geometry after this many steps
/// reports embedded, which refuses the nudge. Stepping only happens inside
/// geometry, which only a pinned body's rays are, and a nudge is attempted
/// about once a minute.
const NUDGE_EMBEDDED_STEPS: usize = 64;
/// How far past the stalled waypoint (XZ) to probe for the cell on the far
/// side of the crossing when reporting a blocked link - just enough to step
/// off the shared edge without skipping a narrow destination cell (0.5
/// Dark feet)
const BLOCKED_PROBE_DISTANCE: f32 = 0.5 / SCALE_FACTOR;
/// How many leading cells of the pre-stall route are remembered to check
/// whether the re-path actually produced a different route
const STALL_ROUTE_PREFIX: usize = 3;
/// How many times one stall incident may escalate (blacklist more of the
/// reproduced route) before falling through to the physical nudge. Bounded
/// so a genuinely one-way corridor can't be sealed link by link.
const MAX_STALL_ESCALATIONS: u32 = 3;
/// Crowd separation: repel from living creatures within this radius (6 Dark
/// feet - about two body widths)
const SEPARATION_RADIUS: f32 = 6.0 / SCALE_FACTOR;
/// ...bending the aim point at most this far sideways (3 Dark feet). A cap
/// keeps separation a BIAS on the route, never a veto - a dense crowd can't
/// steer an AI backwards, it just bows its line around the neighbors
/// (issue #487).
const SEPARATION_MAX_OFFSET: f32 = 3.0 / SCALE_FACTOR;
/// AI-to-AI repel (the original engine's object regulator): a neighbour
/// this close pushes back, ramping from nothing here...
const REPEL_RADIUS: f32 = 4.5 / SCALE_FACTOR;
/// ...to a full push at this distance. Crowd separation is not steep
/// enough near contact to unstack bodies - it fades linearly over its
/// whole six feet, so between two AIs already shoulder to shoulder it
/// barely changes as they close the last foot. This term does.
const REPEL_FULL_DISTANCE: f32 = 1.5 / SCALE_FACTOR;
/// The repel fades out as an AI closes on its target - off at
/// `MELEE_ATTACK_RANGE`, full again by this distance. A hard switch would
/// step the aim point sideways by the whole offset budget in the frame the
/// target drifted across the boundary.
const REPEL_MELEE_FADE_DISTANCE: f32 = 12.0 / SCALE_FACTOR;

/// A partial route counts as reaching its goal anyway when its last waypoint
/// lands within this distance (6 Dark feet). A* answers an unreachable goal
/// with a partial route to the closest reachable CELL CENTER, which lands
/// near a goal it merely could not resolve to a cell, but a whole room short
/// of one on another walk component - and following that to its end just
/// presses the body into whatever geometry sits in between.
const GOAL_REACHED_DISTANCE: f32 = 6.0 / SCALE_FACTOR;

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
    /// Displacement watchdog: where the AI was when the anchor was set, and
    /// how long ago (see DISPLACEMENT_STALL_SECONDS)
    displacement_anchor: Option<(Vector3<f32>, f32)>,
    /// Where the previous stall fired - a new stall from (nearly) the same
    /// spot means retreat + re-path freed nothing and the body is pinned
    /// (see UNSTICK_NUDGE_DISTANCE)
    last_stall_position: Option<Vector3<f32>>,
    /// Last query said the goal has no route (see `goal_unreachable`)
    goal_unreachable: bool,
    /// ...and, when that query ran against a live exclusion, when that
    /// exclusion expires (see `goal_unreachable_until`)
    goal_unreachable_until: Option<f32>,
    /// Static-geometry whiskers, biasing the aim point (see `aim_with_bias`)
    whiskers: WhiskerAvoidance,
    /// The leading CELLS of the route that was in force when the last stall
    /// fired, kept until the next route is adopted. Cells, not waypoints:
    /// waypoints are pulled taut backwards from the goal, so a chase whose
    /// goal moved a foot renames every one of them while the route is the
    /// same route. If the fresh route walks the same cells, the re-path
    /// reproduced the doomed route and walking it again just grinds -
    /// escalate instead (see `MAX_STALL_ESCALATIONS`).
    stall_route_cells: Option<Vec<u32>>,
    /// Escalations spent on the current stall incident; reset once the body
    /// actually moves
    stall_escalations: u32,
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
            displacement_anchor: None,
            last_stall_position: None,
            goal_unreachable: false,
            goal_unreachable_until: None,
            whiskers: WhiskerAvoidance::new(),
            stall_route_cells: None,
            stall_escalations: 0,
        }
    }

    fn clear_path(&mut self) {
        self.path.clear();
        self.next_waypoint = 0;
        self.path_goal = None;
        self.reset_stall();
    }

    /// Advance - or freeze - the two no-progress clocks for this frame, and
    /// answer whether the displacement watchdog has fired.
    ///
    /// A hold is a standstill the AI CHOSE (a door leaf still crossing the
    /// doorway, an authored pivot): it ends by itself, so neither clock moves
    /// through it and the held seconds cannot spend the patience kept for a
    /// real wedge. Only the accounting pauses - the caller keeps steering, so
    /// heading, whiskers and crowd repel stay live through the hold.
    fn advance_stall_clocks(
        &mut self,
        hold: MovementHold,
        position: Vector3<f32>,
        distance: f32,
        elapsed: f32,
    ) -> bool {
        if hold.is_holding() {
            // Both clocks freeze where they are. A body with no anchor yet
            // takes one here, so the displacement watchdog resumes from where
            // it stood rather than from wherever it is first seen moving.
            self.displacement_anchor.get_or_insert((position, 0.0));
            return false;
        }
        if self.next_waypoint != self.stall_waypoint
            || distance < self.stall_best - STALL_PROGRESS_EPSILON
        {
            self.stall_waypoint = self.next_waypoint;
            self.stall_best = distance;
            self.stall_seconds = 0.0;
        } else {
            self.stall_seconds += elapsed;
        }
        // Displacement watchdog: micro-sliding around a blocking capsule can
        // reset the waypoint-progress check above forever while the body
        // stays put - fall back to net displacement
        match self.displacement_anchor {
            Some((anchor, _)) if xz_distance(position, anchor) >= DISPLACEMENT_STALL_DISTANCE => {
                self.displacement_anchor = Some((position, 0.0));
                // Real movement: the next stall (if any) is a fresh incident,
                // not a continuation of a pinned body
                self.last_stall_position = None;
                self.stall_escalations = 0;
                false
            }
            Some((anchor, age)) => {
                let age = age + elapsed;
                self.displacement_anchor = Some((anchor, age));
                age >= DISPLACEMENT_STALL_SECONDS
            }
            None => {
                self.displacement_anchor = Some((position, 0.0));
                false
            }
        }
    }

    fn reset_stall(&mut self) {
        self.stall_waypoint = usize::MAX;
        self.stall_best = f32::INFINITY;
        self.stall_seconds = 0.0;
        self.displacement_anchor = None;
    }
}

impl SteeringStrategy for PathFollowSteeringStrategy {
    fn goal_unreachable(&self) -> bool {
        self.goal_unreachable
    }

    fn goal_unreachable_until(&self) -> Option<f32> {
        self.goal_unreachable_until
    }

    fn steer(
        &mut self,
        current_heading: Deg<f32>,
        world: &World,
        physics: &PhysicsWorld,
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
            let steering =
                Steering::turn_to_point(vec3_to_point3(position), vec3_to_point3(retreat));
            let heading_error =
                ai_util::clamp_to_minimal_delta_angle(steering.desired_heading - current_heading);
            let seconds_left =
                seconds_left - retreat_seconds_spent(heading_error, time.elapsed.as_secs_f32());
            if seconds_left <= 0.0 || xz_distance(position, retreat) < WAYPOINT_ADVANCE_DISTANCE {
                self.recovery = None;
            } else {
                self.recovery = Some((seconds_left, retreat));
                return Some((steering, Effect::NoEffect));
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
                let goal_current = answers_goal(&self.target, response.goal, desired_goal);
                match response.outcome {
                    AiPathOutcome::Failed if goal_current => {
                        self.goal_unreachable = true;
                        self.goal_unreachable_until = response.exclusion_expires_at;
                        self.clear_path();
                        // Nothing to compare a stalled route against
                        self.stall_route_cells = None;
                        // No route (even partially) - back off before asking
                        // again; the fallback chain is the worker's most
                        // expensive outcome
                        self.repath_cooldown =
                            REPATH_FAILURE_BACKOFF_SECONDS * rand::thread_rng().gen_range(0.8..1.2);
                    }
                    _ if goal_current => {
                        // Only a PARTIAL route says anything about
                        // reachability: it is A*'s "closest I could get".
                        self.goal_unreachable = response.outcome == AiPathOutcome::Partial
                            && !route_reaches_goal(&response.waypoints, response.goal);
                        self.goal_unreachable_until = self
                            .goal_unreachable
                            .then_some(response.exclusion_expires_at)
                            .flatten();
                        self.path = response.waypoints;
                        self.path_goal = Some(response.goal);
                        // waypoint 0 is the position the query started from
                        self.next_waypoint = 1;
                        self.reset_stall();
                        // A stall's re-path has to actually change the
                        // route. When it comes back starting exactly the
                        // way the stalled one did, walking it again just
                        // grinds on the same obstacle - blacklist more of
                        // it and ask once more (bounded, so a genuinely
                        // one-way corridor still gets walked and the
                        // existing nudge remains the last resort).
                        if let Some(stalled) = self.stall_route_cells.take() {
                            let fresh =
                                route_cells(&service, position, &self.path[self.next_waypoint..]);
                            let repeats = repeats_route(&stalled, &fresh);
                            tracing::debug!(
                                "ai {:?}: repath adopted, cells {:?} -> {:?}, repeats={}",
                                entity_id,
                                stalled,
                                fresh,
                                repeats
                            );
                            if self.stall_escalations < MAX_STALL_ESCALATIONS && repeats {
                                tracing::debug!(
                                    "ai {:?}: escalate #{} on the reproduced route",
                                    entity_id,
                                    self.stall_escalations + 1
                                );
                                // The cell penalty alone was outbid. Cut the
                                // crossing this route opens with, so the next
                                // query cannot answer with it again - one per
                                // escalation, or a single creature's bad
                                // minute would seal a corridor for everyone.
                                self.stall_escalations += 1;
                                report_stall(&service, &fresh, time.total.as_secs_f32());
                                self.clear_path();
                                self.repath_cooldown = REPATH_COOLDOWN_SECONDS
                                    * rand::thread_rng().gen_range(0.8..1.2);
                            }
                        }
                    }
                    _ => {
                        // A discarded result is no comparison either
                        self.stall_route_cells = None;
                    }
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
                    // A fresh question: the previous answer's verdict on
                    // reachability no longer stands (goals move, and blocked
                    // crossings expire)
                    self.goal_unreachable = false;
                    self.goal_unreachable_until = None;
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

        // Crowd bias: bend the aim point away from nearby living creatures
        // so converging AIs pass around each other instead of pushing
        // capsule-to-capsule into a gridlock. The waypoint (and the path)
        // stay authoritative - the bias is capped well below the waypoint
        // spacing.
        //
        // The near-field repel fades out as we close on the chase target:
        // an attacker in reach of the player is not crowding, and pushing
        // it off the target would cost it the blow.
        // Only an AI that actually HAS a target fades: `chase_target`
        // answers with the player's true position for one that has never
        // seen them, and a patrol passing a wall away from the player must
        // still repel.
        let strength = match target_awareness_distance(world, entity_id, position) {
            Some(distance) => {
                1.0 - ai_util::repel_ramp(
                    distance,
                    ai_util::MELEE_ATTACK_RANGE,
                    REPEL_MELEE_FADE_DISTANCE,
                )
            }
            None => 1.0,
        };
        let repel = (strength > 0.0).then_some(ai_util::CrowdRepel {
            full: REPEL_FULL_DISTANCE,
            none: REPEL_RADIUS,
            strength,
        });
        let crowd = ai_util::crowd_bias(world, entity_id, position, SEPARATION_RADIUS, repel);
        // ...and the same treatment for static geometry the navigation mesh
        // doesn't model (a railing, a crate left on the route): whiskers bend
        // the line around it before the body wedges, without ever taking the
        // heading away from the route. Both together are capped ONCE, so two
        // biases pointing the same way still cannot bend the line further
        // than one of them may.
        let whiskers = self.whiskers.update(world, physics, entity_id, time);
        // A crowd squarely ahead pushes straight back down the route, which
        // the aim point discards - so it becomes a sidestep instead, on the
        // open side when the whiskers have found a wall. Faded with the
        // melee approach exactly like the repel it mostly comes from: an AI
        // in reach of its target must not be stepped off the blow.
        let heading = waypoint - position;
        let yielded = ai_util::yield_sideways(crowd, heading, whiskers);
        let crowd = crowd + (yielded - crowd) * strength;
        let aim = aim_with_bias(
            position,
            waypoint,
            blend_biases(crowd, whiskers, heading, SEPARATION_MAX_OFFSET),
        );

        // Stall escape: if we stop making progress toward the current
        // waypoint (blocked by a prop, another AI, or bad geometry), drop
        // the path so the next re-path - or wander goal - starts fresh
        // instead of pushing into the obstacle forever.
        let distance = xz_distance(position, waypoint);
        let displaced_stall = self.advance_stall_clocks(
            service.movement_hold(entity_id.inner()),
            position,
            distance,
            time.elapsed.as_secs_f32(),
        );
        {
            if self.stall_seconds >= STALL_SECONDS || displaced_stall {
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
                let now_seconds = time.total.as_secs_f32();
                tracing::debug!(
                    "ai {:?}: stall ({}) at {:.2},{:.2},{:.2} cell={:?} wp[{}]={:.2},{:.2},{:.2} d={:.2}",
                    entity_id,
                    if displaced_stall {
                        "displacement"
                    } else {
                        "no-progress"
                    },
                    position.x,
                    position.y,
                    position.z,
                    service.cell_from_position(position),
                    self.next_waypoint,
                    waypoint.x,
                    waypoint.y,
                    waypoint.z,
                    distance
                );
                let toward = waypoint - position;
                let toward_len = (toward.x * toward.x + toward.z * toward.z).sqrt();
                let crossing_into = if toward_len > 1e-3 {
                    let step = BLOCKED_PROBE_DISTANCE / toward_len;
                    let probe = Vector3::new(
                        waypoint.x + toward.x * step,
                        waypoint.y,
                        waypoint.z + toward.z * step,
                    );
                    service
                        .cell_from_position(probe)
                        .or_else(|| service.cell_from_position(waypoint))
                } else {
                    None
                };
                // The route ahead, as cells: what a stall can name, and
                // what the re-path is later checked against.
                let route = route_cells(&service, position, &self.path[self.next_waypoint..]);
                // The probe cell is the better blame when the body reached
                // the edge; a stall short of any edge (the medsci2 Science
                // door jamb - the body presses into the jamb feet before the
                // boundary) has only the route to go on.
                let route = match crossing_into.filter(|to| Some(*to) != route.first().copied()) {
                    Some(to)
                        if route
                            .first()
                            .is_some_and(|from| service.has_link(*from, to)) =>
                    {
                        vec![route[0], to]
                    }
                    _ => route,
                };
                tracing::debug!("ai {:?}: blocked report cells={:?}", entity_id, route);
                report_stall(&service, &route, now_seconds);
                // Remember how the route that failed began, so the re-path
                // can be checked against it when it lands
                self.stall_route_cells = Some(route);
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
                // Pinned-body unstick (last resort): the previous stall
                // fired from (nearly) this same spot, so its retreat and
                // re-path freed nothing - physically nudge toward the
                // retreat point (known-walkable route ground) to break the
                // solver equilibrium
                let pinned = self
                    .last_stall_position
                    .map(|prev| xz_distance(position, prev) < DISPLACEMENT_STALL_DISTANCE)
                    .unwrap_or(false);
                self.last_stall_position = Some(position);
                tracing::debug!(
                    "ai {:?}: backout to {:.2},{:.2},{:.2} (heading {:.2},{:.2}) pinned={}",
                    entity_id,
                    retreat.x,
                    retreat.y,
                    retreat.z,
                    retreat.x - position.x,
                    retreat.z - position.z,
                    pinned
                );
                let unstick_effect = if pinned {
                    // A nudge is a teleport, so both the trip and the landing
                    // are probed (see `probe_nudge`): an unchecked one aimed
                    // through a wall drops the body out of the level entirely.
                    // Candidates, best first - a step toward the retreat
                    // point (the previous waypoint, or simply backwards when
                    // there is none), then a step toward the middle of the
                    // cell the body is in.
                    let cell = service.cell_from_position(position);
                    let step_toward = |target: Vector3<f32>| {
                        let len = xz_distance(target, position);
                        (len > 1e-3).then(|| {
                            let step = UNSTICK_NUDGE_DISTANCE.min(len);
                            Vector3::new(
                                position.x + (target.x - position.x) / len * step,
                                position.y,
                                position.z + (target.z - position.z) / len * step,
                            )
                        })
                    };
                    let cell_center = cell
                        .and_then(|cell| service.path_database.cells.get(cell as usize))
                        .map(|cell| cell.center);
                    let landing = [Some(retreat), cell_center]
                        .into_iter()
                        .flatten()
                        .filter_map(step_toward)
                        .find_map(|candidate| {
                            // A body ALREADY off the mesh is the case the
                            // unstick exists for - there is no cell to keep it
                            // in - but it still has to reach the landing and
                            // stand there, so only THIS check is waived.
                            let navigable =
                                cell.is_none() || on_mesh(&service, candidate).is_some();
                            let probe = probe_nudge(
                                world, physics, entity_id, position, candidate, navigable,
                            );
                            // The landing the probes were taken at - the
                            // candidate riding the floor it lands on, so a
                            // step up does not bury the capsule in it.
                            nudge_is_safe(&probe).then_some(probe.landing)
                        });
                    match landing {
                        Some(landing) => {
                            tracing::debug!(
                                "ai {:?}: nudge {:.2},{:.2} -> {:.2},{:.2}",
                                entity_id,
                                position.x,
                                position.z,
                                landing.x,
                                landing.z
                            );
                            Effect::SetPositionRotation {
                                entity_id,
                                position: landing,
                                rotation: crate::util::get_rotation_from_transform(
                                    world, entity_id,
                                ),
                            }
                        }
                        None => {
                            // Re-path is the only recovery left; grinding in
                            // place beats a body shoved through a wall.
                            tracing::debug!("ai {:?}: nudge refused, no safe landing", entity_id);
                            Effect::NoEffect
                        }
                    }
                } else {
                    Effect::NoEffect
                };
                let jitter = rand::thread_rng().gen_range(0.8..1.6);
                self.recovery = Some((STALL_RECOVERY_SECONDS * jitter, retreat));
                self.clear_path();
                self.repath_cooldown = STALL_RECOVERY_SECONDS * jitter;
                return Some((
                    Steering::turn_to_point(vec3_to_point3(position), vec3_to_point3(retreat)),
                    unstick_effect,
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

/// How much of the retreat budget this frame spends. A body that wedged was
/// pushing AT the obstacle, so backing out starts with a half-turn it cannot
/// walk through - locomotion is zero above a right angle of heading error -
/// and a plain wall clock would spend most of the window pivoting on the
/// spot, leaving the body still in contact when the re-path fires. Spending
/// the budget only while the body can actually move keeps
/// `STALL_RECOVERY_SECONDS` worth of real backing out.
fn retreat_seconds_spent(heading_error: Deg<f32>, elapsed: f32) -> f32 {
    if crate::scripts::ai::animated_monster_ai::locomotion_scale_for_heading_error(heading_error)
        > 0.0
    {
        elapsed
    } else {
        0.0
    }
}

/// How far the AI's believed target is, or None when it has no target at
/// all (nothing has ever published awareness for it).
fn target_awareness_distance(
    world: &World,
    entity_id: EntityId,
    position: Vector3<f32>,
) -> Option<f32> {
    let v_awareness = world
        .borrow::<View<crate::runtime_props::RuntimePropAITargetAwareness>>()
        .ok()?;
    let awareness = v_awareness.get(entity_id).ok()?;
    Some(xz_distance(position, awareness.last_known_pos))
}

/// Combine the crowd bias with the whisker bias into one aim offset.
///
/// The crowd term is a sum of per-neighbour weights with no bound of its
/// own, so it is shortened to the offset budget BEFORE the sum: otherwise
/// a dense crowd - or simply a stronger repel term - buys magnitude out of
/// the whiskers' share and bends the line into the very geometry the
/// whiskers are there to avoid.
///
/// Geometry then outranks the crowd outright: whatever part of the crowd
/// push opposes the whiskers is dropped before the sum, so a neighbour on
/// the open side can never bid an AI back toward the wall the whiskers are
/// steering it off. (Two vectors of similar size pointing opposite ways
/// otherwise cancel to nothing, which is the worst of both - no avoidance
/// and no yielding.) That rule works along the whiskers' own direction; the
/// same priority is then applied a second time along the LATERAL axis,
/// which is the only one the aim point keeps, so a crowd term perpendicular
/// to the whiskers cannot cancel their sidestep either. The blend is
/// shortened once at the end, so two biases pointing the same way still
/// cannot bend the line further than one may.
fn blend_biases(
    crowd: Vector3<f32>,
    whiskers: Vector3<f32>,
    heading: Vector3<f32>,
    max: f32,
) -> Vector3<f32> {
    let crowd = capped(ai_util::drop_opposing(crowd, whiskers), max);
    // ...and once more along the lateral axis, because that is the only part
    // the aim point's forward projection (`aim_with_bias`) keeps: a diagonal
    // wall hit and a neighbour off to one side can be perpendicular - so
    // neither opposes the other and the rule above passes them both - while
    // their sideways parts still cancel, leaving a sum that is purely
    // backward and is erased whole.
    let wall_side = {
        let side = ai_util::lateral_axis(heading);
        side * (whiskers.x * side.x + whiskers.z * side.z)
    };
    let crowd = ai_util::drop_opposing(crowd, wall_side);
    capped(crowd + whiskers, max)
}

/// Shorten a horizontal bias to at most `max`.
fn capped(bias: Vector3<f32>, max: f32) -> Vector3<f32> {
    let magnitude = (bias.x * bias.x + bias.z * bias.z).sqrt();
    if magnitude > max {
        bias * (max / magnitude)
    } else {
        bias
    }
}

/// Bend the aim point by a steering bias, keeping the route authoritative:
/// a bias that would drag the aim point onto (or behind) the body is
/// dropped, since turning to a point you are standing on is not steering.
fn aim_with_bias(
    position: Vector3<f32>,
    waypoint: Vector3<f32>,
    bias: Vector3<f32>,
) -> Vector3<f32> {
    // Drop whatever part of the bias points back down the route: an
    // obstacle square across the path answers with its own normal, and
    // pulling the aim point backwards would only stop the AI in front of it
    // instead of taking it around (what is left is the sideways part).
    let bias = ai_util::drop_opposing(bias, waypoint - position);
    let aim = waypoint + bias;
    if xz_distance(position, aim) < WAYPOINT_ADVANCE_DISTANCE {
        waypoint
    } else {
        aim
    }
}

/// Whether a completed query answers the goal this strategy wants *now*.
/// Queries are keyed by entity, so a result can outlive the strategy that
/// asked for it - a patrol that gives up on a point builds a fresh follower
/// while the old query is still in flight. Adopting that answer would steer
/// the body along the abandoned point's route and, worse, carry its
/// reachability verdict over to a point nothing ever asked about.
fn answers_goal(
    target: &PathTarget,
    response_goal: Vector3<f32>,
    desired_goal: Option<Vector3<f32>>,
) -> bool {
    match (target, desired_goal) {
        // A moving target (the player): the answer is current if it was
        // computed near where the target is now
        (_, Some(now)) => xz_distance(response_goal, now) <= REPATH_TARGET_DRIFT,
        // A fixed point is requested verbatim and echoed back verbatim, so
        // any other goal belongs to a request this strategy did not make
        (PathTarget::Point(point), None) => response_goal == *point,
        // Wander picks its goal inside this strategy, so any answer is its own
        (_, None) => true,
    }
}

/// What one nudge candidate's probes found (see `probe_nudge`).
struct NudgeProbe {
    /// The landing is on a navigable cell (or the body is already off the
    /// mesh, where no cell can be demanded of it)
    navigable: bool,
    /// Nothing solid stands between the body and the landing
    path_clear: bool,
    /// The body has room to either side at the landing
    clearance: bool,
    /// The landing has room above its floor for the body: a creature's
    /// height, or at least as much as the spot it is leaving has
    headroom: bool,
    /// How far the landing's floor sits from the one the body stands on;
    /// None when either has no floor beneath it
    floor_step: Option<f32>,
    /// The landing the probes were actually taken at: the candidate riding
    /// the floor it lands on, which is where the body would stand.
    landing: Vector3<f32>,
}

/// A nudge candidate is only taken when every probe passes and its floor is
/// within a step of the body's own.
fn nudge_is_safe(probe: &NudgeProbe) -> bool {
    probe.navigable
        && probe.path_clear
        && probe.clearance
        && probe.headroom
        && probe
            .floor_step
            .is_some_and(|step| step.abs() <= NUDGE_MAX_STEP)
}

/// One probe ray's verdict.
enum ProbeRay {
    /// Nothing solid on the segment
    Clear,
    /// Something solid, at this reading
    Blocked(crate::physics::RayCastResult),
    /// The ray never got clear of whatever the body is embedded in
    Embedded,
}

/// Probe a nudge candidate: can the body get there, is there room, and is
/// there a floor within a step? The probe is a set of rays, not a sweep of
/// the body's capsule - the physics view a script holds offers no shape cast
/// - so it is an approximation of clearance: two heights (origin and knee,
/// as fractions of the creature's OWN height, so a monkey is not probed like
/// a hybrid) each way at the creature's own body radius, plus a creature's
/// height of headroom above the landing's floor. Geometry that fits between
/// those rays is not seen.
fn probe_nudge(
    world: &World,
    physics: &PhysicsWorld,
    entity_id: EntityId,
    from: Vector3<f32>,
    to: Vector3<f32>,
    navigable: bool,
) -> NudgeProbe {
    // Static geometry and props, not living bodies: being pressed against a
    // neighbour is exactly what the unstick exists for, and its capsule is
    // not a wall.
    let groups = InternalCollisionGroups::ALL_COLLIDABLE
        - InternalCollisionGroups::PLAYER
        - InternalCollisionGroups::ACTOR;
    // The queries are solid, so a ray whose origin lies inside a collider -
    // which a pinned body's does - reads a hit at zero range. Such a reading
    // is not evidence of a clear segment and not evidence of an obstacle
    // either: step past it and judge the REST of the segment, so a wall
    // behind the prop the body is embedded in is still seen. A segment spent
    // entirely inside geometry is reported embedded (which refuses) rather
    // than clear.
    let cast = |origin: Vector3<f32>, direction: Vector3<f32>, distance: f32| {
        let mut origin = origin;
        let mut remaining = distance;
        for step in 0..NUDGE_EMBEDDED_STEPS {
            if remaining <= 0.0 {
                // Stepping consumed the whole segment without ever getting
                // clear of geometry: the body is inside something for the
                // length of the probe, which is not a clear ray.
                return if step == 0 {
                    ProbeRay::Clear
                } else {
                    ProbeRay::Embedded
                };
            }
            let Some(hit) = physics.ray_cast2_as_actor(
                vec3_to_point3(origin),
                direction,
                remaining,
                groups,
                Some(entity_id),
                true,
            ) else {
                return ProbeRay::Clear;
            };
            let range = (hit.hit_point - vec3_to_point3(origin)).magnitude();
            if range > NUDGE_EMBEDDED_DISTANCE {
                return ProbeRay::Blocked(hit);
            }
            let advance = range + NUDGE_EMBEDDED_DISTANCE;
            origin += direction * advance;
            remaining -= advance;
        }
        ProbeRay::Embedded
    };
    let clear = |origin: Vector3<f32>, direction: Vector3<f32>, distance: f32| {
        matches!(cast(origin, direction, distance), ProbeRay::Clear)
    };
    let floor = |at: Vector3<f32>| match cast(at, vec3(0.0, -1.0, 0.0), NUDGE_FLOOR_PROBE) {
        ProbeRay::Blocked(hit) => Some(hit.hit_point.y),
        _ => None,
    };
    let height = ai_util::creature_height(world, entity_id);
    // Open space above a floor, up to a creature's height (which is all the
    // room that can matter).
    let headroom_above = |x: f32, z: f32, floor_y: f32| match cast(
        Vector3::new(x, floor_y + NUDGE_EMBEDDED_DISTANCE, z),
        vec3(0.0, 1.0, 0.0),
        height,
    ) {
        ProbeRay::Clear => height,
        ProbeRay::Blocked(hit) => hit.hit_point.y - floor_y,
        ProbeRay::Embedded => 0.0,
    };
    let radius = ai_util::creature_radius(world, entity_id);
    // The floors come FIRST: an accepted landing rides its own floor, so
    // that - not the body's current height - is where clearance and headroom
    // have to be measured.
    let standing_floor = floor(from);
    let landing_floor = floor(to);
    let floor_step = landing_floor
        .zip(standing_floor)
        .map(|(landing, standing)| landing - standing);
    let landing = Vector3::new(to.x, to.y + floor_step.unwrap_or(0.0), to.z);
    // Origin and knee - a low slab (a railing, a kerb) passes under a single
    // centre ray, the same reason the whiskers probe two heights.
    let heights = |at: Vector3<f32>| {
        [
            at,
            Vector3::new(at.x, at.y - height * NUDGE_KNEE_FRACTION, at.z),
        ]
    };
    let distance = xz_distance(to, from);
    if distance <= NUDGE_EMBEDDED_DISTANCE {
        // Nowhere to go: refuse rather than normalize a zero vector
        return NudgeProbe {
            navigable,
            path_clear: false,
            clearance: false,
            headroom: false,
            floor_step: None,
            landing,
        };
    }
    let direction = vec3((to.x - from.x) / distance, 0.0, (to.z - from.z) / distance);
    let side = vec3(-direction.z, 0.0, direction.x);
    NudgeProbe {
        navigable,
        // Out PAST the landing by the body's own room, so a wall just beyond
        // it refuses the nudge as well
        path_clear: heights(from)
            .into_iter()
            .all(|origin| clear(origin, direction, distance + radius)),
        clearance: heights(landing).into_iter().all(|origin| {
            [side, -side]
                .into_iter()
                .all(|direction| clear(origin, direction, radius))
        }),
        // Comparative, not absolute: a nudge moves two feet, so the landing
        // shares its ceiling with where the body already is. Demanding a full
        // standing height would refuse every candidate for a body pinned in a
        // low pocket - the population the unstick exists for - so the landing
        // only has to be no tighter than the spot being left.
        headroom: landing_floor.zip(standing_floor).is_some_and(
            |(landing_floor, standing_floor)| {
                let landing_room = headroom_above(landing.x, landing.z, landing_floor);
                landing_room >= height
                    || landing_room + NUDGE_EMBEDDED_DISTANCE
                        >= headroom_above(from.x, from.z, standing_floor)
            },
        ),
        floor_step,
        landing,
    }
}

/// `point` if it sits on a navigable cell, else None - a teleport target
/// off the mesh is a body dropped out of the level.
fn on_mesh(
    service: &crate::pathfinding::PathfindingService,
    point: Vector3<f32>,
) -> Option<Vector3<f32>> {
    service.cell_from_position(point).map(|_| point)
}

/// The leading cells a route walks, starting with the one `position` is in.
/// Consecutive waypoints inside one cell collapse to a single entry, so this
/// is the route's shape on the mesh rather than its geometry - two queries a
/// second apart at a moving goal name different waypoints for the same cells.
fn route_cells(
    service: &crate::pathfinding::PathfindingService,
    position: Vector3<f32>,
    waypoints: &[Vector3<f32>],
) -> Vec<u32> {
    let mut cells = Vec::new();
    for point in std::iter::once(&position).chain(waypoints) {
        let Some(cell) = service.cell_from_position(*point) else {
            continue;
        };
        if cells.last() != Some(&cell) {
            cells.push(cell);
        }
        if cells.len() > STALL_ROUTE_PREFIX {
            break;
        }
    }
    cells
}

/// Whether a fresh route walks the cells the stalled one did. A contiguous
/// match anywhere, not just at the head: backing out of the wedge can leave
/// the body a cell short of where it stalled, which shifts the sequence
/// without changing the route.
fn repeats_route(stalled: &[u32], fresh: &[u32]) -> bool {
    !stalled.is_empty() && fresh.windows(stalled.len()).any(|window| window == stalled)
}

/// Blacklist what a route the AI could not walk can be blamed on: the cell
/// it is standing in (a stall inside one cell has nothing else to name), and
/// the first crossing along the route that is a real mesh link. Only real
/// links: a pair named from waypoints need not be one, and an inert entry
/// would hold one of the bounded blacklist slots while excluding nothing.
fn report_stall(service: &crate::pathfinding::PathfindingService, route: &[u32], now_seconds: f32) {
    let Some(&from) = route.first() else {
        return;
    };
    service.report_blocked_cell(from, now_seconds);
    if let Some((from, to)) = route
        .windows(2)
        .map(|pair| (pair[0], pair[1]))
        .find(|&(from, to)| service.has_link(from, to))
    {
        service.report_blocked_link(from, to, now_seconds);
    }
}

/// Whether a route actually arrives at the goal it was computed for. An
/// empty route arrives nowhere.
fn route_reaches_goal(waypoints: &[Vector3<f32>], goal: Vector3<f32>) -> bool {
    waypoints
        .last()
        .map(|last| {
            // Height matters as much as ground distance here: the case this
            // exists for is a goal on the floor below, whose XZ distance is
            // nearly zero.
            xz_distance(*last, goal) <= GOAL_REACHED_DISTANCE
                && (last.y - goal.y).abs() <= GOAL_REACHED_DISTANCE
        })
        .unwrap_or(false)
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

    /// Stand `seconds` in one spot, `distance` from the waypoint, under
    /// `hold`. Answers whether the displacement watchdog fired.
    fn stand_still(
        follower: &mut PathFollowSteeringStrategy,
        hold: MovementHold,
        seconds: f32,
    ) -> bool {
        let mut displaced = false;
        for _ in 0..(seconds / 0.1).round() as u32 {
            displaced |= follower.advance_stall_clocks(hold, vec3(0.0, 0.0, 0.0), 5.0, 0.1);
        }
        displaced
    }

    /// The whole point of a hold: a door leaf the AI is standing off from is
    /// not a wedge, so the seconds it costs must not spend the patience the
    /// follower keeps for one. Waiting is bounded well under the stall window,
    /// but the clock the wait would land in already contains time.
    #[test]
    fn a_door_wait_never_advances_the_stall_clock() {
        // Unheld, standing still IS the stall the follower is looking for.
        let mut follower = PathFollowSteeringStrategy::to_point(vec3(5.0, 0.0, 0.0));
        let displaced = stand_still(
            &mut follower,
            MovementHold::None,
            DISPLACEMENT_STALL_SECONDS + 1.0,
        );
        assert!(follower.stall_seconds >= STALL_SECONDS, "control");
        assert!(displaced, "control: the displacement watchdog fires too");

        let mut follower = PathFollowSteeringStrategy::to_point(vec3(5.0, 0.0, 0.0));
        let displaced = stand_still(
            &mut follower,
            MovementHold::DoorWait,
            DISPLACEMENT_STALL_SECONDS + 1.0,
        );
        assert_eq!(
            follower.stall_seconds, 0.0,
            "a door wait is not no-progress"
        );
        assert!(!displaced, "nor is it displacement");
    }

    /// A pivot is the other deliberate stop. It used to freeze the whole
    /// behavior to keep out of the stall clock; now it only pauses the
    /// accounting, and steering runs through it.
    #[test]
    fn a_pivot_never_advances_the_stall_clock() {
        let mut follower = PathFollowSteeringStrategy::to_point(vec3(5.0, 0.0, 0.0));
        let displaced = stand_still(
            &mut follower,
            MovementHold::Pivot,
            DISPLACEMENT_STALL_SECONDS + 1.0,
        );
        assert_eq!(follower.stall_seconds, 0.0);
        assert!(!displaced);
    }

    /// A hold pauses the clock, it does not reset it: seconds a real wedge
    /// already banked are still there when the hold lifts.
    #[test]
    fn a_hold_leaves_the_stall_it_paused_where_it_found_it() {
        let mut follower = PathFollowSteeringStrategy::to_point(vec3(5.0, 0.0, 0.0));
        stand_still(&mut follower, MovementHold::None, 1.0);
        let banked = follower.stall_seconds;
        stand_still(&mut follower, MovementHold::DoorWait, 1.0);
        assert!((follower.stall_seconds - banked).abs() < 1e-4);
        stand_still(&mut follower, MovementHold::None, 1.0);
        assert!(follower.stall_seconds > banked);
    }

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
    fn a_bias_bends_the_aim_point() {
        let aim = aim_with_bias(
            vec3(0.0, 0.0, 0.0),
            vec3(10.0, 0.0, 0.0),
            vec3(0.0, 0.0, 1.0),
        );
        assert_eq!(aim, vec3(10.0, 0.0, 1.0));
    }

    /// ...but never onto the body itself: a bias pointing back at an AI
    /// nearly on top of its waypoint would spin it around.
    #[test]
    fn a_bias_never_aims_at_the_body() {
        let position = vec3(0.0, 0.0, 0.0);
        let waypoint = vec3(0.3, 0.0, 0.0);
        assert_eq!(
            aim_with_bias(position, waypoint, vec3(-0.3, 0.0, 0.0)),
            waypoint
        );
    }

    /// A bias pointing back down the route keeps only its sideways part -
    /// the AI passes the obstacle instead of stopping in front of it.
    #[test]
    fn a_backward_bias_becomes_a_sideways_one() {
        let aim = aim_with_bias(
            vec3(0.0, 0.0, 0.0),
            vec3(10.0, 0.0, 0.0),
            vec3(-2.0, 0.0, 1.0),
        );
        assert_eq!(aim, vec3(10.0, 0.0, 1.0));
    }

    /// Queries are keyed by entity, so an answer can outlive the strategy
    /// that asked for it - a patrol that gives up on a point builds a fresh
    /// follower while the old query is still in flight. That answer says
    /// nothing about the new point and must not be adopted (its route would
    /// steer the body back at the abandoned point, and its verdict would
    /// declare the new point unreachable without anyone asking).
    #[test]
    fn an_answer_for_an_abandoned_point_is_not_adopted() {
        let point = vec3(10.0, 0.0, 10.0);
        let target = PathTarget::Point(point);
        assert!(answers_goal(&target, point, None), "its own answer");
        assert!(
            !answers_goal(&target, vec3(30.0, 0.0, 30.0), None),
            "an answer for the point the patrol just gave up on"
        );
        // A goal on the floor below is a different point, however close in XZ
        assert!(
            !answers_goal(&target, vec3(10.0, -6.0, 10.0), None),
            "an answer for a goal below this one"
        );
    }

    #[test]
    fn a_route_ending_at_its_goal_reaches_it() {
        let goal = vec3(10.0, 0.0, 10.0);
        assert!(route_reaches_goal(
            &[vec3(0.0, 0.0, 0.0), vec3(10.5, 0.0, 10.0)],
            goal
        ));
    }

    /// A partial route stopping a room short of an unreachable goal is not
    /// arrival - the patrol layer skips such a point instead of walking the
    /// route's end and then pressing on toward the goal.
    #[test]
    fn a_partial_route_stopping_short_does_not_reach_its_goal() {
        let goal = vec3(10.0, 0.0, 10.0);
        assert!(!route_reaches_goal(
            &[vec3(0.0, 0.0, 0.0), vec3(4.0, 0.0, 10.0)],
            goal
        ));
        assert!(!route_reaches_goal(&[], goal));
    }

    /// ...and a route that ends directly above (or below) the goal has not
    /// reached it either - the balcony case.
    #[test]
    fn a_route_ending_on_another_floor_does_not_reach_its_goal() {
        assert!(!route_reaches_goal(
            &[vec3(10.0, 0.0, 10.0)],
            vec3(10.0, -4.8, 10.0)
        ));
    }

    #[test]
    fn a_pivoting_retreat_does_not_burn_its_budget() {
        // Backing out of a wedge starts with a half-turn the body cannot
        // walk through; the clock waits for it
        assert_eq!(retreat_seconds_spent(Deg(180.0), 0.1), 0.0);
        assert_eq!(retreat_seconds_spent(Deg(-120.0), 0.1), 0.0);
        // ...and runs once it is moving again
        assert_eq!(retreat_seconds_spent(Deg(45.0), 0.1), 0.1);
        assert_eq!(retreat_seconds_spent(Deg(0.0), 0.1), 0.1);
    }

    #[test]
    fn a_crowd_cannot_outbid_the_whiskers() {
        let heading = vec3(0.0, 0.0, 1.0);
        // A huge crowd push (many neighbors, or a saturated repel) plus a
        // whisker push at right angles: the crowd is shortened to the
        // budget first, so the geometry it is steering around keeps half
        // the blend instead of being rounded away.
        let blended = blend_biases(vec3(100.0, 0.0, 0.0), vec3(0.0, 0.0, 2.5), heading, 2.5);
        assert!(
            (blended.z - blended.x).abs() < 1e-4,
            "an unbounded crowd must not outweigh the whiskers: {blended:?}"
        );
        // ...and the blend is still shortened once, to the budget
        let magnitude = (blended.x * blended.x + blended.z * blended.z).sqrt();
        assert!((magnitude - 2.5).abs() < 1e-4, "got {magnitude}");
        // A crowd on its own is capped like any other bias
        let alone = blend_biases(vec3(100.0, 0.0, 0.0), vec3(0.0, 0.0, 0.0), heading, 2.5);
        assert!((alone.x - 2.5).abs() < 1e-4, "got {alone:?}");
    }

    /// A neighbour on the open side pushes an AI back at the wall the
    /// whiskers just found. Summed, two opposite pushes of similar size
    /// cancel to nothing - no avoidance AND no yielding - so the crowd's
    /// opposing part is dropped and the wall wins outright.
    #[test]
    fn a_wall_outranks_a_crowd_push_into_it() {
        let heading = vec3(0.0, 0.0, 1.0);
        let wall_push = vec3(2.0, 0.0, 0.0);
        let crowd_into_wall = vec3(-2.2, 0.0, 0.0);
        let blended = blend_biases(crowd_into_wall, wall_push, heading, 2.5);
        assert!(
            blended.x > 1.0,
            "the whiskers must still steer away from the wall: {blended:?}"
        );
        // A crowd push that does NOT oppose the wall is untouched
        let alongside = blend_biases(vec3(0.0, 0.0, 1.0), wall_push, heading, 2.5);
        assert!((alongside.z - 1.0).abs() < 1e-4, "got {alongside:?}");
    }

    /// The full composition, travelling +z: a diagonal wall hit and a
    /// neighbour that sits OUTSIDE the head-on cone (so it is not converted
    /// to a sidestep). Their sideways parts are opposite, so summed as
    /// whole vectors they cancel and the aim point's forward projection
    /// erases what is left - losing the wall avoidance exactly when a
    /// neighbour is also present.
    #[test]
    fn a_wall_survives_the_projection_when_a_crowd_is_present() {
        let position = vec3(0.0, 0.0, 0.0);
        let waypoint = vec3(0.0, 0.0, 10.0);
        let heading = waypoint - position;
        let whiskers = vec3(1.0, 0.0, -1.0);
        let crowd = vec3(-1.0, 0.0, -1.0);
        // outside the head-on cone: the sidestep conversion leaves it alone
        let yielded = ai_util::yield_sideways(crowd, heading, whiskers);
        assert!(xz_distance(yielded, crowd) < 1e-4, "got {yielded:?}");

        let aim = aim_with_bias(
            position,
            waypoint,
            blend_biases(yielded, whiskers, heading, 2.5),
        );
        assert!(
            aim.x > 0.5,
            "the wall's sidestep must survive the projection: {aim:?}"
        );
    }

    #[test]
    fn a_crowd_agreeing_with_the_wall_still_adds() {
        let heading = vec3(0.0, 0.0, 1.0);
        // both push +x: the priority rule must not touch them, they sum
        let blended = blend_biases(vec3(0.5, 0.0, 0.0), vec3(1.0, 0.0, 0.0), heading, 2.5);
        assert!((blended.x - 1.5).abs() < 1e-4, "got {blended:?}");
        // ...and the sum is still shortened to the budget
        let capped_sum = blend_biases(vec3(2.0, 0.0, 0.0), vec3(2.0, 0.0, 0.0), heading, 2.5);
        assert!((capped_sum.x - 2.5).abs() < 1e-4, "got {capped_sum:?}");
    }

    #[test]
    fn a_crowd_with_no_whiskers_is_untouched() {
        let heading = vec3(0.0, 0.0, 1.0);
        let crowd = vec3(-1.0, 0.0, -1.0);
        let blended = blend_biases(crowd, vec3(0.0, 0.0, 0.0), heading, 2.5);
        assert!(xz_distance(blended, crowd) < 1e-4, "got {blended:?}");
    }

    #[test]
    fn biases_are_capped_together() {
        let shortened = capped(vec3(3.0, 0.0, 4.0), 2.5);
        assert!(
            ((shortened.x * shortened.x + shortened.z * shortened.z).sqrt() - 2.5).abs() < 1e-5
        );
        assert_eq!(capped(vec3(0.3, 0.0, 0.4), 2.5), vec3(0.3, 0.0, 0.4));
    }

    #[test]
    fn advance_does_not_reach_waypoints_on_other_floors() {
        // A waypoint 5 units directly overhead (a stacked floor) is NOT
        // reached by standing beneath it
        let path = vec![vec3(0.0, 5.0, 0.0), vec3(10.0, 5.0, 0.0)];
        assert_eq!(advance_waypoint(vec3(0.0, 0.0, 0.0), &path, 0), 0);
    }

    /// A safe landing: on the mesh, reachable, roomy, level floor.
    fn safe_probe() -> NudgeProbe {
        NudgeProbe {
            navigable: true,
            path_clear: true,
            clearance: true,
            headroom: true,
            floor_step: Some(0.0),
            landing: vec3(0.0, 0.0, 0.0),
        }
    }

    #[test]
    fn a_clear_supported_landing_is_taken() {
        assert!(nudge_is_safe(&safe_probe()));
        // ...including one a step up or down, which the nudge rides
        for floor_step in [NUDGE_MAX_STEP, -NUDGE_MAX_STEP] {
            assert!(nudge_is_safe(&NudgeProbe {
                floor_step: Some(floor_step),
                ..safe_probe()
            }));
        }
    }

    #[test]
    fn every_failed_probe_refuses_the_nudge() {
        // A nudge is a teleport: off the mesh, through a wall, into a body's
        // width of geometry, over a ledge, or into thin air are all refusals,
        // and the body grinds on until the re-path frees it instead.
        let refusals = [
            NudgeProbe {
                navigable: false,
                ..safe_probe()
            },
            NudgeProbe {
                path_clear: false,
                ..safe_probe()
            },
            NudgeProbe {
                clearance: false,
                ..safe_probe()
            },
            NudgeProbe {
                headroom: false,
                ..safe_probe()
            },
            NudgeProbe {
                floor_step: Some(-10.0 / SCALE_FACTOR),
                ..safe_probe()
            },
            NudgeProbe {
                floor_step: None,
                ..safe_probe()
            },
        ];
        for probe in &refusals {
            assert!(!nudge_is_safe(probe));
        }
    }

    /// Dark feet -> world units, so the synthetic probe worlds below read in
    /// the same units the constants are written in.
    fn ft(feet: f32) -> f32 {
        feet / SCALE_FACTOR
    }

    /// A creature (the human schema a hybrid animates on) and an empty
    /// physics world to build probe geometry in.
    fn probe_world() -> (World, EntityId, PhysicsWorld) {
        let mut world = World::new();
        let creature = world.add_entity((dark::properties::PropCreature(0),));
        (world, creature, PhysicsWorld::new())
    }

    /// Colliders only enter the broad phase on a step, and every query reads
    /// the broad phase - so geometry added to a fresh world is invisible to
    /// rays until this runs.
    fn settle(physics: &mut PhysicsWorld) {
        let mut player = physics.create_player(
            vec3(ft(1000.0), ft(1000.0), ft(1000.0)),
            EntityId::from_inner(1001).unwrap(),
        );
        physics.update(vec3(0.0, 0.0, 0.0), &mut player);
    }

    /// A static box, centre and half-extents in world units.
    fn add_box(physics: &mut PhysicsWorld, center: Vector3<f32>, half: Vector3<f32>) {
        use rapier3d::prelude::*;
        let body = physics.create_static_body(
            rapier3d::na::Isometry3::translation(center.x, center.y, center.z),
            None,
        );
        physics.attach_collider(
            body,
            SharedShape::cuboid(half.x, half.y, half.z),
            1.0,
            crate::physics::CollisionGroup::entity(),
        );
    }

    /// A half-foot step up, everywhere past a foot along +x. Low enough that
    /// it does not block the trip itself.
    fn add_step(physics: &mut PhysicsWorld) {
        add_box(
            physics,
            vec3(ft(6.0), ft(0.25), 0.0),
            vec3(ft(5.0), ft(0.25), ft(10.0)),
        );
    }

    /// Floor slab with its top surface at `top_y`, over the whole test area.
    fn add_floor(physics: &mut PhysicsWorld, top_y: f32) {
        add_box(
            physics,
            vec3(0.0, top_y - ft(1.0), 0.0),
            vec3(ft(50.0), ft(1.0), ft(50.0)),
        );
    }

    /// The body's origin sits inside a prop (which is what being pinned
    /// means), and a wall stands just past the landing. The solid queries
    /// report the prop at zero range; discarding that reading as "embedded"
    /// without looking past it hid the wall and called the trip clear.
    #[test]
    fn a_wall_behind_the_prop_the_body_is_embedded_in_refuses_the_nudge() {
        let (world, creature, mut physics) = probe_world();
        add_floor(&mut physics, 0.0);
        let from = vec3(0.0, ft(3.0), 0.0);
        let to = vec3(ft(2.0), ft(3.0), 0.0);
        // The prop the body is embedded in, centred on its origin and deep
        // enough that BOTH probe heights start inside it
        add_box(&mut physics, from, vec3(ft(0.5), ft(2.5), ft(0.5)));
        // ...and a wall a foot beyond the landing
        add_box(
            &mut physics,
            vec3(ft(3.25), ft(5.0), 0.0),
            vec3(ft(0.25), ft(5.0), ft(10.0)),
        );

        settle(&mut physics);
        let probe = probe_nudge(&world, &physics, creature, from, to, true);
        assert!(
            !probe.path_clear,
            "the wall past the landing must be seen through the prop the body is embedded in"
        );
        assert!(!nudge_is_safe(&probe));
    }

    /// The other half of the embedded case: with room beyond the prop, the
    /// body the unstick exists for is still nudged. Seeing past the prop must
    /// not turn into refusing everyone who is inside one.
    #[test]
    fn a_body_embedded_in_a_prop_is_still_nudged_when_the_way_is_clear() {
        let (world, creature, mut physics) = probe_world();
        add_floor(&mut physics, 0.0);
        let from = vec3(0.0, ft(3.0), 0.0);
        let to = vec3(ft(2.0), ft(3.0), 0.0);
        add_box(&mut physics, from, vec3(ft(0.5), ft(2.5), ft(0.5)));

        settle(&mut physics);
        let probe = probe_nudge(&world, &physics, creature, from, to, true);

        assert!(nudge_is_safe(&probe));
    }

    /// Headroom is comparative: a body already pinned under something low
    /// must still be nudgeable, or the unstick refuses the one population it
    /// exists for. The landing only has to be no tighter than the spot left.
    #[test]
    fn a_landing_no_tighter_than_the_spot_left_is_still_nudged() {
        let (world, creature, mut physics) = probe_world();
        add_floor(&mut physics, 0.0);
        // A ceiling too low for a hybrid, over the body AND the landing
        add_box(
            &mut physics,
            vec3(0.0, ft(5.5), 0.0),
            vec3(ft(50.0), ft(1.0), ft(50.0)),
        );
        let from = vec3(0.0, ft(3.0), 0.0);
        let to = vec3(ft(2.0), ft(3.0), 0.0);

        settle(&mut physics);
        let probe = probe_nudge(&world, &physics, creature, from, to, true);

        assert!(probe.headroom);
        assert!(nudge_is_safe(&probe));
    }

    /// ...and a body inside geometry for the WHOLE probe segment is not a
    /// clear ray either - the stepping runs out of segment without ever
    /// getting outside, which refuses.
    #[test]
    fn a_segment_that_never_leaves_the_geometry_refuses_the_nudge() {
        let (world, creature, mut physics) = probe_world();
        add_floor(&mut physics, 0.0);
        let from = vec3(0.0, ft(3.0), 0.0);
        let to = vec3(ft(2.0), ft(3.0), 0.0);
        // One solid block swallowing the body, the trip and the landing
        add_box(
            &mut physics,
            vec3(ft(1.0), ft(3.0), 0.0),
            vec3(ft(6.0), ft(3.0), ft(6.0)),
        );

        settle(&mut physics);
        let probe = probe_nudge(&world, &physics, creature, from, to, true);

        assert!(
            !probe.path_clear,
            "a segment spent entirely inside geometry is not a clear trip"
        );
        assert!(!nudge_is_safe(&probe));
    }

    /// A landing a step up rides that floor - so the room it needs is the
    /// room ABOVE the raised floor. Nothing measured that before the headroom
    /// probe, and a body does not fit where its head does not.
    #[test]
    fn a_step_up_under_an_overhang_refuses_the_nudge() {
        let (world, creature, mut physics) = probe_world();
        add_floor(&mut physics, 0.0);
        add_step(&mut physics);
        // ...with an overhang four feet above it - short of a hybrid
        add_box(
            &mut physics,
            vec3(ft(6.0), ft(5.5), 0.0),
            vec3(ft(5.0), ft(1.0), ft(10.0)),
        );
        let from = vec3(0.0, ft(3.0), 0.0);
        let to = vec3(ft(2.0), ft(3.0), 0.0);

        settle(&mut physics);
        let probe = probe_nudge(&world, &physics, creature, from, to, true);

        assert!(
            probe.path_clear,
            "the trip itself is clear - the refusal must come from the landing"
        );
        assert!(
            !probe.headroom,
            "a landing the body does not fit under must be refused"
        );
        assert!(!nudge_is_safe(&probe));
    }

    /// Clearance belongs to the landing the body would stand at, not to the
    /// height it currently occupies: a step up puts its shoulders alongside
    /// geometry that its old height cleared.
    #[test]
    fn a_step_up_alongside_geometry_refuses_the_nudge() {
        let (world, creature, mut physics) = probe_world();
        add_floor(&mut physics, 0.0);
        add_step(&mut physics);
        // ...beside a slab that starts just above the body's current origin
        add_box(
            &mut physics,
            vec3(ft(2.0), ft(4.6), ft(1.5)),
            vec3(ft(2.0), ft(1.4), ft(0.5)),
        );
        let from = vec3(0.0, ft(3.0), 0.0);
        let to = vec3(ft(2.0), ft(3.0), 0.0);

        settle(&mut physics);
        let probe = probe_nudge(&world, &physics, creature, from, to, true);

        assert!(probe.path_clear && probe.headroom);
        assert!(
            !probe.clearance,
            "the slab beside the RAISED landing must refuse it"
        );
        assert!(!nudge_is_safe(&probe));
    }

    /// The positive control, and the landing the call site takes: open floor,
    /// nothing in the way, and a landing that rides its own floor.
    #[test]
    fn an_open_landing_is_probed_safe_and_rides_its_floor() {
        let (world, creature, mut physics) = probe_world();
        add_floor(&mut physics, 0.0);
        let from = vec3(0.0, ft(3.0), 0.0);
        let to = vec3(ft(2.0), ft(3.0), 0.0);

        settle(&mut physics);
        let probe = probe_nudge(&world, &physics, creature, from, to, true);
        assert!(nudge_is_safe(&probe));
        assert!((probe.landing.y - from.y).abs() < 1.0e-3);

        // ...and half a foot up, the landing rises with the floor
        add_step(&mut physics);
        settle(&mut physics);
        let probe = probe_nudge(&world, &physics, creature, from, to, true);
        assert!(nudge_is_safe(&probe));
        assert!(
            (probe.landing.y - (from.y + ft(0.5))).abs() < 1.0e-2,
            "landing y {} did not ride the half-foot step",
            probe.landing.y
        );
    }

    #[test]
    fn a_repath_that_repeats_the_stalled_route_is_recognized() {
        let stalled = vec![7, 8];
        // The same cells, plus more of the route: still the same opening
        assert!(repeats_route(&stalled, &[7, 8, 9]));
        // ...and still the same route when the retreat left the body one
        // cell further back than it stalled in
        assert!(repeats_route(&stalled, &[6, 7, 8]));
    }

    #[test]
    fn a_repath_that_turns_away_is_a_different_route() {
        let stalled = vec![7, 8];
        assert!(!repeats_route(&stalled, &[7, 12]));
        // A route that stops short is different too
        assert!(!repeats_route(&stalled, &[7]));
        // Nothing to compare against is not a repeat
        assert!(!repeats_route(&[], &[7, 8]));
    }

    #[test]
    fn a_stall_reports_only_crossings_the_mesh_actually_has() {
        use crate::pathfinding::PathfindingService;
        use std::sync::Arc;

        // 0 -> 1 -> 2 is a real chain; the route below also names 0 -> 2,
        // which is not a link and must not eat a blacklist slot.
        let service = PathfindingService::new(Arc::new(crate::pathfinding::tests::three_cell_db(
            dark::mission::path_database::PathCellFlags::empty(),
        )));
        report_stall(&service, &[0, 2, 1], 0.0);
        let avoidance = service.avoidance(1.0);
        assert!(avoidance.cells.contains(&0), "the stalled cell is reported");
        assert_eq!(
            avoidance.links.into_iter().collect::<Vec<_>>(),
            Vec::new(),
            "0 -> 2 is not a link, and 2 -> 1 is not a crossing this route makes first"
        );

        report_stall(&service, &[0, 1], 0.0);
        assert!(service.avoidance(1.0).links.contains(&(0, 1)));
    }
}
