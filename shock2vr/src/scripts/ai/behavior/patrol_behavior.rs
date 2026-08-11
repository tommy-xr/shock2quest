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
}

impl PatrolBehavior {
    pub fn new(target_point: EntityId, goal: Vector3<f32>) -> PatrolBehavior {
        PatrolBehavior {
            target_point,
            goal,
            steering_strategy: Self::steering_to(goal),
            finished: false,
            stall_anchor: None,
            stall_seconds: 0.0,
            skipped_points: 0,
            target_dirty: true,
        }
    }

    fn steering_to(goal: Vector3<f32>) -> Box<dyn SteeringStrategy> {
        steering::chained(vec![
            // Path steering leads; whisker avoidance only covers the no-route
            // case (see chase_behavior for the deadlock this prevents)
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

    /// Advance to the next point on the route; a closed loop repeats. A
    /// dead-end (no next link) ends the patrol.
    fn advance(
        &mut self,
        world: &World,
        entity_id: EntityId,
        clear_flag_on_dead_end: bool,
    ) -> Effect {
        match ai_util::next_patrol_point(world, entity_id, self.target_point) {
            Some((next, goal)) => {
                self.target_point = next;
                self.goal = goal;
                self.steering_strategy = Self::steering_to(goal);
                self.stall_anchor = None;
                self.stall_seconds = 0.0;
                self.target_dirty = false;
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
                if clear_flag_on_dead_end {
                    effects.push(Effect::SetAIProperty {
                        entity_id,
                        update: crate::scripts::AIPropertyUpdate::PatrolEnabled { enabled: false },
                    });
                }
                Effect::combine(effects)
            }
        }
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
            } else if self.stalled(position, time) {
                // Going nowhere: give up on this point and try the next one.
                // Enough of those in a row and the whole route is out of
                // reach, so stop patrolling and hand back to idle via
                // next_behavior rather than walking in place forever.
                self.skipped_points += 1;
                if self.skipped_points >= PATROL_MAX_SKIPPED_POINTS {
                    self.finished = true;
                    patrol_effects.push(Effect::SetAICurrentPatrol {
                        entity_id,
                        target: None,
                    });
                } else {
                    patrol_effects.push(self.advance(world, entity_id, false));
                }
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

    #[test]
    fn patrol_dead_end_clears_flag_and_current_target() {
        let mut world = World::new();
        let creature = world.add_entity(RuntimePropTransform(Matrix4::from_scale(1.0)));
        let final_point = world.add_entity((
            RuntimePropTransform(Matrix4::from_scale(1.0)),
            Links::empty(),
        ));
        let physics = PhysicsWorld::new();
        let mut patrol = PatrolBehavior::new(final_point, vec3(0.0, 0.0, 0.0));
        let time = Time {
            elapsed: std::time::Duration::from_millis(100),
            total: std::time::Duration::from_millis(0),
        };

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
