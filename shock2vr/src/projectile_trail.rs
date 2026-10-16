//! Smoke left hanging along projectiles' paths, plus a glowing head on
//! traveling shots (which have no model). Puffs age on the world clock, so a
//! slowed world leaves a shot's whole flight in the air, and outlive their
//! projectile, fading out gradually.
use std::{collections::HashMap, rc::Rc, time::Duration};

use cgmath::{InnerSpace, Matrix4, Vector3, vec3};
use engine::{
    scene::{ParticleSystem, SceneObject, SceneObjectDebugTag},
    texture::TextureTrait,
};
use shipyard::EntityId;

/// World units between smoke puffs along a path.
const SMOKE_SPACING: f32 = 0.18;
/// Puffs alive per trail; past this the oldest is recycled, so a long
/// flight keeps smoke near the shot and loses it near the muzzle.
const MAX_PUFFS: usize = 80;
/// World units per world second a projectile must move to emit smoke.
const MIN_TRAIL_SPEED: f32 = 1.0;
/// Projectiles trailed at once (Quest overdraw: at most
/// `MAX_TRAILS * MAX_PUFFS` puffs). A new projectile replaces the oldest
/// trail that is only fading; if all are emitting it goes untrailed.
const MAX_TRAILS: usize = 8;
/// World length of a traveling shot's glowing head.
const HEAD_LENGTH: f32 = 0.6;

struct Trail {
    entity: EntityId,
    smoke: ParticleSystem,
    /// The projectile's last sampled position, where the emitter rests once
    /// it is gone.
    position: Vector3<f32>,
    live: bool,
    /// Emitted this tick; a trail that is not and has no puffs left is
    /// dropped, so resting projectiles do not hold trail slots.
    emitting: bool,
    /// Where it struck: smoke reaches there on the tick after it is gone.
    end: Option<Vector3<f32>>,
}

pub(crate) struct ProjectileTrails {
    trails: Vec<Trail>,
    /// Every projectile's position last tick, to tell flight from rest.
    last_seen: HashMap<EntityId, Vector3<f32>>,
    /// This tick's projectiles: `(entity, position, travel if in flight)`.
    frame: Vec<(EntityId, Vector3<f32>, Option<Vector3<f32>>)>,
    /// Each body-less shot in flight this tick: `(entity, position,
    /// heading)`. Kept apart from `trails` so the smoke cap never hides one.
    heads: Vec<(EntityId, Vector3<f32>, Vector3<f32>)>,
    /// Animated smoke sprite; empty falls back to the default particle disk.
    smoke_frames: Vec<Rc<dyn TextureTrait>>,
}

impl ProjectileTrails {
    pub fn new(smoke_frames: Vec<Rc<dyn TextureTrait>>) -> Self {
        Self {
            trails: Vec::new(),
            last_seen: HashMap::new(),
            frame: Vec::new(),
            heads: Vec::new(),
            smoke_frames,
        }
    }

