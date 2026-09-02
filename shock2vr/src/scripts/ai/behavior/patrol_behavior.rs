use cgmath::{Deg, EuclideanSpace, Vector3};
use dark::SCALE_FACTOR;
use dark::motion::MotionQueryItem;
use shipyard::{EntityId, World};

use crate::{
    physics::PhysicsWorld,
    scripts::{
        Effect,
        ai::ai_util,
        ai::steering::{
            self, CollisionAvoidanceSteeringStrategy, PathFollowSteeringStrategy, Steering,
            SteeringOutput, SteeringStrategy,
        },
    },
    time::Time,
};

use std::cell::RefCell;

use super::{Behavior, IdleBehavior, NextBehavior};

/// Close enough to a patrol point to advance to the next one
const PATROL_ARRIVE_DISTANCE: f32 = 4.0 / SCALE_FACTOR;
/// ...and within this height difference (patrol markers sit near the floor).
/// Standing under a marker on a walkway above is not arrival.
const PATROL_ARRIVE_HEIGHT: f32 = 6.0 / SCALE_FACTOR;
/// The route point the AI happens to be nearest is not always one it can
/// walk to: A* can report no route at all (the point sits in a disconnected
/// part of the navigation graph), and the fallback whisker steering then just
/// presses the body into the geometry in between. Giving up after this long
/// without covering ground - the patrol-scale counterpart of the path
/// follower's own per-waypoint stall watchdog, which cannot see a route that
/// never existed - keeps the route moving instead of walking into a wall for
/// the rest of the mission.
const PATROL_STALL_SECONDS: f32 = 5.0;
/// Ground covered (XZ, 2 Dark feet) that counts as progress and re-anchors
/// the stall watchdog. A patrolling AI clears this in a fraction of a second,
/// so only a body that is genuinely going nowhere trips the timer.
const PATROL_STALL_PROGRESS: f32 = 2.0 / SCALE_FACTOR;
/// Points skipped in a row, without reaching one in between, after which the
/// AI stops patrolling altogether and hands back to idle - rather than
/// shuffling forever between points it has no route to.
const PATROL_MAX_SKIPPED_POINTS: u32 = 3;

/// Walk an authored patrol route: head to the current patrol point, and on
/// arrival advance to the next one along the `AIPatrol` link chain. Routes are
/// typically closed loops, so this repeats indefinitely. Alertness changes (the
/// player becoming visible, a noise) replace this behavior through the normal
/// level-change path.
pub struct PatrolBehavior {
    /// The patrol-point object we are currently heading to
    target_point: EntityId,
    /// Its world position (the steering goal)
    goal: Vector3<f32>,
    steering_strategy: Box<dyn SteeringStrategy>,
    /// Set when the route dead-ends (a chain that isn't a loop): the AI has
    /// reached the final point and there is nowhere further to go, so it stops
    /// patrolling and hands back to idle. Also set once the AI has skipped
    /// PATROL_MAX_SKIPPED_POINTS unreachable points in a row.
    finished: bool,
    /// Stall watchdog: where the AI was when the timer was last re-anchored,
    /// and how long it has been stuck within PATROL_STALL_PROGRESS of it.
    stall_anchor: Option<Vector3<f32>>,
    stall_seconds: f32,
    /// Points given up on since the last one actually reached.
    skipped_points: u32,
    /// The runtime AICurrentPatrol link must be published once for a fresh
    /// behavior and whenever the target changes.
    target_dirty: bool,
    /// Builds the steering for a goal - a hook so tests can drive the
    /// behavior with a stubbed route outcome.
    make_steering: fn(Vector3<f32>) -> Box<dyn SteeringStrategy>,
    /// Whether the route has actually advanced at least one edge. Dark clears
    /// the authored AI_Patrol flag at a real end of chain, but the start rule
    /// targets the *destination* of the nearest link, which in shipped data
    /// can itself be a sink (eng2, hydro1, station all have some). Clearing on
    /// that first arrival would disable the creature's patrol permanently -
    /// the flag is a saved property - so a route that never moved on keeps it.
    walked_an_edge: bool,
}

