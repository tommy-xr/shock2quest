//! Opt-in simulation of the Quest floor-relative tracking input.
use cgmath::{Vector3, vec3};
use serde::{Deserialize, Serialize};
use shock2vr::{
    input_context::InputContext, vr_crouch::VrCrouchDetector, vr_tracking::TrackingTransform,
};

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TrackingPatch {
    pub enabled: Option<bool>,
    pub position_tracked: Option<bool>,
    pub reset: bool,
    pub head_position: Option<[f32; 3]>,
    pub left_hand_position: Option<[f32; 3]>,
    pub right_hand_position: Option<[f32; 3]>,
}

#[derive(Debug, Default, Clone, Serialize)]
pub struct TrackingStatus {
    pub enabled: bool,
    pub position_tracked: bool,
    /// Requested floor-relative poses in meters, not pawn-local world units.
    pub head_position: Option<[f32; 3]>,
    pub left_hand_position: Option<[f32; 3]>,
    pub right_hand_position: Option<[f32; 3]>,
    pub physical_crouch: bool,
    pub explicit_crouch: bool,
    pub resolved_head_position: [f32; 3],
}

#[derive(Default)]
pub struct TrackingSimulation {
    status: TrackingStatus,
    detector: VrCrouchDetector,
    last_head: Option<[f32; 3]>,
}

impl TrackingSimulation {
    pub fn patch(&mut self, patch: TrackingPatch, vr: bool) -> Result<(), String> {
        if !vr {
            return Err("Floor-relative tracking simulation requires --vr".into());
        }
        let mut next = self.status.clone();
        if let Some(enabled) = patch.enabled {
            next.enabled = enabled;
        }
        if let Some(tracked) = patch.position_tracked {
            next.position_tracked = tracked;
        }
        for (target, pose) in [
            (&mut next.head_position, patch.head_position),
            (&mut next.left_hand_position, patch.left_hand_position),
            (&mut next.right_hand_position, patch.right_hand_position),
        ] {
            if let Some(pose) = pose {
                if !pose.iter().all(|x| x.is_finite()) {
                    return Err("Tracking positions must be finite meters".into());
                }
                *target = Some(pose);
            }
        }
        if next
            .head_position
            .is_some_and(|p| !(0.0..=3.0).contains(&p[1]))
        {
            return Err("Tracked head height must be between 0 and 3 meters".into());
        }
        if next.enabled
            && (next.head_position.is_none()
                || next.left_hand_position.is_none()
                || next.right_hand_position.is_none())
        {
            return Err("Enabling tracking requires head_position and both hand positions in floor-relative meters".into());
        }
        if !self.status.enabled && next.enabled {
            next.position_tracked = patch.position_tracked.unwrap_or(true);
        }
        if patch.reset || !next.enabled || !self.status.enabled {
            self.detector.reset();
            self.last_head = None;
            next.physical_crouch = false;
        }
        self.status = next;
        Ok(())
    }

    /// Same detector and climb-freeze policy as the Quest runtime. Invalid/lost
    /// tracking holds the last pose and stance request until tracking returns.
    pub fn update(&mut self, gripping: bool) {
        if self.status.enabled {
            if self.status.position_tracked {
                self.last_head = self.status.head_position;
            }
            self.status.physical_crouch = self.detector.update(
                self.status
                    .position_tracked
                    .then(|| self.status.head_position.unwrap()[1]),
                gripping,
            );
        }
    }

    pub fn resolve(&self, raw: &InputContext, center: f32, cap: f32) -> InputContext {
        let mut input = raw.clone();
        if self.status.enabled {
            let head = self.last_head.unwrap_or([
                0.0,
                (shock2vr::input_context::DEFAULT_HEAD_HEIGHT + center)
                    * shock2vr::METERS_PER_WORLD_UNIT,
                0.0,
            ]);
            let tracking = TrackingTransform::new(center, cap, head[1], 0.0);
            let pose = |p: [f32; 3]| tracking.stage_to_pawn(vec3(p[0], p[1], p[2]));
            input.head.position = pose(head);
            input.left_hand.position = pose(self.status.left_hand_position.unwrap());
            input.right_hand.position = pose(self.status.right_hand_position.unwrap());
            input.tracking = Some(tracking);
            input.crouch |= self.status.physical_crouch;
        }
        input
    }