    /// Advances every trail's smoke by the world `dt` and emits along the
    /// path of each projectile in `live` (`(entity, position, is_shot)`)
    /// that is in flight; one resting, jittering or creeping emits nothing.
    /// New trails' puffs last `smoke_seconds` of world time.
    pub fn update(
        &mut self,
        dt: f32,
        smoke_seconds: f32,
        live: impl IntoIterator<Item = (EntityId, Vector3<f32>, bool)>,
    ) {
        if dt <= 0.0 {
            return;
        }
        let step = Duration::from_secs_f32(dt);
        let min_step = MIN_TRAIL_SPEED * dt;
        self.frame.clear();
        self.heads.clear();
        for (entity, position, shot) in live {
            let travel = self
                .last_seen
                .get(&entity)
                .map(|previous| position - previous)
                .filter(|travel| travel.magnitude2() >= min_step * min_step);
            if let Some(travel) = travel.filter(|_| shot) {
                self.heads.push((entity, position, travel.normalize()));
            }
            self.frame.push((entity, position, travel));
        }
        // Classify every trail by this tick's projectiles first, so a new
        // projectile evicts only trails that are fading as of now.
        for trail in &mut self.trails {
            let travel = self
                .frame
                .iter()
                .find(|(entity, _, _)| *entity == trail.entity)
                .map(|(_, _, travel)| travel.is_some());
            trail.live = travel.is_some();
            trail.emitting = travel == Some(true);
        }
        for index in 0..self.frame.len() {
            let (entity, position, travel) = self.frame[index];
            let trail = match self.trails.iter().position(|trail| trail.entity == entity) {
                Some(index) => &mut self.trails[index],
                None if travel.is_some() && self.make_room() => {
                    let start = position - travel.unwrap();
                    let mut smoke = smoke_puffs(&self.smoke_frames, smoke_seconds);
                    // Prime the emitter where the projectile was, so the
                    // first puffs fill the path from there.
                    smoke.update(Duration::ZERO, Matrix4::from_translation(start));
                    self.trails.push(Trail {
                        entity,
                        smoke,
                        position: start,
                        live: true,
                        emitting: true,
                        end: None,
                    });
                    self.trails.last_mut().unwrap()
                }
                None => continue,
            };
            trail.smoke.set_emitting(travel.is_some());
            if let Some(travel) = travel {
                // One puff per SMOKE_SPACING at this tick's speed.
                let interval = SMOKE_SPACING * dt / travel.magnitude();
                trail
                    .smoke
                    .set_launch_time(Duration::from_secs_f32(interval.max(1.0e-4)));
            }
            trail
                .smoke
                .update(step, Matrix4::from_translation(position));
            trail.position = position;
        }
        for trail in self.trails.iter_mut().filter(|trail| !trail.live) {
            // Gone: draw the last stretch to the impact, then only fade.
            let end = trail.end.take();
            if let Some(end) = end {
                let interval = SMOKE_SPACING * dt / (end - trail.position).magnitude().max(1.0e-4);
                trail
                    .smoke
                    .set_launch_time(Duration::from_secs_f32(interval.max(1.0e-4)));
            }
            trail.smoke.set_emitting(end.is_some());
            trail.position = end.unwrap_or(trail.position);
            trail
                .smoke
                .update(step, Matrix4::from_translation(trail.position));
            trail.smoke.set_emitting(false);
        }
        self.trails
            .retain(|trail| trail.emitting || trail.smoke.particle_count() > 0);
        self.last_seen.clear();
        self.last_seen.extend(
            self.frame
                .iter()
                .map(|&(entity, position, _)| (entity, position)),
        );
    }

    /// Whether a new trail fits, evicting the oldest one that is only
    /// fading if not.
    fn make_room(&mut self) -> bool {
        if self.trails.len() < MAX_TRAILS {
            return true;
        }
        match self.trails.iter().position(|trail| !trail.emitting) {
            Some(index) => {
                self.trails.remove(index);
                true
            }
            None => false,
        }
    }

    /// Records where `entity` struck, so its smoke reaches the impact.
    pub fn end(&mut self, entity: EntityId, point: Vector3<f32>) {
        if let Some(trail) = self.trails.iter_mut().find(|trail| trail.entity == entity) {
            trail.end = Some(point);
        }
    }

    /// Every trail's smoke, and with `heads` a short glowing streak leading
    /// each traveling shot in flight.
    pub fn render(&self, heads: bool) -> Vec<SceneObject> {
        // Lets `/v1/scene` attribute each object to its projectile.
        let tag = |entity: EntityId, source: &str| {
            Some(Rc::new(SceneObjectDebugTag {
                entity_id: Some(entity.inner()),
                source: Some(source.into()),
                ..Default::default()
            }))
        };
        let mut objects = Vec::new();
        for trail in &self.trails {
            let smoke_tag = tag(trail.entity, "projectile_smoke");
            for mut puff in trail.smoke.render() {
                puff.set_debug_tag(smoke_tag.clone());
                objects.push(puff);
            }
        }
        for &(entity, position, heading) in self.heads.iter().filter(|_| heads) {
            // The beam fades toward its far end, so drawing it backward from
            // the shot leaves a bright tip and a fading tail.
            let head_tag = tag(entity, "projectile_head");
            for mut object in engine::scene::laser_material::beam(
                position,
                -heading * HEAD_LENGTH,
                (0.03, vec3(1.0, 0.75, 0.4)),
                (0.006, vec3(1.0, 0.95, 0.85)),
                0.0,
            ) {
                object.set_debug_tag(head_tag.clone());
                objects.push(object);
            }
        }
        objects
    }

    #[cfg(test)]
    fn puff_counts(&self) -> Vec<(EntityId, usize)> {
        self.trails
            .iter()
            .map(|trail| (trail.entity, trail.smoke.particle_count()))
            .collect()
    }
}

