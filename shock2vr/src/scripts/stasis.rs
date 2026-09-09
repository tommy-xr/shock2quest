//! Saved native AI freeze state. The timer belongs to BaseMonster, and the
//! captured model-space pose keeps animation and hitboxes identical on load.
use crate::runtime_props::RuntimePropJointTransforms;
use cgmath::Matrix4;
use serde::{Deserialize, Serialize};
use shipyard::{EntityId, Get, View, World};

#[derive(Clone, Serialize, Deserialize)]
pub struct StasisState {
    pub remaining_seconds: f32,
    pose: Option<Vec<Matrix4<f32>>>,
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
        }
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