impl PatrolBehavior {
    pub fn new(target_point: EntityId, goal: Vector3<f32>) -> PatrolBehavior {
        PatrolBehavior {
            target_point,
            goal,
            steering_strategy: Self::steering_to(goal),
            make_steering: Self::steering_to,
            finished: false,
            stall_anchor: None,
            stall_seconds: 0.0,
            skipped_points: 0,
            target_dirty: true,
            walked_an_edge: false,
        }
    }

    #[cfg(test)]
    fn with_steering(
        target_point: EntityId,
        goal: Vector3<f32>,
        make_steering: fn(Vector3<f32>) -> Box<dyn SteeringStrategy>,
    ) -> PatrolBehavior {
        PatrolBehavior {
            steering_strategy: make_steering(goal),
            make_steering,
            ..PatrolBehavior::new(target_point, goal)
        }
    }

    fn steering_to(goal: Vector3<f32>) -> Box<dyn SteeringStrategy> {
        steering::chained(vec![
            // Path steering leads - it carries its own whiskers as a bias on
            // its aim point; this one is the no-route fallback, where a
            // heading override is safe (see chase_behavior for the deadlock
            // that overriding an ACTIVE route causes)
            Box::new(PathFollowSteeringStrategy::to_point(goal)),
            Box::new(CollisionAvoidanceSteeringStrategy::conservative()),
        ])
    }

    fn arrived(&self, position: Vector3<f32>) -> bool {
        let dx = position.x - self.goal.x;
        let dz = position.z - self.goal.z;
        (dx * dx + dz * dz).sqrt() < PATROL_ARRIVE_DISTANCE
            && (position.y - self.goal.y).abs() < PATROL_ARRIVE_HEIGHT
    }

    /// Advance to the next point on the route; a closed loop repeats. Running
    /// out of route ends the patrol, and - when this arrival is a genuine end
    /// of chain the AI actually walked to - also clears the authored flag.
    fn advance(&mut self, world: &World, entity_id: EntityId, may_clear_flag: bool) -> Effect {
        match ai_util::next_patrol_point(world, entity_id, self.target_point) {
            Some((next, goal)) => {
                self.target_point = next;
                self.goal = goal;
                self.steering_strategy = (self.make_steering)(goal);
                self.stall_anchor = None;
                self.stall_seconds = 0.0;
                self.walked_an_edge = true;
                Effect::SetAICurrentPatrol {
                    entity_id,
                    target: Some(next),
                }
            }
            None => {
                self.finished = true;
                let mut effects = vec![Effect::SetAICurrentPatrol {
                    entity_id,
                    target: None,
                }];
                // Only an authored end of chain retires the route. A link the
                // mission never resolved (no instantiated target, or one with
                // no transform) also lands here, and that is a broken route,
                // not a finished one - leave the flag alone so the AI can pick
                // the route up again the next time it calms down.
                let true_dead_end = ai_util::is_patrol_dead_end(world, self.target_point);
                if may_clear_flag && true_dead_end && self.walked_an_edge {
                    effects.push(Effect::SetAIProperty {
                        entity_id,
                        update: crate::scripts::AIPropertyUpdate::PatrolEnabled { enabled: false },
                    });
                }
                Effect::combine(effects)
            }
        }
    }

    /// Give up on the current point and try the next one. Enough of those in
    /// a row - without reaching one in between - and the whole route is out
    /// of reach, so stop patrolling and hand back to idle rather than
    /// shuffling (and re-querying A*) between points forever.
    fn skip_point(&mut self, world: &World, entity_id: EntityId) -> Effect {
        self.skipped_points += 1;
        if self.skipped_points >= PATROL_MAX_SKIPPED_POINTS {
            self.finished = true;
            return Effect::SetAICurrentPatrol {
                entity_id,
                target: None,
            };
        }
        self.advance(world, entity_id, false)
    }

    /// Whether the AI has failed to cover ground for PATROL_STALL_SECONDS.
    /// Re-anchors (and reports false) as soon as it moves.
    fn stalled(&mut self, position: Vector3<f32>, time: &Time) -> bool {
        let moved = self
            .stall_anchor
            .map(|anchor| {
                (position.x - anchor.x).hypot(position.z - anchor.z) >= PATROL_STALL_PROGRESS
            })
            .unwrap_or(true);
        if moved {
            self.stall_anchor = Some(position);
            self.stall_seconds = 0.0;
            return false;
        }
        self.stall_seconds += time.elapsed.as_secs_f32();
        self.stall_seconds >= PATROL_STALL_SECONDS
    }
}

