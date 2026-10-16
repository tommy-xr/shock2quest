//! Fading streaks behind projectiles. Samples age on the world clock, so a
//! slowed world keeps them on screen longer, and outlive their projectile so
//! a shot that hits still leaves its line.
use std::collections::{HashMap, VecDeque};

use cgmath::{InnerSpace, Vector3, vec3};
use engine::scene::{SceneObject, SceneObjectDebugTag};
use shipyard::EntityId;

/// World seconds a trail sample lasts.
pub(crate) const TRAIL_LIFETIME: f32 = 0.3;
/// Samples kept per trail; older ones are dropped first.
const MAX_SAMPLES: usize = 10;
/// World units per world second a projectile must move to extend its trail.
const MIN_TRAIL_SPEED: f32 = 1.0;
/// Projectiles trailed at once; further ones go untrailed (Quest budget).
const MAX_TRAILS: usize = 24;

struct Sample {
    position: Vector3<f32>,
    age: f32,
}

struct Trail {
    entity: EntityId,
    samples: VecDeque<Sample>,
}

#[derive(Default)]
pub(crate) struct ProjectileTrails {
    trails: Vec<Trail>,
    /// Every projectile's position last tick, to tell flight from rest.
    last_seen: HashMap<EntityId, Vector3<f32>>,
    seen: HashMap<EntityId, Vector3<f32>>,
}

impl ProjectileTrails {
    /// Ages every sample by the world `dt` and extends the trail of each
    /// projectile in `live` that is in flight; one resting, jittering or
    /// creeping adds nothing.
    pub fn update(&mut self, dt: f32, live: impl IntoIterator<Item = (EntityId, Vector3<f32>)>) {
        if dt <= 0.0 {
            return;
        }
        for trail in &mut self.trails {
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
        self.trails.retain(|trail| !trail.samples.is_empty());

        let min_step = MIN_TRAIL_SPEED * dt;
        self.seen.clear();
        for (entity, position) in live {
            self.seen.insert(entity, position);
            let Some(&previous) = self.last_seen.get(&entity) else {
                continue;
            };
            if (position - previous).magnitude2() < min_step * min_step {
                continue;
            }
            let trail = match self.trails.iter().position(|trail| trail.entity == entity) {
                Some(index) => &mut self.trails[index],
                None if self.trails.len() < MAX_TRAILS => {
                    let mut samples = VecDeque::with_capacity(MAX_SAMPLES);
                    samples.push_back(Sample {
                        position: previous,
                        age: dt,
                    });
                    self.trails.push(Trail { entity, samples });
                    self.trails.last_mut().unwrap()
                }
                None => continue,
            };
            if trail.samples.len() == MAX_SAMPLES {
                trail.samples.pop_front();
            }
            trail.samples.push_back(Sample { position, age: 0.0 });
        }
        std::mem::swap(&mut self.last_seen, &mut self.seen);
    }

    /// One thin beam per trail, drawn from the newest sample back to the
    /// oldest so the beam's far-end falloff tapers the tail; the whole trail
    /// fades with the age of its newest sample once the projectile stops.
    pub fn render(&self) -> Vec<SceneObject> {
        let mut objects = Vec::new();
        for trail in &self.trails {
            let (Some(tail), Some(head)) = (trail.samples.front(), trail.samples.back()) else {
                continue;
            };
            let reach = tail.position - head.position;
            if reach.magnitude2() < 1.0e-8 {
                continue;
            }
            let fade = (head.age / TRAIL_LIFETIME).clamp(0.0, 1.0);
            // Lets `/v1/scene` attribute the beam to its projectile.
            let tag = std::rc::Rc::new(SceneObjectDebugTag {
                entity_id: Some(trail.entity.inner()),
                source: Some("projectile_trail".into()),
                ..Default::default()
            });
            for mut object in engine::scene::laser_material::beam(
                head.position,
                reach,
                (0.025, vec3(1.0, 0.7, 0.35)),
                (0.005, vec3(1.0, 0.92, 0.75)),
                0.0,
            ) {
                object.set_transparency(Some(fade));
                object.set_debug_tag(Some(tag.clone()));
                objects.push(object);
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
        trails.update(DT, [(id(1), vec3(0.0, 0.0, 1.0))]);
        for _ in 0..1000 {
            trails.update(0.0, [(id(1), vec3(0.0, 0.0, 1.0))]);
        }
        assert_eq!(trails.sample_counts(), vec![(id(1), 2)]);
        assert_eq!(trails.trails[0].samples[1].age, 0.0);
    }

    #[test]
    fn a_creeping_projectile_does_not_grow_a_trail() {
        let mut trails = ProjectileTrails::default();
        // Half of MIN_TRAIL_SPEED.
        for i in 0..30 {
            let x = i as f32 * 0.5 * MIN_TRAIL_SPEED * DT;
            trails.update(DT, [(id(1), vec3(x, 0.0, 0.0))]);
        }
        assert!(trails.sample_counts().is_empty());
    }

    #[test]
    fn trails_are_capped() {
        let mut trails = ProjectileTrails::default();
        let ids = || 1..=(MAX_TRAILS as u64 + 4);
        trails.update(DT, ids().map(|n| (id(n), vec3(0.0, 0.0, 0.0))));
        trails.update(DT, ids().map(|n| (id(n), vec3(0.0, 0.0, 1.0))));
        assert_eq!(trails.trails.len(), MAX_TRAILS);
    }
}