    pub fn status(&self, raw: &InputContext, head: Vector3<f32>) -> TrackingStatus {
        let mut status = self.status.clone();
        status.explicit_crouch = raw.crouch;
        status.resolved_head_position = [head.x, head.y, head.z];
        status
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cgmath::InnerSpace;
    fn enabled() -> TrackingSimulation {
        let mut simulation = TrackingSimulation::default();
        simulation
            .patch(
                TrackingPatch {
                    enabled: Some(true),
                    head_position: Some([0.0, 1.7, 0.0]),
                    left_hand_position: Some([-0.2, 1.4, -0.3]),
                    right_hand_position: Some([0.2, 1.4, -0.3]),
                    ..Default::default()
                },
                true,
            )
            .unwrap();
        simulation.update(false);
        simulation
    }
    #[test]
    fn opt_in_calibration_loss_reset_and_climb_freeze_use_shared_detector() {
        let raw = InputContext::default();
        let mut simulation = enabled();
        simulation
            .patch(
                TrackingPatch {
                    head_position: Some([0.0, 1.0, 0.0]),
                    ..Default::default()
                },
                true,
            )
            .unwrap();
        simulation.update(true);
        assert!(!simulation.resolve(&raw, 1.2, 1.04).crouch);
        simulation.update(false);
        assert!(simulation.resolve(&raw, 1.2, 1.04).crouch);
        simulation
            .patch(
                TrackingPatch {
                    position_tracked: Some(false),
                    ..Default::default()
                },
                true,
            )
            .unwrap();
        simulation.update(false);
        assert!(simulation.resolve(&raw, 1.2, 1.04).crouch);
        simulation
            .patch(
                TrackingPatch {
                    reset: true,
                    position_tracked: Some(true),
                    ..Default::default()
                },
                true,
            )
            .unwrap();
        simulation.update(false);
        assert!(!simulation.resolve(&raw, 1.2, 1.04).crouch);
    }
    #[test]
    fn explicit_crouch_and_default_input_survive_without_stage_simulation() {
        let simulation = TrackingSimulation::default();
        let mut raw = InputContext::default();
        raw.crouch = true;
        raw.head.position = vec3(0.1, 0.2, 0.3);
        let resolved = simulation.resolve(&raw, 1.2, 1.04);
        assert!(resolved.crouch);
        assert_eq!(resolved.head.position, raw.head.position);
        assert!(resolved.tracking.is_none());
        assert!(enabled().resolve(&raw, 1.2, 1.04).crouch);
    }
    #[test]
    fn rejects_flat_incomplete_or_invalid_calibration_without_partial_application() {
        let mut simulation = TrackingSimulation::default();
        assert!(
            simulation
                .patch(
                    TrackingPatch {
                        enabled: Some(true),
                        ..Default::default()
                    },
                    false
                )
                .is_err()
        );
        assert!(
            simulation
                .patch(
                    TrackingPatch {
                        enabled: Some(true),
                        ..Default::default()
                    },
                    true
                )
                .is_err()
        );
        let mut simulation = enabled();
        assert!(
            simulation
                .patch(
                    TrackingPatch {
                        head_position: Some([0.0, 5.0, 0.0]),
                        ..Default::default()
                    },
                    true
                )
                .is_err()
        );
        assert_eq!(simulation.status.head_position, Some([0.0, 1.7, 0.0]));
    }
    #[test]
    fn actual_stance_caps_eye_and_moves_whole_rig_together() {
        let simulation = enabled();
        let raw = InputContext::default();
        let standing = simulation.resolve(&raw, 1.244, 1.04);
        let blocked_stand = simulation.resolve(&raw, 0.604, 0.48);
        assert!((blocked_stand.head.position.y - 0.48).abs() < 1e-5);
        assert!(
            ((standing.right_hand.position - standing.head.position)
                - (blocked_stand.right_hand.position - blocked_stand.head.position))
                .magnitude()
                < 1e-5
        );
    }
}