impl Behavior for PatrolBehavior {
    fn name(&self) -> &'static str {
        "Patrol"
    }

    fn steer(
        &mut self,
        current_heading: Deg<f32>,
        world: &World,
        physics: &PhysicsWorld,
        entity_id: EntityId,
        time: &Time,
    ) -> Option<(SteeringOutput, Effect)> {
        let mut patrol_effects = Vec::new();
        if self.target_dirty {
            self.target_dirty = false;
            patrol_effects.push(Effect::SetAICurrentPatrol {
                entity_id,
                target: Some(self.target_point),
            });
        }
        if !self.finished {
            let (position, _) = ai_util::get_position_and_forward(world, entity_id);
            let position = position.to_vec();
            if self.arrived(position) {
                self.skipped_points = 0;
                patrol_effects.push(self.advance(world, entity_id, true));
            } else if self.steering_strategy.goal_unreachable() {
                // No route to this point (shipped routes include points on a
                // disconnected part of the navigation graph, e.g. a marker on
                // the floor below). Move on at once rather than steering at it
                // on a heading nothing checked, grinding into the geometry in
                // between until the stall watchdog retires the whole route.
                // Counted like a stalled skip, so a route with no reachable
                // point left ends instead of cycling (and re-querying) forever.
                tracing::debug!(
                    "patrol {entity_id:?}: no route to point {:?}, skipping to the next one",
                    self.target_point
                );
                patrol_effects.push(self.skip_point(world, entity_id));
            } else if self.stalled(position, time) {
                // Going nowhere: give up on this point and try the next one.
                // Enough of those in a row and the whole route is out of
                // reach, so stop patrolling and hand back to idle via
                // next_behavior rather than walking in place forever.
                patrol_effects.push(self.skip_point(world, entity_id));
            }
        }

        if self.finished {
            return Some((
                Steering::from_current(current_heading),
                Effect::combine(patrol_effects),
            ));
        }

        let (steering, steering_effect) = self
            .steering_strategy
            .steer(current_heading, world, physics, entity_id, time)
            .unwrap_or((Steering::from_current(current_heading), Effect::NoEffect));
        patrol_effects.push(steering_effect);
        Some((steering, Effect::combine(patrol_effects)))
    }

    fn next_behavior(
        &mut self,
        _world: &World,
        _physics: &PhysicsWorld,
        _entity_id: EntityId,
    ) -> NextBehavior {
        if self.finished {
            NextBehavior::Next(Box::new(RefCell::new(IdleBehavior::new())))
        } else {
            NextBehavior::Stay
        }
    }

    fn animation(&self) -> Vec<MotionQueryItem> {
        if self.finished {
            return vec![MotionQueryItem::new("idlegesture").optional()];
        }
        vec![
            MotionQueryItem::new("locourgent").optional(),
            MotionQueryItem::new("locomote"),
        ]
    }

    fn is_locomotion(&self) -> bool {
        !self.finished
    }

    #[cfg(test)]
    fn patrol_target(&self) -> Option<EntityId> {
        (!self.finished).then_some(self.target_point)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime_props::RuntimePropTransform;
    use cgmath::{Matrix4, vec3};
    use dark::properties::{Link, Links, ToLink, WrappedEntityId};
    use shipyard::{Get, ViewMut};

    /// A world with a creature pinned at the origin and a three-point
    /// `AIPatrol` loop 100 units away that it can never reach.
    fn world_with_unreachable_route() -> (World, EntityId, EntityId, Vector3<f32>) {
        let mut world = World::new();
        let creature = world.add_entity(RuntimePropTransform(Matrix4::from_scale(1.0)));
        let points: Vec<EntityId> = (0..3)
            .map(|i| {
                world.add_entity((
                    RuntimePropTransform(Matrix4::from_translation(vec3(
                        100.0 + i as f32,
                        0.0,
                        100.0,
                    ))),
                    Links::empty(),
                ))
            })
            .collect();
        {
            let mut v_links = world.borrow::<ViewMut<Links>>().unwrap();
            for (index, point) in points.iter().enumerate() {
                (&mut v_links).get(*point).unwrap().to_links.push(ToLink {
                    to_template_id: 0,
                    to_entity_id: Some(WrappedEntityId(points[(index + 1) % points.len()])),
                    link: Link::AIPatrol,
                });
            }
        }
        (world, creature, points[0], vec3(100.0, 0.0, 100.0))
    }

    /// A patroller that cannot reach its route must not walk into the
    /// geometry forever. The nearest point is chosen geometrically, so it can
    /// sit in a disconnected part of the navigation graph - A* then returns
    /// no route at all, which the path follower's own per-waypoint watchdog
    /// cannot see, and the whisker fallback just presses the body onward.
    #[test]
    fn a_patroller_that_cannot_reach_its_route_gives_up() {
        let (world, creature, first, goal) = world_with_unreachable_route();
        let physics = PhysicsWorld::new();
        let mut patrol = PatrolBehavior::new(first, goal);

        // 20 simulated seconds without the body ever moving - four times the
        // stall window, so every point on the loop gets its turn.
        let mut elapsed = 0.0;
        let mut emitted = Vec::new();
        for _ in 0..200 {
            let time = Time {
                elapsed: std::time::Duration::from_millis(100),
                total: std::time::Duration::from_millis(0),
            };
            if let Some((_, effect)) = patrol.steer(Deg(0.0), &world, &physics, creature, &time) {
                emitted.extend(Effect::flatten(vec![effect]));
            }
            elapsed += 0.1;
            if patrol.finished {
                break;
            }
        }

        assert!(
            patrol.finished,
            "a patroller going nowhere should give up, still patrolling after {elapsed}s",
        );
        assert!(
            matches!(
                patrol.next_behavior(&world, &physics, creature),
                NextBehavior::Next(_)
            ),
            "giving up should hand back to idle",
        );
        assert!(emitted.iter().any(|effect| matches!(
            effect,
            Effect::SetAICurrentPatrol {
                entity_id,
                target: None,
            } if *entity_id == creature
        )));
        assert!(
            !emitted.iter().any(|effect| matches!(
                effect,
                Effect::SetAIProperty {
                    update: crate::scripts::AIPropertyUpdate::PatrolEnabled { enabled: false },
                    ..
                }
            )),
            "movement failure stops this attempt but leaves the authored patrol flag enabled"
        );
    }

    /// ...but only when it is genuinely going nowhere: an AI that keeps
    /// covering ground toward a point it has not reached yet stays on route.
    #[test]
    fn a_patroller_making_progress_keeps_its_route() {
        let (world, creature, first, goal) = world_with_unreachable_route();
        let physics = PhysicsWorld::new();
        let mut patrol = PatrolBehavior::new(first, goal);

        for step in 0..200 {
            // Walk 1 unit per tick toward the goal (never arriving).
            world.run(|mut v_xform: ViewMut<RuntimePropTransform>| {
                (&mut v_xform).get(creature).unwrap().0 =
                    Matrix4::from_translation(vec3(step as f32, 0.0, 0.0));
            });
            let time = Time {
                elapsed: std::time::Duration::from_millis(100),
                total: std::time::Duration::from_millis(0),
            };
            patrol.steer(Deg(0.0), &world, &physics, creature, &time);
        }

        assert!(
            !patrol.finished,
            "a moving patroller must stay on its route"
        );
        assert_eq!(patrol.target_point, first, "and keep the same point");
    }

    /// Steering that reports no route to whatever goal it was given.
    struct UnreachableSteering;
    /// ...and its counterpart, which has a route.
    struct ReachableSteering;
    impl SteeringStrategy for ReachableSteering {}
    impl SteeringStrategy for UnreachableSteering {
        fn goal_unreachable(&self) -> bool {
            true
        }
    }

    /// A patrol point with no route to it (shipped routes include points on
    /// a disconnected part of the navigation graph) is skipped at once - and
    /// the route keeps going, rather than the AI grinding into the geometry
    /// between it and the point until the watchdog retires the patrol.
    #[test]
    fn a_patroller_skips_a_point_it_has_no_route_to() {
        let (world, creature, first, goal) = world_with_unreachable_route();
        let physics = PhysicsWorld::new();
        let mut patrol =
            PatrolBehavior::with_steering(first, goal, |_| Box::new(UnreachableSteering));

        let mut emitted = Vec::new();
        for _ in 0..100 {
            if let Some((_, effect)) = patrol.steer(Deg(0.0), &world, &physics, creature, &tick()) {
                emitted.extend(Effect::flatten(vec![effect]));
            }
        }

        // The route moves on at once - one tick per point, no waiting out the
        // stall watchdog - and, since NO point on this loop is reachable, it
        // ends at the skip cap rather than cycling (and re-querying) forever.
        let targets: Vec<EntityId> = emitted
            .iter()
            .filter_map(|effect| match effect {
                Effect::SetAICurrentPatrol { target, .. } => *target,
                _ => None,
            })
            .collect();
        assert_eq!(
            targets.len(),
            PATROL_MAX_SKIPPED_POINTS as usize,
            "expected one skip per tick up to the cap, got {targets:?}"
        );
        assert!(patrol.finished, "a route with no reachable point ends");
    }

    thread_local! {
        /// How many steering strategies the test below has handed out
        static STEERINGS_BUILT: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
    }

    /// One unreachable point does not end a route: the AI skips it and
    /// carries on toward the next, which it can route to.
    #[test]
    fn one_unreachable_point_does_not_end_the_route() {
        let (world, creature, first, goal) = world_with_unreachable_route();
        let physics = PhysicsWorld::new();
        STEERINGS_BUILT.with(|built| built.set(0));
        // Only the first point reports no route.
        let mut patrol = PatrolBehavior::with_steering(first, goal, |_| {
            let nth = STEERINGS_BUILT.with(|built| {
                let nth = built.get();
                built.set(nth + 1);
                nth
            });
            if nth == 0 {
                Box::new(UnreachableSteering)
            } else {
                Box::new(ReachableSteering) as Box<dyn SteeringStrategy>
            }
        });

        let mut emitted = Vec::new();
        for _ in 0..20 {
            if let Some((_, effect)) = patrol.steer(Deg(0.0), &world, &physics, creature, &tick()) {
                emitted.extend(Effect::flatten(vec![effect]));
            }
        }

        assert!(!patrol.finished, "the route carries on past one bad point");
        let skips = emitted
            .iter()
            .filter(|effect| matches!(effect, Effect::SetAICurrentPatrol { .. }))
            .count();
        assert_eq!(skips, 2, "the initial target, then one skip");
    }

    #[test]
    fn patrol_publishes_its_live_target_relation() {
        let (world, creature, first, goal) = world_with_unreachable_route();
        let physics = PhysicsWorld::new();
        let mut patrol = PatrolBehavior::new(first, goal);
        let time = Time {
            elapsed: std::time::Duration::from_millis(100),
            total: std::time::Duration::from_millis(0),
        };

        let (_, effect) = patrol
            .steer(Deg(0.0), &world, &physics, creature, &time)
            .expect("an active patrol steers");
        assert!(Effect::flatten(vec![effect]).iter().any(|effect| matches!(
            effect,
            Effect::SetAICurrentPatrol {
                entity_id,
                target: Some(target),
            } if *entity_id == creature && *target == first
        )));
    }

    /// A two-point route `first -> final_point`, with the creature and both
    /// markers stacked at the origin so every `arrived()` is immediate: the
    /// AI walks one real edge and then hits the end of the chain.
    fn world_with_route_ending_in_a_sink() -> (World, EntityId, EntityId, EntityId) {
        let mut world = World::new();
        let creature = world.add_entity(RuntimePropTransform(Matrix4::from_scale(1.0)));
        let final_point = world.add_entity((
            RuntimePropTransform(Matrix4::from_scale(1.0)),
            Links::empty(),
        ));
        let first = world.add_entity((
            RuntimePropTransform(Matrix4::from_scale(1.0)),
            Links {
                to_links: vec![ToLink {
                    to_template_id: 0,
                    to_entity_id: Some(WrappedEntityId(final_point)),
                    link: Link::AIPatrol,
                }],
            },
        ));
        (world, creature, first, final_point)
    }

    fn tick() -> Time {
        Time {
            elapsed: std::time::Duration::from_millis(100),
            total: std::time::Duration::from_millis(0),
        }
    }

    fn cleared_patrol_flag(effects: &[Effect], creature: EntityId) -> bool {
        effects.iter().any(|effect| {
            matches!(
                effect,
                Effect::SetAIProperty {
                    entity_id,
                    update: crate::scripts::AIPropertyUpdate::PatrolEnabled { enabled: false },
                } if *entity_id == creature
            )
        })
    }

    /// The start rule targets the *destination* of the nearest link, and in
    /// shipped data (eng2, hydro1, station) that destination can itself be a
    /// sink. Retiring the route on that very first arrival would write
    /// `PropAIPatrol(false)`, which is saved - the creature would never patrol
    /// again for the rest of the playthrough.
    #[test]
    fn patrol_does_not_retire_a_route_it_never_walked() {
        let (world, creature, _, sink) = world_with_route_ending_in_a_sink();
        let physics = PhysicsWorld::new();
        let mut patrol = PatrolBehavior::new(sink, vec3(0.0, 0.0, 0.0));

        let (_, effect) = patrol
            .steer(Deg(0.0), &world, &physics, creature, &tick())
            .expect("the terminal transition must still emit its effects");

        let effects = Effect::flatten(vec![effect]);
        assert!(patrol.finished, "the behavior still ends");
        assert!(
            !cleared_patrol_flag(&effects, creature),
            "a route that never advanced must keep its authored patrol flag"
        );
    }

    /// A link the mission never resolved is a broken route, not a finished
    /// one - it must not retire the authored flag either.
    #[test]
    fn patrol_does_not_retire_a_route_with_an_unresolved_link() {
        let mut world = World::new();
        let creature = world.add_entity(RuntimePropTransform(Matrix4::from_scale(1.0)));
        let dangling = world.add_entity((
            RuntimePropTransform(Matrix4::from_scale(1.0)),
            Links {
                to_links: vec![ToLink {
                    to_template_id: 77,
                    to_entity_id: None,
                    link: Link::AIPatrol,
                }],
            },
        ));
        let physics = PhysicsWorld::new();
        let mut patrol = PatrolBehavior::new(dangling, vec3(0.0, 0.0, 0.0));
        patrol.walked_an_edge = true;

        let (_, effect) = patrol
            .steer(Deg(0.0), &world, &physics, creature, &tick())
            .expect("the terminal transition must still emit its effects");

        assert!(
            !cleared_patrol_flag(&Effect::flatten(vec![effect]), creature),
            "an unresolved patrol link is not an authored end of chain"
        );
    }

    #[test]
    fn patrol_dead_end_clears_flag_and_current_target() {
        let (world, creature, first, _) = world_with_route_ending_in_a_sink();
        let physics = PhysicsWorld::new();
        let mut patrol = PatrolBehavior::new(first, vec3(0.0, 0.0, 0.0));
        let time = tick();

        // First arrival walks the real edge first -> final_point...
        patrol
            .steer(Deg(0.0), &world, &physics, creature, &time)
            .expect("an active patrol steers");
        assert!(!patrol.finished, "one edge still remains");

        // ...and the second arrival is the genuine end of the chain.
        let (_, effect) = patrol
            .steer(Deg(0.0), &world, &physics, creature, &time)
            .expect("the terminal transition must still emit its effects");
        let effects = Effect::flatten(vec![effect]);
        assert!(effects.iter().any(|effect| matches!(
            effect,
            Effect::SetAIProperty {
                entity_id,
                update: crate::scripts::AIPropertyUpdate::PatrolEnabled { enabled: false },
            } if *entity_id == creature
        )));
        assert!(effects.iter().any(|effect| matches!(
            effect,
            Effect::SetAICurrentPatrol {
                entity_id,
                target: None,
            } if *entity_id == creature
        )));
    }
}
