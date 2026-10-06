//! Presentation-only attachment; controller poses remain the climbing input.
use crate::{
    hand_pose::FingerAmounts,
    ladder_holds::{LadderMember, MemberKind},
    physics::ClimbGrip,
    vr_config::Handedness,
    vr_grip::GripKinematics,
    vr_support::GripPose,
};
use cgmath::{InnerSpace, Matrix, Matrix3, Quaternion, Rotation, Vector3};

const RELEASE_SECONDS: f32 = 0.12;

#[derive(Default)]
pub(super) struct ClimbVisual {
    attached: Option<GripPose>,
    release: f32,
    weight: f32,
}

impl ClimbVisual {
    pub fn update(
        &mut self,
        tracked: GripPose,
        target: Option<GripPose>,
        dt: f32,
    ) -> Option<GripPose> {
        if !tracked.is_tracked() {
            *self = Self::default();
            return None;
        }
        if let Some(target) = target {
            self.attached = Some(target);
            self.release = 0.0;
            self.weight = 1.0;
            return Some(target);
        }
        let from = self.attached?;
        self.release = (self.release + dt.max(0.0)).min(RELEASE_SECONDS);
        let t = self.release / RELEASE_SECONDS;
        self.weight = 1.0 - t * t * (3.0 - 2.0 * t);
        if self.weight <= 0.0 {
            *self = Self::default();
            return None;
        }
        Some(GripPose {
            position: tracked.position * (1.0 - self.weight) + from.position * self.weight,
            rotation: tracked
                .rotation
                .normalize()
                .slerp(from.rotation, self.weight),
        })
    }

    pub fn fingers(&self) -> Option<(FingerAmounts, f32)> {
        self.attached.map(|_| {
            (
                FingerAmounts {
                    thumb: 0.8,
                    index: 0.65,
                    middle: 0.65,
                    ring: 0.65,
                    pinky: 0.65,
                },
                self.weight,
            )
        })
    }
}

