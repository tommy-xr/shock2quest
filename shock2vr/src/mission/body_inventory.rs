//! Shared body-inventory heading and refused-release latch.
use shipyard::EntityId;

pub(super) fn followed_yaw(previous: Option<f32>, observed: f32, frozen: bool, dt: f32) -> f32 {
    match previous {
        Some(yaw) if frozen => yaw,
        Some(yaw) => {
            let delta = observed - yaw;
            let delta = delta.sin().atan2(delta.cos());
            yaw + delta * (1.0 - (-dt.clamp(0.0, 0.1) / 0.5).exp())
        }
        None => observed,
    }
}

#[derive(Default)]
pub(super) struct RetainedRelease(pub [Option<EntityId>; 2]);

impl RetainedRelease {
    pub fn update(&mut self, held: [Option<EntityId>; 2], squeezed: [f32; 2]) {
        for i in 0..2 {
            if self.0[i] != held[i] || squeezed[i] > 0.5 {
                self.0[i] = None;
            }
        }
    }
    pub fn retain(&mut self, hand: usize, entity: EntityId) {
        self.0[hand] = Some(entity);
    }
    pub fn keep_grip(&self, hand: usize) -> bool {
        self.0[hand].is_some()
    }
}

/// Shared head-relative frame for adjacent belt and thigh targets. Distances
/// are metres at this boundary; gameplay uses world units internally.
#[derive(Clone, Copy)]
pub(super) struct BodyPose {
    pub head: cgmath::Vector3<f32>,
    pub yaw: f32,
}

impl BodyPose {
    pub fn front(&self, below: f32, forward: f32) -> cgmath::Vector3<f32> {
        self.head
            + cgmath::vec3(self.yaw.sin() * forward, -below, -self.yaw.cos() * forward)
                / crate::METERS_PER_WORLD_UNIT
    }
}
