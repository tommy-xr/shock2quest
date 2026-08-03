use cgmath::Deg;
use dark::motion::MotionQueryItem;
use rand::Rng;
use shipyard::{EntityId, World};

use crate::{
    physics::PhysicsWorld,
    scripts::{
        Effect,
        ai::steering::{Steering, SteeringOutput},
    },
    time::Time,
};

use super::Behavior;

/// How long an idle creature holds a heading before sweeping to the next one.
const SCAN_DWELL_SECONDS: f32 = 2.5;

/// How fast the scan heading sweeps, in degrees per second. Well under the
/// 180 deg/s turn speed, so the body follows the sweep exactly instead of
/// snapping between poses.
const SCAN_SWEEP_DEGREES_PER_SECOND: f32 = 45.0;

/// How far to either side of its post the sweep reaches. Combined with the
/// 60-degree FOV half-angle this covers 300 degrees around the post and
/// leaves a blind wedge directly behind it, so sneaking up from straight
/// back still works.
const SCAN_HALF_ARC_DEGREES: f32 = 90.0;

/// Standing watch: the creature holds its post, but looks around.
///
/// Sight is the only thing that escalates a calm creature's alertness, and
/// the cone it sees through is fixed to its heading. A creature that never
/// turns is therefore permanently blind to everything outside that one cone -
/// the "never re-acquires the player" half of #791. The hydro2 Midwife that
/// reopened the issue is the clearest case: it takes up its post facing a
/// ledge wall 1.6 units away, so with a fixed heading no line of sight to it
/// can ever exist. Sweeping the heading back and forth over the post (Dark's
/// idle AIs look around the same way) lets the normal FOV + line-of-sight
/// check eventually see what is really there. Nothing about the sight test
/// itself is relaxed - the creature still only ever sees what is genuinely
/// in front of it and unoccluded.
pub struct IdleBehavior {
    /// The heading the creature took its post with. Latched on the first
    /// steer, because the script - not the behavior - owns the heading.
    post_heading: Option<Deg<f32>>,
    /// Current sweep offset from `post_heading`, in degrees.
    offset: f32,
    /// The end of the sweep currently being travelled to, in degrees.
    target: f32,
    /// Seconds left of the pause at this end of the sweep.
    dwell: f32,
}

impl Default for IdleBehavior {
    fn default() -> Self {
        Self::new()
    }
}

impl IdleBehavior {
    pub fn new() -> IdleBehavior {
        // Randomize which way the first sweep goes so creatures posted
        // together don't scan in lockstep.
        let direction = if rand::thread_rng().gen_bool(0.5) {
            1.0
        } else {
            -1.0
        };
        IdleBehavior {
            post_heading: None,
            offset: 0.0,
            target: direction * SCAN_HALF_ARC_DEGREES,
            // Settle onto the post before the first sweep.
            dwell: SCAN_DWELL_SECONDS,
        }
    }

    /// Advance the scan by `delta` seconds, returning the offset from the
    /// post heading to face.
    fn advance(&mut self, delta: f32) -> f32 {
        if self.dwell > 0.0 {
            self.dwell -= delta;
            return self.offset;
        }

        let step = SCAN_SWEEP_DEGREES_PER_SECOND * delta;
        if (self.target - self.offset).abs() <= step {
            // Reached this end of the sweep: pause, then head for the other.
            self.offset = self.target;
            self.target = -self.target;
            self.dwell = SCAN_DWELL_SECONDS;
        } else {
            self.offset += step * (self.target - self.offset).signum();
        }
        self.offset
    }
}

impl Behavior for IdleBehavior {
    fn name(&self) -> &'static str {
        "Idle"
    }

    fn animation(self: &IdleBehavior) -> Vec<MotionQueryItem> {
        vec![MotionQueryItem::new("idlegesture")]
    }

    fn animation_queries(&self) -> Vec<Vec<MotionQueryItem>> {
        vec![self.animation(), vec![MotionQueryItem::new("stand")]]
    }

    fn steer(
        &mut self,
        current_heading: Deg<f32>,
        _world: &World,
        _physics: &PhysicsWorld,
        _entity_id: EntityId,
        time: &Time,
    ) -> Option<(SteeringOutput, Effect)> {
        let post = *self.post_heading.get_or_insert(current_heading);
        let offset = self.advance(time.elapsed.as_secs_f32());
        Some((
            Steering::from_current(Deg(post.0 + offset)),
            Effect::NoEffect,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn steer_for(behavior: &mut IdleBehavior, post: Deg<f32>, seconds: f32) -> f32 {
        let world = World::new();
        let physics = PhysicsWorld::new();
        let time = Time {
            elapsed: std::time::Duration::from_secs_f32(seconds),
            total: std::time::Duration::from_secs_f32(seconds),
        };
        behavior
            .steer(post, &world, &physics, EntityId::dead(), &time)
            .expect("idle always steers")
            .0
            .desired_heading
            .0
    }

    #[test]
    fn idle_prefers_gesture_then_stand() {
        let queries = IdleBehavior::new().animation_queries();
        let tags = queries
            .iter()
            .map(|query| {
                query
                    .iter()
                    .map(MotionQueryItem::tag_name)
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();

        assert_eq!(tags, vec![vec!["idlegesture"], vec!["stand"]]);
    }

    /// #791: an idle creature must look around, or its fixed FOV cone leaves
    /// everything outside that one direction permanently invisible to it.
    #[test]
    fn idle_sweeps_its_heading_to_both_sides_of_its_post() {
        let mut behavior = IdleBehavior::new();
        let post = Deg(30.0);

        let mut min_offset: f32 = 0.0;
        let mut max_offset: f32 = 0.0;
        // Two full sweeps' worth of dwell + travel, at 60Hz.
        for _ in 0..1200 {
            let offset = steer_for(&mut behavior, post, 1.0 / 60.0) - post.0;
            min_offset = min_offset.min(offset);
            max_offset = max_offset.max(offset);
        }

        assert!(
            max_offset >= SCAN_HALF_ARC_DEGREES - 0.01,
            "idle should sweep a full arc to one side, reached {max_offset}",
        );
        assert!(
            min_offset <= -(SCAN_HALF_ARC_DEGREES - 0.01),
            "idle should sweep a full arc to the other side, reached {min_offset}",
        );
    }

    /// The sweep is anchored on the post, so a creature standing watch never
    /// drifts off the heading it was left with.
    #[test]
    fn idle_sweep_stays_within_its_arc() {
        let mut behavior = IdleBehavior::new();
        let post = Deg(-140.0);

        for _ in 0..1800 {
            let offset = steer_for(&mut behavior, post, 1.0 / 60.0) - post.0;
            assert!(
                offset.abs() <= SCAN_HALF_ARC_DEGREES + 0.01,
                "idle swept {offset} degrees off its post",
            );
        }
    }

    /// It settles onto its post before the first sweep, so a creature that
    /// just took up a heading (e.g. after losing the player) holds it for a
    /// beat instead of immediately turning away.
    #[test]
    fn idle_holds_its_post_before_the_first_sweep() {
        let mut behavior = IdleBehavior::new();
        let post = Deg(75.0);

        let heading = steer_for(&mut behavior, post, 0.1);
        assert!(
            (heading - post.0).abs() < f32::EPSILON,
            "the first frame should hold the post heading, got {heading}",
        );
    }
}
