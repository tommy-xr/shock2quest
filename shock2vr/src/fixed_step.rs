//! Fixed simulation clock used by the opt-in `fixed_simulation` scheduler.
//!
//! The caller supplies active elapsed time (excluding pause/focus suspension).
//! A per-render catch-up limit retains excess debt; it does not discard time.
use std::{num::NonZeroU32, time::Duration};

use crate::time::Time;

pub struct FixedStepClock {
    hz: NonZeroU32,
    max_steps: NonZeroU32,
    elapsed: Duration,
    completed: u128,
}

impl FixedStepClock {
    pub fn new(hz: NonZeroU32, max_steps: NonZeroU32) -> Self {
        assert!(
            hz.get() <= 1_000_000_000,
            "ticks must be at least one nanosecond"
        );
        Self {
            hz,
            max_steps,
            elapsed: Duration::ZERO,
            completed: 0,
        }
    }

    fn time_at(&self, tick: u128) -> Duration {
        // Rational boundaries avoid accumulating the rounding error from
        // repeatedly adding a rounded 1/60 s Duration. dt varies by at most
        // one nanosecond; the physics solver retains its own fixed f32 dt.
        let nanos = tick * 1_000_000_000 / u128::from(self.hz.get());
        Duration::new(
            (nanos / 1_000_000_000) as u64,
            (nanos % 1_000_000_000) as u32,
        )
    }

    pub fn total(&self) -> Duration {
        self.time_at(self.completed)
    }

    /// Includes any complete unexecuted steps as well as the fractional step.
    pub fn pending(&self) -> Duration {
        self.elapsed - self.total()
    }

    /// Only presentation may interpolate; this never modifies simulated state.
    /// During overload, clamp to current state rather than extrapolating.
    pub fn interpolation_alpha(&self) -> f64 {
        let next_dt = self.time_at(self.completed + 1) - self.total();
        (self.pending().as_secs_f64() / next_dt.as_secs_f64()).min(1.0)
    }

    pub fn advance(&mut self, active_elapsed: Duration) -> Steps<'_> {
        self.elapsed += active_elapsed;
        let remaining = self.max_steps.get();
        Steps {
            clock: self,
            remaining,
        }
    }
}

