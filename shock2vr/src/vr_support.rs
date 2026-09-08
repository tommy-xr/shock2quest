//! Rigid two-anchor posing. Geometry, scale and ownership stay with the primary hand.
use cgmath::{Deg, Euler, InnerSpace, Matrix4, Point3, Quaternion, Rotation, Transform, Vector3};
use serde::{Deserialize, Serialize};

/// Fixed authored support sockets currently enabled in gameplay.
/// Keep the Explorer eligibility notice and runtime policy together.
pub fn supports_model(model: &str) -> bool {
    matches!(model, "wrench_h" | "atek_h" | "sg_h")
}

#[derive(Clone, Copy, Debug, Serialize)]
pub struct GripPose {
    pub position: Vector3<f32>,
    pub rotation: Quaternion<f32>,
}

impl GripPose {
    pub fn point(self, local: Vector3<f32>) -> Vector3<f32> {
        self.position + self.rotation.rotate_vector(local)
    }

    pub fn is_tracked(self) -> bool {
        [
            self.position.x,
            self.position.y,
            self.position.z,
            self.rotation.s,
            self.rotation.v.x,
            self.rotation.v.y,
            self.rotation.v.z,
        ]
        .into_iter()
        .all(f32::is_finite)
            && crate::util::tracked_rotation(self.rotation).is_some()
    }
}

/// Anchor in the normalized weapon mesh used by prepared grips (before item scale).
/// Stored in the right-primary model frame; the renderer supplies the opposite frame.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SupportProfile {
    pub palm_anchor: [f32; 3],
    /// Support wrist rotation relative to the primary wrist; authored right-primary.
    #[serde(default)]
    pub rotation_degrees: [f32; 3],
    pub curls: [f32; 5],
    pub grab_radius: f32,
    pub release_distance: f32,
    pub max_swing_degrees: f32,
}

impl SupportProfile {
    pub fn is_valid(&self) -> bool {
        self.palm_anchor.iter().all(|v| v.is_finite())
            && self
                .rotation_degrees
                .iter()
                .all(|v| v.is_finite() && v.abs() <= 360.0)
            && self
                .curls
                .iter()
                .all(|v| v.is_finite() && (0.0..=1.0).contains(v))
            && self.grab_radius.is_finite()
            && (0.01..=0.15).contains(&self.grab_radius)
            && self.release_distance.is_finite()
            && (self.grab_radius..=0.3).contains(&self.release_distance)
            && (20.0..=120.0).contains(&self.max_swing_degrees)
    }

    /// Shared runtime/editor support glove placement from the same palm anchor.
    pub fn glove_pose(
        &self,
        primary: crate::Handedness,
        model: GripPose,
        grip: &crate::vr_grip::ResolvedGrip,
        support_rig: &crate::vr_grip::GripKinematics,
        scaled_anchor: Vector3<f32>,
    ) -> GripPose {
        let [x, y, z] = self.rotation_degrees;
        let q = Quaternion::from(Euler::new(Deg(x), Deg(y), Deg(z)));
        // Glove rotation mirrors hand X independently of the item's model axes.
        let q = if primary == crate::Handedness::Left {
            Quaternion::new(q.s, q.v.x, -q.v.y, -q.v.z)
        } else {
            q
        };
        let rotation = model.rotation * grip.rotation.conjugate() * q;
        GripPose {
            position: model.point(scaled_anchor) - rotation.rotate_vector(support_rig.palm),
            rotation,
        }
    }

    pub fn allows_swing(&self, from: Vector3<f32>, to: Vector3<f32>) -> bool {
        from.magnitude2() > 1e-8
            && to.magnitude2() > 1e-8
            && from.normalize().dot(to.normalize()) >= self.max_swing_degrees.to_radians().cos()
    }