/// Grey smoke puffs (the caller sets the launch period each tick), drifting
/// and swelling a little and fading over their whole life.
fn smoke_puffs(frames: &[Rc<dyn TextureTrait>], seconds: f32) -> ParticleSystem {
    let smoke = ParticleSystem::new()
        .with_world_space(true)
        .with_num_particles(MAX_PUFFS)
        .with_recycle_oldest(true)
        .with_launch_bounding_box(vec3(-0.05, -0.05, -0.05), vec3(0.05, 0.05, 0.05))
        .with_velocity(vec3(-0.08, -0.02, -0.08), vec3(0.08, 0.1, 0.08))
        .with_lifetime(seconds * 0.8, seconds)
        .with_particle_size(0.25, 0.45)
        .with_size_velocity(0.4)
        .with_color(vec3(0.8, 0.8, 0.8))
        .with_alpha(0.5)
        .with_fade_in_time(0.03)
        .with_fade_time(seconds);
    if frames.is_empty() {
        return smoke;
    }
    // Play the smoke animation once over a puff's life.
    let frame_time = Duration::from_secs_f32(seconds / frames.len() as f32);
    smoke.with_sprite_animation(frames.to_vec(), frame_time, false)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DT: f32 = 1.0 / 60.0;
    const SMOKE: f32 = 1.0;

    fn id(n: u64) -> EntityId {
        EntityId::from_inner(n).unwrap()
    }

    fn trails() -> ProjectileTrails {
        ProjectileTrails::new(Vec::new())
    }

    /// Flies entity 1 along +X at `speed` for `ticks`, then removes it.
    fn fly(trails: &mut ProjectileTrails, speed: f32, ticks: usize) {
        for i in 0..=ticks {
            let x = i as f32 * speed * DT;
            trails.update(DT, SMOKE, [(id(1), vec3(x, 0.0, 0.0), true)]);
        }
    }

    #[test]
    fn smoke_fills_the_path_from_the_first_position() {
        let mut trails = trails();
        // 6 world units at 0.18 spacing.
        fly(&mut trails, 36.0, 10);
        let puffs = trails.puff_counts()[0].1;
        assert!((32..=35).contains(&puffs), "{puffs}");
    }

    #[test]
    fn spacing_follows_the_speed_of_each_tick() {
        let mut trails = trails();
        // A short first step (a shot leaving the muzzle), then full speed:
        // 0.06 + 10 * 0.6 = 6.06 world units.
        let mut x = 0.0;
        trails.update(DT, SMOKE, [(id(1), vec3(x, 0.0, 0.0), true)]);
        x += 0.06;
        trails.update(DT, SMOKE, [(id(1), vec3(x, 0.0, 0.0), true)]);
        for _ in 0..10 {
            x += 0.6;
            trails.update(DT, SMOKE, [(id(1), vec3(x, 0.0, 0.0), true)]);
        }
        let puffs = trails.puff_counts()[0].1;
        assert!((31..=35).contains(&puffs), "{puffs}");
    }

    #[test]
    fn a_long_flight_is_capped() {
        let mut trails = trails();
        fly(&mut trails, 120.0, 30);
        assert_eq!(trails.puff_counts(), vec![(id(1), MAX_PUFFS)]);
    }

    #[test]
    fn smoke_outlives_its_projectile_then_clears() {
        let mut trails = trails();
        fly(&mut trails, 36.0, 10);
        let puffs = trails.puff_counts()[0].1;
        // Gone: no new puffs, and every puff fades within SMOKE seconds.
        trails.update(DT, SMOKE, []);
        assert_eq!(trails.puff_counts(), vec![(id(1), puffs)]);
        for _ in 0..(SMOKE / DT) as usize + 2 {
            trails.update(DT, SMOKE, []);
        }
        assert!(trails.puff_counts().is_empty());
    }

    #[test]
    fn an_impact_draws_smoke_to_the_hit_point() {
        let mut trails = trails();
        fly(&mut trails, 36.0, 10);
        let before = trails.puff_counts()[0].1;
        // Struck one step past its last sampled point.
        trails.end(id(1), vec3(6.6, 0.0, 0.0));
        trails.update(DT, SMOKE, []);
        let after = trails.puff_counts()[0].1;
        assert!(after > before, "{before} -> {after}");
        // Without an impact, a vanished projectile adds nothing.
        trails.update(DT, SMOKE, []);
        assert_eq!(trails.puff_counts()[0].1, after);
    }

    #[test]
    fn a_resting_or_creeping_projectile_emits_nothing() {
        let mut trails = trails();
        for i in 0..30 {
            let x = i as f32 * 0.5 * MIN_TRAIL_SPEED * DT;
            trails.update(DT, SMOKE, [(id(1), vec3(x, 0.0, 0.0), false)]);
        }
        assert!(trails.puff_counts().is_empty());
    }

    #[test]
    fn a_paused_world_changes_nothing() {
        let mut trails = trails();
        fly(&mut trails, 36.0, 5);
        let puffs = trails.puff_counts();
        for _ in 0..100 {
            trails.update(0.0, SMOKE, [(id(1), vec3(50.0, 0.0, 0.0), true)]);
        }
        assert_eq!(trails.puff_counts(), puffs);
    }

    #[test]
    fn a_new_shot_replaces_trails_whose_projectiles_just_vanished() {
        let mut trails = trails();
        let ids = || 1..=(MAX_TRAILS as u64);
        trails.update(DT, SMOKE, ids().map(|n| (id(n), vec3(0.0, 0.0, 0.0), true)));
        trails.update(DT, SMOKE, ids().map(|n| (id(n), vec3(0.0, 0.0, 1.0), true)));
        // The same tick they all vanish, a new shot appears and moves.
        let late = id(MAX_TRAILS as u64 + 1);
        trails.update(DT, SMOKE, [(late, vec3(0.0, 0.0, 0.0), true)]);
        trails.update(DT, SMOKE, [(late, vec3(0.0, 0.0, 1.0), true)]);
        assert!(trails.trails.iter().any(|trail| trail.entity == late));
        assert_eq!(trails.trails.len(), MAX_TRAILS);
    }

    #[test]
    fn a_new_shot_never_evicts_a_trail_resuming_flight() {
        let mut trails = trails();
        let ids = || 1..=(MAX_TRAILS as u64);
        let at = |z: f32| ids().map(move |n| (id(n), vec3(n as f32, 0.0, z), true));
        trails.update(DT, SMOKE, at(0.0));
        trails.update(DT, SMOKE, at(1.0));
        // All pause for a tick, then resume; a new shot is listed first.
        trails.update(DT, SMOKE, at(1.0));
        let late = id(MAX_TRAILS as u64 + 1);
        trails.update(
            DT,
            SMOKE,
            [(late, vec3(0.0, 9.0, 0.0), true)]
                .into_iter()
                .chain(at(1.0)),
        );
        trails.update(
            DT,
            SMOKE,
            [(late, vec3(0.0, 9.0, 1.0), true)]
                .into_iter()
                .chain(at(2.0)),
        );
        assert!(trails.trails.iter().all(|trail| trail.entity != late));
        assert_eq!(trails.trails.len(), MAX_TRAILS);
    }

    #[test]
    fn trails_are_capped() {
        let mut trails = trails();
        let ids = || 1..=(MAX_TRAILS as u64 + 4);
        trails.update(DT, SMOKE, ids().map(|n| (id(n), vec3(0.0, 0.0, 0.0), true)));
        trails.update(DT, SMOKE, ids().map(|n| (id(n), vec3(0.0, 0.0, 1.0), true)));
        assert_eq!(trails.trails.len(), MAX_TRAILS);
    }

    #[test]
    fn only_a_flying_shot_has_a_head() {
        let mut trails = trails();
        trails.update(
            DT,
            SMOKE,
            [
                (id(1), vec3(0.0, 0.0, 0.0), true),
                (id(2), vec3(0.0, 0.0, 0.0), false),
            ],
        );
        trails.update(
            DT,
            SMOKE,
            [
                (id(1), vec3(0.0, 0.0, 1.0), true),
                (id(2), vec3(0.0, 0.0, 1.0), false),
            ],
        );
        assert_eq!(
            trails.heads,
            vec![(id(1), vec3(0.0, 0.0, 1.0), vec3(0.0, 0.0, 1.0))]
        );
        trails.update(DT, SMOKE, []);
        assert!(trails.heads.is_empty());
    }

    #[test]
    fn the_smoke_cap_does_not_hide_a_shot() {
        let mut trails = trails();
        let ids = || 1..=(MAX_TRAILS as u64 + 1);
        trails.update(DT, SMOKE, ids().map(|n| (id(n), vec3(0.0, 0.0, 0.0), true)));
        trails.update(DT, SMOKE, ids().map(|n| (id(n), vec3(0.0, 0.0, 1.0), true)));
        assert_eq!(trails.trails.len(), MAX_TRAILS);
        assert_eq!(trails.heads.len(), MAX_TRAILS + 1);
    }

    #[test]
    fn a_projectile_come_to_rest_frees_its_slot() {
        let mut trails = trails();
        fly(&mut trails, 36.0, 3);
        // It lands and stays put, alive, until its smoke is gone.
        for _ in 0..(SMOKE / DT) as usize + 2 {
            trails.update(DT, SMOKE, [(id(1), vec3(1.8, 0.0, 0.0), true)]);
        }
        assert!(trails.trails.is_empty());
    }
}
