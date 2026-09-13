//! Mission-local scent left by the moving player. Knowing this buffer exists
//! does not reveal its contents to AI: acquisition is local and obstruction-gated.
use std::collections::VecDeque;

use cgmath::{MetricSpace, Vector3};
use serde::{Deserialize, Serialize};
use shipyard::Unique;

const MAX_POINTS: usize = 200;
const LIFETIME: f32 = 20.0;
// At the debug/player maximum ordinary speed (10 units/s), samples stay
// one unit apart so local pickup can follow them without remote knowledge.
const SAMPLE_SECONDS: f32 = 0.1;
const SAMPLE_DISTANCE: f32 = 0.75;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ScentPoint {
    pub id: u64,
    pub position: Vector3<f32>,
    remaining: f32,
}

#[derive(Clone, Default, Debug, Serialize, Deserialize, Unique)]
pub struct PlayerTrail {
    points: VecDeque<ScentPoint>,
    next_id: u64,
    sample_elapsed: f32,
}

impl PlayerTrail {
    /// Simulation time only. Airborne/dead players age the trail without
    /// depositing it; a paused frame neither ages nor refreshes scent.
    pub fn update(&mut self, seconds: f32, position: Option<Vector3<f32>>) {
        if seconds <= 0.0 || !seconds.is_finite() {
            return;
        }
        for point in &mut self.points {
            point.remaining -= seconds;
        }
        self.points.retain(|point| point.remaining > 0.0);
        self.sample_elapsed += seconds;
        if self.sample_elapsed < SAMPLE_SECONDS {
            return;
        }
        let Some(position) = position else { return };
        if !position.x.is_finite() || !position.y.is_finite() || !position.z.is_finite() {
            return;
        }
        self.sample_elapsed = 0.0;
        if let Some(last) = self.points.back_mut() {
            if last.position.distance(position) < SAMPLE_DISTANCE {
                // Standing still refreshes one spot, not a queue of duplicate
                // destinations. Its identity is stable so AI cannot reacquire
                // this same spot forever without a new movement cue.
                last.remaining = LIFETIME;
                return;
            }
        }
        self.next_id += 1;
        self.points.push_back(ScentPoint {
            id: self.next_id,
            position,
            remaining: LIFETIME,
        });
        if self.points.len() > MAX_POINTS {
            self.points.pop_front();
        }
    }

    /// Fresh scent reaches 2.5 world units, fading to 0.75 before expiry.
    /// On first acquisition prefer the freshest LOCAL point. Once tracking,
    /// consider subsequent points in order, never walking backward in time.
    /// The caller supplies the physics test; remote points never cause rays.
    pub fn discover(
        &self,
        position: Vector3<f32>,
        after: Option<u64>,
        mut unobstructed: impl FnMut(Vector3<f32>) -> bool,
    ) -> Option<&ScentPoint> {
        let eligible = |point: &&ScentPoint| {
            after.is_none_or(|id| point.id > id)
                && position.distance(point.position) < 0.75 + 1.75 * point.remaining / LIFETIME
        };
        if after.is_some() {
            self.points
                .iter()
                .filter(eligible)
                .find(|p| unobstructed(p.position))
        } else {
            self.points
                .iter()
                .rev()
                .filter(eligible)
                .find(|p| unobstructed(p.position))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cgmath::vec3;

    #[test]
    fn scent_is_local_fades_expires_and_respects_cover() {
        let mut trail = PlayerTrail::default();
        trail.update(0.5, Some(vec3(0.0, 0.0, 0.0)));
        assert!(
            trail
                .discover(vec3(2.0, 0.0, 0.0), None, |_| true)
                .is_some()
        );
        assert!(
            trail
                .discover(vec3(0.0, 0.0, 0.0), None, |_| false)
                .is_none()
        );
        assert!(
            trail
                .discover(vec3(8.0, 0.0, 0.0), None, |_| panic!(
                    "remote scent must not raycast"
                ))
                .is_none()
        );
        trail.update(15.0, None);
        assert!(
            trail
                .discover(vec3(2.0, 0.0, 0.0), None, |_| true)
                .is_none()
        );
        assert!(
            trail
                .discover(vec3(0.5, 0.0, 0.0), None, |_| true)
                .is_some()
        );
        trail.update(5.0, None);
        assert!(trail.points.is_empty());
    }

    #[test]
    fn tracking_moves_forward_without_reacquiring_stationary_scent() {
        let mut trail = PlayerTrail::default();
        for x in 0..4 {
            trail.update(0.5, Some(vec3(x as f32, 0.0, 0.0)));
        }
        let first = trail.discover(vec3(0.0, 0.0, 0.0), None, |_| true).unwrap();
        assert_eq!(first.position.x, 2.0);
        let next = trail
            .discover(first.position, Some(first.id), |_| true)
            .unwrap();
        let id = next.id;
        assert_eq!(next.position.x, 3.0);
        trail.update(1.0, Some(vec3(3.0, 0.0, 0.0)));
        assert!(
            trail
                .discover(vec3(3.0, 0.0, 0.0), Some(id), |_| true)
                .is_none()
        );
    }

    #[test]
    fn full_speed_movement_leaves_a_locally_followable_trail() {
        let mut trail = PlayerTrail::default();
        for frame in 0..600 {
            trail.update(1.0 / 60.0, Some(vec3(frame as f32 * 10.0 / 60.0, 0.0, 0.0)));
        }
        let points: Vec<_> = trail.points.iter().collect();
        assert!(points.len() >= 80);
        for pair in points.windows(2) {
            assert!(pair[0].position.distance(pair[1].position) < 1.2);
        }
    }

    #[test]
    fn bounded_sampling_pause_and_save_round_trip() {
        let mut trail = PlayerTrail::default();
        trail.update(0.0, Some(vec3(0.0, 0.0, 0.0)));
        assert!(trail.points.is_empty());
        for x in 0..300 {
            trail.update(0.1, Some(vec3(x as f32, 0.0, 0.0)));
        }
        assert_eq!(trail.points.len(), MAX_POINTS);
        let saved = serde_json::to_string(&trail).unwrap();
        trail.update(0.0, Some(vec3(999.0, 0.0, 0.0)));
        assert_eq!(serde_json::to_string(&trail).unwrap(), saved);
        let restored: PlayerTrail = serde_json::from_str(&saved).unwrap();
        assert_eq!(serde_json::to_string(&restored).unwrap(), saved);

        // Mission save/load owns scent; carried inventory must not replace
        // the destination mission's trail during hydration.
        let mut mission = crate::save_load::EntitySaveData::empty();
        mission.player_trail = Some(restored);
        let serialized = serde_json::to_string(&mission).unwrap();
        let loaded: crate::save_load::EntitySaveData = serde_json::from_str(&serialized).unwrap();
        let mut world = shipyard::World::new();
        loaded.instantiate(&mut world);
        crate::save_load::EntitySaveData::empty().instantiate(&mut world);
        let hydrated = world.borrow::<shipyard::UniqueView<PlayerTrail>>().unwrap();
        assert_eq!(serde_json::to_string(&*hydrated).unwrap(), saved);
    }
}