/// Steps become consumed only when yielded. Dropping a partially consumed
/// iterator retains every unconsumed tick for a subsequent advance call.
pub struct Steps<'a> {
    clock: &'a mut FixedStepClock,
    remaining: u32,
}
impl Iterator for Steps<'_> {
    type Item = Time;
    fn next(&mut self) -> Option<Time> {
        if self.remaining == 0 {
            return None;
        }
        let total = self.clock.time_at(self.clock.completed + 1);
        if total > self.clock.elapsed {
            return None;
        }
        let elapsed = total - self.clock.total();
        self.clock.completed += 1;
        self.remaining -= 1;
        Some(Time { elapsed, total })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn clock(hz: u32, limit: u32) -> FixedStepClock {
        FixedStepClock::new(
            NonZeroU32::new(hz).unwrap(),
            NonZeroU32::new(limit).unwrap(),
        )
    }

    #[test]
    fn equal_elapsed_time_has_equal_tick_count_at_all_display_rates() {
        for render_hz in [60, 72, 90, 120, 144] {
            let mut clock = clock(60, 8);
            let mut elapsed = Duration::ZERO;
            let mut ticks = 0;
            for frame in 0..render_hz * 10 {
                let from = Duration::from_secs_f64(frame as f64 / render_hz as f64);
                let to = Duration::from_secs_f64((frame + 1) as f64 / render_hz as f64);
                for tick in clock.advance(to - from) {
                    elapsed += tick.elapsed;
                    ticks += 1;
                }
            }
            assert_eq!(ticks, 600, "render rate {render_hz}");
            assert_eq!(elapsed, Duration::from_secs(10));
            assert_eq!(clock.total(), elapsed);
            assert_eq!(clock.pending(), Duration::ZERO);
        }
    }

    #[test]
    fn bounded_catchup_and_an_abandoned_iterator_preserve_debt() {
        let mut clock = clock(60, 2);
        let mut steps = clock.advance(Duration::from_millis(100));
        assert!(steps.next().is_some());
        drop(steps);
        assert_eq!(clock.interpolation_alpha(), 1.0);
        assert_eq!(clock.advance(Duration::ZERO).count(), 2);
        assert_eq!(clock.advance(Duration::ZERO).count(), 2);
        assert_eq!(clock.advance(Duration::ZERO).count(), 1);
        assert_eq!(clock.total(), Duration::from_millis(100));
        assert_eq!(clock.pending(), Duration::ZERO);
    }

    #[test]
    fn real_physics_fall_and_player_movement_match_across_render_rates() {
        use crate::physics::{CollisionGroup, DynamicPhysicsOptions, PhysicsShape, PhysicsWorld};
        use cgmath::{Quaternion, vec3};
        use shipyard::EntityId;
        let mut reference = None;
        for render_hz in [30, 60, 72, 90, 120, 144] {
            let mut clock = clock(60, 8);
            let mut physics = PhysicsWorld::new();
            let entity = EntityId::from_inner(10).unwrap();
            let mut player =
                physics.create_player(vec3(0.0, 100.0, 0.0), EntityId::from_inner(11).unwrap());
            let body = physics.add_dynamic(
                entity,
                vec3(10.0, 100.0, 0.0),
                Quaternion::new(1.0, 0.0, 0.0, 0.0),
                vec3(0.0, 0.0, 0.0),
                PhysicsShape::Sphere(0.1),
                CollisionGroup::entity(),
                false,
                DynamicPhysicsOptions::default(),
            );
            let mut player_position = vec3(0.0, 100.0, 0.0);
            let mut steps = 0;
            for frame in 0..render_hz {
                let from = Duration::from_secs_f64(frame as f64 / render_hz as f64);
                let to = Duration::from_secs_f64((frame + 1) as f64 / render_hz as f64);
                for tick in clock.advance(to - from) {
                    (player_position, _) = physics.update(
                        vec3(tick.elapsed.as_secs_f32() * 2.0, 0.0, 0.0),
                        &mut player,
                    );
                    steps += 1;
                }
            }
            let position = physics.get_position(body).unwrap();
            let velocity = physics.get_velocity(entity).unwrap();
            assert_eq!(steps, 60);
            assert!((velocity.y + 9.81).abs() < 0.001);
            let state = (position, velocity, player_position);
            if let Some(expected) = reference {
                assert_eq!(state, expected, "render rate {render_hz}");
            } else {
                reference = Some(state);
            }
        }
    }

    #[test]
    fn grounded_jump_has_the_same_arc_at_every_display_rate() {
        use crate::physics::{CollisionGroup, PhysicsWorld};
        use cgmath::{Quaternion, vec3};
        use shipyard::EntityId;
        let mut reference = None;
        for hz in [30, 60, 72, 90, 120, 144] {
            let mut clock = clock(60, 8);
            let mut world = PhysicsWorld::new();
            world.add_kinematic(
                EntityId::from_inner(1000).unwrap(),
                vec3(0.0, -0.5, 0.0),
                Quaternion::new(1.0, 0.0, 0.0, 0.0),
                vec3(0.0, 0.0, 0.0),
                vec3(20.0, 1.0, 20.0),
                CollisionGroup::entity(),
                false,
            );
            let mut player =
                world.create_player(vec3(0.0, 2.0, 0.0), EntityId::from_inner(2000).unwrap());
            let mut positions = Vec::new();
            for frame in 0..hz * 3 {
                let from = Duration::from_secs_f64(frame as f64 / hz as f64);
                let to = Duration::from_secs_f64((frame + 1) as f64 / hz as f64);
                for tick in clock.advance(to - from) {
                    let jump =
                        tick.total >= Duration::from_secs(1) && tick.total < Duration::from_secs(2);
                    let (position, _) = world.update_with_facing_and_jump(
                        vec3(0.0, 0.0, 0.0),
                        vec3(0.0, 0.0, -1.0),
                        jump,
                        &mut player,
                    );
                    positions.push(position);
                }
            }
            assert_eq!(positions.len(), 180);
            let ground = positions[58].y;
            let peak = positions[60..].iter().map(|p| p.y).fold(f32::MIN, f32::max);
            assert!(peak > ground + 0.1, "jump must actually leave the ground");
            assert!((positions.last().unwrap().y - ground).abs() < 0.01);
            if let Some(expected) = &reference {
                assert_eq!(&positions, expected, "render rate {hz}");
            } else {
                reference = Some(positions);
            }
        }
    }

    #[test]
    fn irregular_intervals_keep_the_fractional_remainder() {
        let mut clock = clock(100, 8);
        let intervals = [0, 3, 27, 2, 41, 0, 26];
        assert_eq!(
            intervals
                .into_iter()
                .map(|ms| clock.advance(Duration::from_millis(ms)).count())
                .sum::<usize>(),
            9
        );
        assert_eq!(clock.total(), Duration::from_millis(90));
        assert_eq!(clock.pending(), Duration::from_millis(9));
        assert!((clock.interpolation_alpha() - 0.9).abs() < 1e-12);
        assert_eq!(clock.advance(Duration::from_millis(1)).count(), 1);
        assert_eq!(clock.pending(), Duration::ZERO);
    }

    #[test]
    fn suspended_wall_time_is_not_banked_by_the_clock() {
        let mut clock = clock(60, 8);
        assert_eq!(clock.advance(Duration::from_millis(20)).count(), 1);
        let pending = clock.pending();
        // A caller excludes paused elapsed time rather than enqueueing it.
        for _ in 0..120 * 60 {
            assert_eq!(clock.advance(Duration::ZERO).count(), 0);
        }
        assert_eq!(clock.pending(), pending);
        assert_eq!(clock.advance(Duration::from_millis(14)).count(), 1);
    }
}