    /// Use the same reflection as the rendered item: gun Z and posed melee X
    /// are different model frames even though both gloves mirror hand X.
    pub fn anchor_in_frame(
        &self,
        primary: crate::Handedness,
        model_mirror: Matrix4<f32>,
    ) -> Vector3<f32> {
        let point = Point3::from(self.palm_anchor);
        let point = if primary == crate::Handedness::Left {
            model_mirror.transform_point(point)
        } else {
            point
        };
        point.to_homogeneous().truncate()
    }
}

/// Keep the primary anchor exact, align the anchor vector, and inherit twist
/// from the primary orientation. Coincident hands keep the previous rotation.
/// Anchors are already scaled: there is no scale solve or hand-distance stretch.
pub fn solve_two_hand_pose(
    primary_palm: Vector3<f32>,
    support_palm: Vector3<f32>,
    primary_rotation: Quaternion<f32>,
    primary_anchor: Vector3<f32>,
    support_anchor: Vector3<f32>,
    previous_rotation: Quaternion<f32>,
) -> GripPose {
    let source = primary_rotation.rotate_vector(support_anchor - primary_anchor);
    let target = support_palm - primary_palm;
    let rotation = if source.magnitude2() < 1e-8 || target.magnitude2() < 1e-8 {
        previous_rotation
    } else {
        let from = source.normalize();
        // The antiparallel case needs a deterministic primary-hand twist axis.
        let mut axis = primary_rotation.rotate_vector(Vector3::unit_x());
        if axis.dot(from).abs() > 0.9 {
            axis = primary_rotation.rotate_vector(Vector3::unit_z());
        }
        let axis = (axis - from * axis.dot(from)).normalize();
        (Quaternion::from_arc(from, target.normalize(), Some(axis)) * primary_rotation).normalize()
    };
    GripPose {
        position: primary_palm - rotation.rotate_vector(primary_anchor),
        rotation,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cgmath::{Deg, One, Rotation3, vec3};

    #[test]
    fn rotated_support_pose_preserves_the_mirrored_palm_in_runtime_and_editor() {
        let profile = SupportProfile {
            palm_anchor: [-0.09, 0.354, 0.02],
            rotation_degrees: [20.0, 15.0, -30.0],
            curls: [0.4; 5],
            grab_radius: 0.07,
            release_distance: 0.12,
            max_swing_degrees: 75.0,
        };
        let grip = crate::vr_grip::ResolvedGrip {
            item_scale: 0.7,
            pose_family: "cylindrical".into(),
            offset: vec3(-0.05, -0.1, 0.02),
            rotation: Quaternion::from_angle_x(Deg(20.0)),
            curls: [0.4; 5],
            contacts: [None; 5],
            anchor: [0.0; 3],
            score: 0.0,
        };
        let model = GripPose {
            position: vec3(2.0, 3.0, 4.0),
            rotation: Quaternion::from_angle_y(Deg(35.0)),
        };
        for primary in [crate::Handedness::Left, crate::Handedness::Right] {
            let rig = crate::vr_grip::GripKinematics {
                fingers: std::array::from_fn(|_| Vec::new()),
                palm: vec3(0.01, -0.001, -0.079),
                normal: Vector3::unit_x(),
            };
            let pose = profile.glove_pose(
                primary,
                model,
                &grip,
                &rig,
                profile.anchor_in_frame(primary, crate::Handedness::Left.mirror())
                    * grip.item_scale,
            );
            let expected =
                model.point(primary.mirror_point(vec3(-0.09, 0.354, 0.02)) * grip.item_scale);
            assert!((pose.point(rig.palm) - expected).magnitude() < 1e-5);
            assert!((pose.rotation.magnitude() - 1.0).abs() < 1e-5);
            let sign = if primary == crate::Handedness::Left {
                -1.0
            } else {
                1.0
            };
            let editor_rotation = Quaternion::from(cgmath::Euler::new(
                Deg(20.0),
                Deg(15.0 * sign),
                Deg(-30.0 * sign),
            ));
            let expected_rotation = model.rotation * grip.rotation.conjugate() * editor_rotation;
            for axis in [Vector3::unit_x(), Vector3::unit_y(), Vector3::unit_z()] {
                assert!((pose.rotation * axis - expected_rotation * axis).magnitude() < 1e-5);
            }
            assert!(pose.is_tracked());
        }
    }

    #[test]
    fn primary_twist_survives_and_invalid_profiles_do_not_opt_in() {
        let a = vec3(0.0, 0.0, 0.0);
        let b = vec3(0.0, 0.2, 0.0);
        let q = Quaternion::from_angle_y(Deg(73.0));
        let pose = solve_two_hand_pose(a, b, q, a, b, Quaternion::one());
        assert!(
            (pose.rotation.rotate_vector(Vector3::unit_x()) - q.rotate_vector(Vector3::unit_x()))
                .magnitude()
                < 1e-5
        );
        let mut profile = SupportProfile {
            palm_anchor: [0.02, 0.2, 0.0],
            rotation_degrees: [0.0; 3],
            curls: [0.5; 5],
            grab_radius: 0.07,
            release_distance: 0.12,
            max_swing_degrees: 75.0,
        };
        assert!(profile.is_valid());
        assert!(profile.allows_swing(Vector3::unit_y(), Vector3::unit_y()));
        assert!(!profile.allows_swing(Vector3::unit_y(), Vector3::unit_x()));
        assert_eq!(
            profile
                .anchor_in_frame(crate::Handedness::Left, crate::Handedness::Left.mirror())
                .x,
            -profile
                .anchor_in_frame(crate::Handedness::Right, crate::Handedness::Left.mirror())
                .x
        );
        profile.release_distance = 0.02;
        assert!(!profile.is_valid());
        profile.release_distance = 0.12;
        profile.curls[0] = f32::NAN;
        assert!(!profile.is_valid());
    }

    #[test]
    fn off_axis_anchors_preserve_primary_and_align_without_stretching() {
        let a = vec3(0.04, 0.1, -0.06);
        let b = vec3(-0.03, 0.35, -0.02);
        let p = vec3(2.0, 3.0, 4.0);
        let target = vec3(0.5, 0.3, -0.2);
        let q = Quaternion::from_angle_z(Deg(27.0));
        let pose = solve_two_hand_pose(p, p + target, q, a, b, q);
        assert!((pose.point(a) - p).magnitude() < 1e-5);
        assert!(((pose.point(b) - p).normalize() - target.normalize()).magnitude() < 1e-5);
        assert!(((pose.point(b) - p).magnitude() - (b - a).magnitude()).abs() < 1e-5);
        assert!((pose.rotation.magnitude() - 1.0).abs() < 1e-5);
    }

    #[test]
    fn antiparallel_and_coincident_hands_are_finite_and_stable() {
        let a = Vector3::new(0.03, 0.02, -0.01);
        let b = a + Vector3::unit_y() * 0.2;
        let q = Quaternion::from_angle_y(Deg(41.0));
        let p = Vector3::new(1.0, 2.0, 3.0);
        let pose = solve_two_hand_pose(p, p - Vector3::unit_y(), q, a, b, q);
        assert!(pose.is_tracked());
        assert!((pose.point(a) - p).magnitude() < 1e-5);
        assert!(((pose.point(b) - p).normalize() + Vector3::unit_y()).magnitude() < 1e-5);
        let still = solve_two_hand_pose(p, p, q, a, b, pose.rotation);
        assert_eq!(still.rotation, pose.rotation);
        assert!((still.point(a) - p).magnitude() < 1e-5);
        assert!(
            !GripPose {
                position: p,
                rotation: Quaternion::new(0.0, 0.0, 0.0, 0.0)
            }
            .is_tracked()
        );
        assert!(
            GripPose {
                position: p,
                rotation: Quaternion::one()
            }
            .is_tracked()
        );
    }
}
