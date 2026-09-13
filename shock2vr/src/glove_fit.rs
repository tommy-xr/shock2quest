//! Global hand-frame calibration, plus fit-scene-only visual previews.
use cgmath::{Matrix4, Quaternion, Rotation, Vector3, vec3};

use crate::{dev_params, vr_config::Handedness, vr_support::GripPose};

/// Controller-to-hand translation, in the rotation's space and world units.
/// Subtract it when publishing a raw controller target for a calibrated grip.
pub(crate) fn forward_translation(rotation: Quaternion<f32>, forward_cm: f32) -> Vector3<f32> {
    crate::util::tracked_rotation(rotation).map_or(vec3(0.0, 0.0, 0.0), |rotation| {
        rotation.rotate_vector(vec3(
            0.0,
            0.0,
            -forward_cm * 0.01 / crate::METERS_PER_WORLD_UNIT,
        ))
    })
}

/// Translate the consumed hand frame once, before menus and gameplay diverge.
/// Cached grip samples and wrist mounts remain relative to that frame, so live
/// tuning moves glove, held item and interactions together without rebaking.
/// Work on a copy: callers retain raw tracking and repeated updates never drift.
pub(crate) fn calibrated_input(
    input: &crate::input_context::InputContext,
    presentation: crate::PresentationMode,
    fit_scene: bool,
    forward_cm: f32,
) -> crate::input_context::InputContext {
    let mut calibrated = input.clone();
    if presentation == crate::PresentationMode::Vr || fit_scene {
        for (index, hand) in [&mut calibrated.left_hand, &mut calibrated.right_hand]
            .into_iter()
            .enumerate()
        {
            if input
                .pose_tracking
                .is_some_and(|tracking| !tracking.hands[index])
            {
                continue;
            }
            hand.position += forward_translation(hand.rotation, forward_cm);
        }
    }
    calibrated
}

#[derive(Clone, Copy)]
pub(crate) struct GloveFit {
    pub side_cm: f32,
    pub up_cm: f32,
    pub size: f32,
    pub visible: bool,
}

impl GloveFit {
    pub fn current() -> Self {
        Self {
            side_cm: dev_params::get(dev_params::GLOVE_SIDE_CM),
            up_cm: dev_params::get(dev_params::GLOVE_UP_CM),
            size: dev_params::get(dev_params::GLOVE_FIT_SIZE),
            visible: dev_params::get_bool(dev_params::GLOVE_FIT_VISIBLE),
        }
    }

    /// Physical centimeters in controller space; mirror lateral movement so
    /// both hands can be fitted symmetrically. Scale about the shifted origin.
    pub fn transform(self, pose: GripPose, side: Handedness) -> Matrix4<f32> {
        let offset = side.mirror_point(vec3(self.side_cm, self.up_cm, 0.0))
            * (0.01 / crate::METERS_PER_WORLD_UNIT);
        Matrix4::from_translation(pose.position)
            * Matrix4::from(pose.rotation)
            * Matrix4::from_translation(offset)
            * Matrix4::from_scale(self.size)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cgmath::{Deg, InnerSpace, Quaternion, Rotation, Rotation3, Transform, point3};

    #[test]
    fn global_forward_preserves_tracking_and_calibrates_each_hand_once() {
        use crate::{
            PresentationMode,
            input_context::{InputContext, PoseTracking},
        };
        let mut raw = InputContext::default();
        raw.left_hand.position = vec3(-0.3, 0.7, -0.4);
        raw.right_hand.position = vec3(0.3, 0.7, -0.4);
        raw.left_hand.rotation = Quaternion::from_angle_y(Deg(90.0));
        raw.right_hand.rotation = Quaternion::from_angle_x(Deg(45.0));
        raw.pose_tracking = Some(PoseTracking {
            head: true,
            hands: [true, true],
        });
        for presentation in [PresentationMode::Vr, PresentationMode::Flat] {
            for fit_scene in [false, true] {
                let first = calibrated_input(&raw, presentation, fit_scene, -15.0);
                let again = calibrated_input(&raw, presentation, fit_scene, -15.0);
                let zero = calibrated_input(&raw, presentation, fit_scene, 0.0);
                assert_eq!(first.head.position, raw.head.position);
                assert_eq!(first.head.rotation, raw.head.rotation);
                for (original, (actual, repeated)) in
                    [&raw.left_hand, &raw.right_hand].into_iter().zip([
                        (&first.left_hand, &again.left_hand),
                        (&first.right_hand, &again.right_hand),
                    ])
                {
                    let expected_meters = if presentation == PresentationMode::Vr || fit_scene {
                        original.rotation.rotate_vector(vec3(0.0, 0.0, 0.15))
                    } else {
                        vec3(0.0, 0.0, 0.0)
                    };
                    assert!(
                        ((actual.position - original.position) * crate::METERS_PER_WORLD_UNIT
                            - expected_meters)
                            .magnitude()
                            < 1e-6
                    );
                    assert_eq!(actual.position, repeated.position);
                    assert_eq!(actual.rotation, original.rotation);
                }
                assert_eq!(zero.left_hand.position, raw.left_hand.position);
                assert_eq!(zero.right_hand.position, raw.right_hand.position);
            }
        }
        raw.pose_tracking.as_mut().unwrap().hands[0] = false;
        let adjusted = calibrated_input(&raw, PresentationMode::Vr, false, -15.0);
        assert_eq!(adjusted.left_hand.position, raw.left_hand.position);
        assert_eq!(raw.left_hand.position, vec3(-0.3, 0.7, -0.4));
    }

    #[test]
    fn calibration_is_physical_mirrored_and_rotates_with_the_hand() {
        let baseline = GloveFit {
            side_cm: 0.0,
            up_cm: 0.0,
            size: 1.0,
            visible: true,
        };
        for roll in [0.0, 90.0, 180.0] {
            let pose = GripPose {
                position: vec3(3.0, 2.0, -4.0),
                rotation: Quaternion::from_angle_y(Deg(30.0)) * Quaternion::from_angle_z(Deg(roll)),
            };
            let origin = point3(0.0, 0.0, 0.0);
            let sample = point3(0.02, 0.1, -0.2);
            for side in [Handedness::Left, Handedness::Right] {
                let base = baseline.transform(pose, side);
                let production =
                    Matrix4::from_translation(pose.position) * Matrix4::from(pose.rotation);
                assert!(
                    (base.transform_point(sample) - production.transform_point(sample)).magnitude()
                        < 1e-6
                );
                for size in [0.5, 1.0, 1.5] {
                    let fit = GloveFit {
                        side_cm: 2.0,
                        up_cm: 3.0,
                        size,
                        ..baseline
                    };
                    let transformed = fit.transform(pose, side);
                    let delta = (transformed.transform_point(origin)
                        - base.transform_point(origin))
                        * crate::METERS_PER_WORLD_UNIT;
                    let expected = pose
                        .rotation
                        .rotate_vector(side.mirror_point(vec3(0.02, 0.03, 0.0)));
                    assert!((delta - expected).magnitude() < 1e-6);
                    let length = (transformed.transform_point(sample)
                        - transformed.transform_point(origin))
                    .magnitude();
                    assert!((length - (sample - origin).magnitude() * size).abs() < 1e-6);
                }
            }
        }
    }
}