/// Align the measured glove palm with the modelled member. For a rung the
/// fingers travel upward before curling over it; a rail/pole rotates that
/// frame a quarter turn so fingers wrap horizontally with the thumb above.
pub(super) fn attached_pose(
    rig: &GripKinematics,
    hand: Handedness,
    grip: ClimbGrip,
    member: LadderMember,
) -> Option<GripPose> {
    let tip = *rig.fingers[2].first()?.last()?;
    let normal = rig.normal;
    let along = tip - rig.palm;
    let along = (along - normal * along.dot(normal)).normalize();
    let local = Matrix3::from_cols(along.cross(normal), along, normal);
    // Use the ladder's plane, not its box's cap normal. Preserve the side on
    // which the hold was acquired even while the tracked controller twists.
    let outward = member.face_normal
        * if member.face_normal.dot(grip.normal) < 0.0 {
            -1.0
        } else {
            1.0
        };
    let along = match member.kind {
        MemberKind::Rung => {
            let up = member.axis.cross(outward).normalize();
            if up.dot(Vector3::unit_y()) < 0.0 {
                -up
            } else {
                up
            }
        }
        MemberKind::Rail => {
            outward.cross(member.axis).normalize()
                * if hand == Handedness::Left { -1.0 } else { 1.0 }
        }
    };
    let normal = -outward;
    let desired = Matrix3::from_cols(along.cross(normal), along, normal);
    let rotation = Quaternion::from(desired * local.transpose()).normalize();
    // Palm clearance and the contact inside the curled knuckles are glove-scale
    // dimensions, shared by the two ladder shapes and both hands.
    let palm = grip.point + outward * 0.065 + along * 0.02;
    Some(GripPose {
        position: palm - rotation.rotate_vector(rig.palm),
        rotation,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use cgmath::{One, Quaternion, vec3};
    #[test]
    fn holds_the_bar_through_controller_drift_and_blends_back_on_release() {
        let mut visual = ClimbVisual::default();
        let tracked = GripPose {
            position: vec3(1.0, 2.0, 3.0),
            rotation: Quaternion::one(),
        };
        let attached = GripPose {
            position: vec3(1.1, 2.2, 3.0),
            rotation: Quaternion::one(),
        };
        let held = visual.update(tracked, Some(attached), 1.0 / 60.0).unwrap();
        assert_eq!(
            held.position, attached.position,
            "held glove must attach to the bar"
        );
        let moved = GripPose {
            position: vec3(1.2, 2.0, 3.0),
            ..tracked
        };
        assert_eq!(
            visual
                .update(moved, Some(attached), 1.0 / 60.0)
                .unwrap()
                .position,
            attached.position
        );
        let release = visual.update(moved, None, 1.0 / 60.0).unwrap();
        assert!(release.position.y > moved.position.y && release.position.y < attached.position.y);
        for _ in 0..12 {
            visual.update(moved, None, 1.0 / 60.0);
        }
        assert!(visual.update(moved, None, 1.0 / 60.0).is_none());
    }
    #[test]
    fn rung_and_rail_frames_follow_the_member_for_both_hands() {
        let rig = GripKinematics {
            palm: vec3(0.0, 0.0, 0.0),
            normal: Vector3::unit_z(),
            fingers: std::array::from_fn(|_| vec![vec![Vector3::unit_y()]]),
        };
        let grip = ClimbGrip {
            kind: crate::physics::ClimbGripKind::Ladder,
            entity_id: None,
            point: vec3(0.0, 0.0, 0.0),
            normal: Vector3::unit_z(),
        };
        for hand in [Handedness::Left, Handedness::Right] {
            for kind in [MemberKind::Rung, MemberKind::Rail] {
                let member = LadderMember {
                    kind,
                    axis: if kind == MemberKind::Rung {
                        Vector3::unit_x()
                    } else {
                        Vector3::unit_y()
                    },
                    face_normal: Vector3::unit_z(),
                };
                let pose = attached_pose(&rig, hand, grip, member).unwrap();
                assert!(
                    (pose.rotation.rotate_vector(rig.normal) + Vector3::unit_z()).magnitude()
                        < 1e-5
                );
                let expected = if kind == MemberKind::Rung {
                    Vector3::unit_y()
                } else if hand == Handedness::Left {
                    Vector3::unit_x()
                } else {
                    -Vector3::unit_x()
                };
                assert!(
                    (pose.rotation.rotate_vector(Vector3::unit_y()) - expected).magnitude() < 1e-5
                );
                // A rotated/translated ladder carries the entire visual frame.
                use cgmath::{Deg, Rotation3};
                let turn = Quaternion::from_angle_y(Deg(77.0));
                let offset = vec3(4.0, 2.0, -3.0);
                let rotated = attached_pose(
                    &rig,
                    hand,
                    ClimbGrip {
                        point: offset,
                        normal: turn.rotate_vector(grip.normal),
                        ..grip
                    },
                    LadderMember {
                        axis: turn.rotate_vector(member.axis),
                        face_normal: turn.rotate_vector(member.face_normal),
                        ..member
                    },
                )
                .unwrap();
                assert!(
                    (rotated.position - offset - turn.rotate_vector(pose.position)).magnitude()
                        < 1e-5
                );
            }
        }
    }

    #[test]
    fn pause_preserves_release_and_tracking_loss_clears_it() {
        let tracked = GripPose {
            position: vec3(0.0, 0.0, 0.0),
            rotation: Quaternion::one(),
        };
        let attached = GripPose {
            position: Vector3::unit_y(),
            ..tracked
        };
        let mut visual = ClimbVisual::default();
        visual.update(tracked, Some(attached), 0.0);
        assert_eq!(
            visual.update(tracked, None, 0.0).unwrap().position,
            attached.position
        );
        assert!(
            visual
                .update(
                    GripPose {
                        rotation: Quaternion::new(0.0, 0.0, 0.0, 0.0),
                        ..tracked
                    },
                    None,
                    0.1
                )
                .is_none()
        );
        assert!(visual.fingers().is_none());
        assert!(visual.update(tracked, None, 0.0).is_none());
    }
}
