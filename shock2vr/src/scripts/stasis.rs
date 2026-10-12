//! Saved native AI freeze state. The timer belongs to BaseMonster, and the
//! captured model-space pose keeps animation and hitboxes identical on load.
use crate::runtime_props::RuntimePropJointTransforms;
use cgmath::Matrix4;
use serde::{Deserialize, Serialize};
use shipyard::{EntityId, Get, UniqueView, View, World};
use std::time::Duration;

#[derive(Clone, Serialize, Deserialize)]
pub struct StasisState {
    pub remaining_seconds: f32,
    pose: Option<Vec<Matrix4<f32>>>,
    visual_time: Duration,
}
impl StasisState {
    pub fn capture(world: &World, entity: EntityId, seconds: f32) -> Self {
        let pose = world
            .borrow::<View<RuntimePropJointTransforms>>()
            .unwrap()
            .get(entity)
            .ok()
            .map(|p| p.0.to_vec());
        Self {
            remaining_seconds: seconds,
            pose,
            visual_time: world
                .borrow::<UniqueView<crate::time::Time>>()
                .map(|time| time.total)
                .unwrap_or_default(),
        }
    }
    /// Simulation phase at freeze, retained with the pose across save/load.
    pub fn visual_time(&self) -> Duration {
        self.visual_time
    }
    pub fn pose(&self) -> Option<&[Matrix4<f32>; 40]> {
        self.pose.as_deref()?.try_into().ok()
    }
    pub fn valid(&self) -> bool {
        self.remaining_seconds.is_finite() && self.pose.as_ref().is_none_or(|pose| pose.len() == 40)
    }
    /// Negative authored durations are indefinite. A finite timer expires at
    /// the boundary, not one frame later.
    pub fn tick(&mut self, seconds: f32) -> bool {
        if self.remaining_seconds < 0.0 {
            return true;
        }
        self.remaining_seconds = (self.remaining_seconds - seconds).max(0.0);
        self.remaining_seconds > 0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn frozen_visual_time_round_trips_independently_of_the_live_clock() {
        let world = World::new();
        world.add_unique(crate::time::Time {
            total: Duration::from_millis(375),
            elapsed: Duration::ZERO,
        });
        let state = StasisState::capture(&world, EntityId::dead(), 8.0);
        let mut restored: StasisState =
            serde_json::from_str(&serde_json::to_string(&state).unwrap()).unwrap();
        assert!(restored.tick(2.0));
        assert_eq!(restored.visual_time(), Duration::from_millis(375));
    }
}
