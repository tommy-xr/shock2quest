//! Fading streaks behind projectiles. Samples age on the world clock, so a
//! slowed world keeps them on screen longer, and outlive their projectile so
//! a shot that hits still leaves its line.
use std::collections::VecDeque;

use cgmath::{InnerSpace, Vector3, vec3};
use engine::scene::SceneObject;
use shipyard::EntityId;

/// World seconds a trail sample lasts.
pub(crate) const TRAIL_LIFETIME: f32 = 0.3;
/// Samples kept per trail; older ones are dropped first.
const MAX_SAMPLES: usize = 10;
/// Projectiles trailed at once; further ones go untrailed (Quest budget).
const MAX_TRAILS: usize = 24;

struct Sample {
    position: Vector3<f32>,
    age: f32,
}

struct Trail {
    entity: EntityId,
    /// Whether `entity` was sampled this tick; once it is not, the trail only
    /// fades.
    live: bool,
    samples: VecDeque<Sample>,
}

#[derive(Default)]
pub(crate) struct ProjectileTrails {
    trails: Vec<Trail>,
}

impl ProjectileTrails {
    /// Ages every sample by the world `dt` and records each live projectile's
    /// current position.
    pub fn update(&mut self, dt: f32, live: impl IntoIterator<Item = (EntityId, Vector3<f32>)>) {
        for trail in &mut self.trails {
            trail.live = false;
            for sample in &mut trail.samples {
                sample.age += dt;
            }
            while trail
                .samples
                .front()
                .is_some_and(|sample| sample.age >= TRAIL_LIFETIME)
            {
                trail.samples.pop_front();
            }
        }
        for (entity, position) in live {
            let index = match self.trails.iter().position(|trail| trail.entity == entity) {
                Some(index) => index,
                None if self.trails.len() < MAX_TRAILS => {
                    self.trails.push(Trail {
                        entity,
                        live: false,
                        samples: VecDeque::with_capacity(MAX_SAMPLES),
                    });
                    self.trails.len() - 1
                }
                None => continue,
            };
            let trail = &mut self.trails[index];
            trail.live = true;
            // A paused world or a resting projectile adds nothing new.
            if trail
                .samples
                .back()
                .is_some_and(|last| (last.position - position).magnitude2() < 1.0e-8)
            {
                continue;
            }
            if trail.samples.len() == MAX_SAMPLES {
                trail.samples.pop_front();
            }
            trail.samples.push_back(Sample { position, age: 0.0 });
        }
        self.trails
            .retain(|trail| trail.live || !trail.samples.is_empty());
    }

    /// One thin beam per pair of consecutive samples, thinning and fading
    /// with the age of its older end.
    pub fn render(&self) -> Vec<SceneObject> {
        let mut objects = Vec::new();
        for trail in &self.trails {
            for (older, newer) in trail.samples.iter().zip(trail.samples.iter().skip(1)) {
                let reach = newer.position - older.position;
                if reach.magnitude2() < 1.0e-8 {
                    continue;
                }
                let fade = (older.age / TRAIL_LIFETIME).clamp(0.0, 1.0);
                let width = 1.0 - 0.7 * fade;
                for mut object in engine::scene::laser_material::beam(
                    older.position,
                    reach,
                    (0.02 * width, vec3(1.0, 0.7, 0.35)),
                    (0.004 * width, vec3(1.0, 0.92, 0.75)),
                    0.0,
                ) {
                    object.set_transparency(Some(fade));
                    objects.push(object);
                }
            }
        }
        objects
    }

    #[cfg(test)]
    fn sample_counts(&self) -> Vec<(EntityId, usize)> {
        self.trails
            .iter()
            .map(|trail| (trail.entity, trail.samples.len()))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DT: f32 = 1.0 / 60.0;

    fn id(n: u64) -> EntityId {
        EntityId::from_inner(n).unwrap()
    }

    #[test]
    fn samples_are_capped_and_the_oldest_dropped() {
        let mut trails = ProjectileTrails::default();
        for i in 0..(MAX_SAMPLES + 5) {
            trails.update(0.001, [(id(1), vec3(i as f32, 0.0, 0.0))]);
        }
        assert_eq!(trails.sample_counts(), vec![(id(1), MAX_SAMPLES)]);
        assert_eq!(
            trails.trails[0].samples.front().map(|s| s.position.x),
            Some(5.0)
        );
    }

    #[test]
    fn a_trail_outlives_its_projectile_until_its_samples_age_out() {
        let mut trails = ProjectileTrails::default();
        trails.update(DT, [(id(1), vec3(0.0, 0.0, 0.0))]);
        trails.update(DT, [(id(1), vec3(0.0, 0.0, 1.0))]);
        // The projectile is gone: the trail stays until TRAIL_LIFETIME.
        let mut ticks = 0;
        while !trails.trails.is_empty() {
            trails.update(DT, []);
            ticks += 1;
            assert!(ticks < 100);
        }
        let expected = (TRAIL_LIFETIME / DT).ceil() as usize;
        assert!(
            (ticks as i32 - expected as i32).abs() <= 1,
            "{ticks} vs {expected}"
        );
    }

    #[test]
    fn a_paused_world_neither_ages_nor_extends_a_trail() {
        let mut trails = ProjectileTrails::default();
        trails.update(DT, [(id(1), vec3(0.0, 0.0, 0.0))]);
        for _ in 0..1000 {
            trails.update(0.0, [(id(1), vec3(0.0, 0.0, 0.0))]);
        }
        assert_eq!(trails.sample_counts(), vec![(id(1), 1)]);
        assert_eq!(trails.trails[0].samples[0].age, 0.0);
    }

    #[test]
    fn trails_are_capped() {
        let mut trails = ProjectileTrails::default();
        trails.update(
            DT,
            (1..=(MAX_TRAILS as u64 + 4)).map(|n| (id(n), vec3(0.0, 0.0, 0.0))),
        );
        assert_eq!(trails.trails.len(), MAX_TRAILS);
    }
}
