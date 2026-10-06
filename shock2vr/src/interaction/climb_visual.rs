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

/// The wrist is sampled when the hold is acquired, not while the controller
/// twists or the pawn turns underneath a hand that is still attached.
#[derive(Clone, Copy)]
pub(super) struct HeldMember {
    pub grip: ClimbGrip,
    pub member: LadderMember,
    pub wrist_rotation: Quaternion<f32>,
}

impl HeldMember {
    pub fn capture(
        previous: Option<Self>,
        grip: ClimbGrip,
        member: LadderMember,
        wrist_rotation: Quaternion<f32>,
    ) -> Self {
        Self {
            grip,
            member,
            wrist_rotation: previous
                .filter(|held| {
                    held.grip.entity_id == grip.entity_id && held.grip.point == grip.point
                })
                .map_or(wrist_rotation, |held| held.wrist_rotation),
        }
    }
}

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
/// fingers travel upward before curling over it. A rail/pole aligns the
/// knuckles with its axis and chooses the wrap closest to the acquired wrist.
/// Both wraps keep the palm on the acquired face; the hint must not move it
/// behind a rail or into the wall supporting it.
pub(super) fn attached_pose(
    rig: &GripKinematics,
    hand: Handedness,
    grip: ClimbGrip,
    member: LadderMember,
    wrist_rotation: Quaternion<f32>,
) -> Option<GripPose> {
    let tip = *rig.fingers[2].first()?.last()?;
    let normal = rig.normal;
    let along = tip - rig.palm;
    let along = (along - normal * along.dot(normal)).normalize();
    let local = Matrix3::from_cols(along.cross(normal), along, normal);
    // Use the ladder's plane, not its box's cap normal. Preserve the side on
    // which the hold was acquired even while the tracked controller twists.
    let face_outward = member.face_normal
        * if member.face_normal.dot(grip.normal) < 0.0 {
            -1.0
        } else {
            1.0
        };
    let rotation = match member.kind {
        MemberKind::Rung => {
            let up = member.axis.cross(face_outward).normalize();
            let along = if up.dot(Vector3::unit_y()) < 0.0 {
                -up
            } else {
                up
            };
            let normal = -face_outward;
            let desired = Matrix3::from_cols(along.cross(normal), along, normal);
            Quaternion::from(desired * local.transpose()).normalize()
        }
        MemberKind::Rail => {
            let wrist = GripPose {
                position: grip.point,
                rotation: wrist_rotation,
            };
            if !wrist.is_tracked() {
                return None;
            }
            let wrist = wrist_rotation.normalize();
            let along = face_outward.cross(member.axis).normalize()
                * if hand == Handedness::Left { -1.0 } else { 1.0 };
            let normal = -face_outward;
            let wrap = |along: Vector3<f32>| {
                Quaternion::from(
                    Matrix3::from_cols(along.cross(normal), along, normal) * local.transpose(),
                )
                .normalize()
            };
            let (upright, reversed) = (wrap(along), wrap(-along));
            let difference = reversed.dot(wrist).abs() - upright.dot(wrist).abs();
            // Quaternion scores tie when the tracked palm faces away from
            // the pole. Its finger direction still supplies a useful hint.
            if difference > 1e-5
                || (difference.abs() <= 1e-5 && wrist.rotate_vector(local.y).dot(along) < -1e-5)
            {
                reversed
            } else {
                upright
            }
        }
    };
    let outward = -rotation.rotate_vector(normal);
    let along = rotation.rotate_vector(along);
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
    fn rail_preserves_an_already_aligned_wrist_from_either_side() {
        use cgmath::{Deg, Rotation3};
        let rig = GripKinematics {
            palm: vec3(0.1, 0.2, 0.3),
            normal: Vector3::unit_z(),
            fingers: std::array::from_fn(|_| vec![vec![vec3(0.1, 1.2, 0.3)]]),
        };
        let grip = ClimbGrip {
            kind: crate::physics::ClimbGripKind::Ladder,
            entity_id: None,
            point: vec3(1.0, 2.0, 3.0),
            normal: Vector3::unit_z(),
        };
        let member = LadderMember {
            kind: MemberKind::Rail,
            axis: Vector3::unit_y(),
            face_normal: Vector3::unit_z(),
        };
        for hand in [Handedness::Left, Handedness::Right] {
            for yaw in [0.0, 75.0, 180.0] {
                let turn = Quaternion::from_angle_y(Deg(yaw));
                let grip = ClimbGrip {
                    normal: turn.rotate_vector(grip.normal),
                    ..grip
                };
                let member = LadderMember {
                    face_normal: turn.rotate_vector(member.face_normal),
                    ..member
                };
                for roll in [-90.0, 90.0] {
                    let aligned = turn
                        * Quaternion::from_angle_x(Deg(180.0))
                        * Quaternion::from_angle_z(Deg(roll));
                    for tilt in [-35.0, 0.0, 35.0] {
                        let wrist = aligned * Quaternion::from_angle_z(Deg(tilt));
                        let pose = attached_pose(&rig, hand, grip, member, wrist).unwrap();
                        assert!(
                            pose.rotation.dot(aligned).abs() > 0.9999,
                            "{hand:?}, yaw {yaw}, roll {roll}, tilt {tilt}: align without flipping the wrist"
                        );
                        let palm = pose.point(rig.palm);
                        assert!((palm.y - grip.point.y).abs() < 1e-5);
                        assert!((palm - grip.point).magnitude() < 0.07);
                        assert!((palm - grip.point).dot(grip.normal) > 0.06);
                    }
                }
                // Even an away-facing wrist must not put the palm behind the
                // acquired face (the free-azimuth candidate hid it in the pole).
                for roll in [0.0, 90.0, 180.0, 270.0] {
                    let wrist = turn * Quaternion::from_angle_z(Deg(roll));
                    let pose = attached_pose(&rig, hand, grip, member, wrist).unwrap();
                    assert!((pose.point(rig.palm) - grip.point).dot(grip.normal) > 0.06);
                    let along = pose.rotation.rotate_vector(Vector3::unit_y());
                    assert!(along.dot(wrist.rotate_vector(Vector3::unit_y())) >= -1e-5);
                }
            }
        }
    }
    #[test]
    fn wrist_hint_is_latched_until_release_or_a_different_hold() {
        use cgmath::{Deg, Rotation3};
        let grip = ClimbGrip {
            kind: crate::physics::ClimbGripKind::Ladder,
            entity_id: None,
            point: vec3(1.0, 2.0, 3.0),
            normal: Vector3::unit_z(),
        };
        let member = LadderMember {
            kind: MemberKind::Rail,
            axis: Vector3::unit_y(),
            face_normal: Vector3::unit_z(),
        };
        let first = HeldMember::capture(None, grip, member, Quaternion::one());
        let twisted = Quaternion::from_angle_y(Deg(180.0));
        let held = HeldMember::capture(Some(first), grip, member, twisted);
        assert_eq!(held.wrist_rotation, first.wrist_rotation);
        assert_eq!(
            HeldMember::capture(None, grip, member, twisted).wrist_rotation,
            twisted
        );
        let next = ClimbGrip {
            point: grip.point + Vector3::unit_y(),
            ..grip
        };
        assert_eq!(
            HeldMember::capture(Some(held), next, member, twisted).wrist_rotation,
            twisted
        );
    }

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
                use cgmath::{Deg, Rotation3};
                let wrist = Quaternion::from_angle_x(Deg(180.0))
                    * Quaternion::from_angle_z(Deg(if hand == Handedness::Left {
                        -90.0
                    } else {
                        90.0
                    }));
                let pose = attached_pose(&rig, hand, grip, member, wrist).unwrap();
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
                    turn * wrist,
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
