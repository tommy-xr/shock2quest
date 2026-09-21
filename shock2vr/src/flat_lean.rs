//! Flat-only eye displacement. The pawn/collider stays at the feet; rendering,
//! interaction and weapon placement all consume the resulting pawn-space pose.
use crate::{death_camera::EyePose, physics::PhysicsWorld};
use cgmath::{InnerSpace, Quaternion, Rad, Rotation3, Vector3, vec3};
use shipyard::EntityId;

const ROLL: f32 = 7.0 * std::f32::consts::PI / 180.0;
/// Reach full lean in 150 ms, independent of frame rate.
const SECONDS: f32 = 0.15;

#[derive(Default)]
pub(crate) struct FlatLean {
    amount: f32,
}

impl FlatLean {
    pub fn update(
        &mut self,
        requested: f32,
        dt: f32,
        neutral: EyePose,
        pawn_position: Vector3<f32>,
        pawn_rotation: Quaternion<f32>,
        physics: &PhysicsWorld,
        ignore: &[Option<EntityId>],
    ) -> EyePose {
        let max_distance =
            crate::dev_params::get(crate::dev_params::FLAT_LEAN_DISTANCE) / dark::SCALE_FACTOR;
        if max_distance == 0.0 {
            self.amount = 0.0;
            return neutral;
        }
        let target = if requested.is_finite() {
            requested.clamp(-1.0, 1.0)
        } else {
            0.0
        };
        let step = dt.max(0.0) / SECONDS;
        self.amount += (target - self.amount).clamp(-step, step);
        // Project the gaze's right axis onto the floor: pitch cannot make a
        // sideways lean raise the eye into the ceiling.
        let right = neutral.rotation * Vector3::unit_x();
        let horizontal = vec3(right.x, 0.0, right.z);
        let right = if horizontal.magnitude2() > 1e-8 {
            horizontal.normalize()
        } else {
            // Synthetic debug input can roll the right axis straight up.
            Vector3::unit_x()
        };
        let offset = right * (self.amount * max_distance);
        let distance = offset.magnitude();
        if distance > 1e-6 {
            let origin = pawn_position + pawn_rotation * neutral.position;
            let allowed = physics.lean_distance(
                cgmath::Point3::new(origin.x, origin.y, origin.z),
                pawn_rotation * (offset / distance),
                distance,
                &|entity| !ignore.contains(&Some(entity)),
            );
            // Clamp the state too: holding against a wall must not accumulate
            // an invisible lean that jumps outward on clearing the corner.
            self.amount *= (allowed / distance).clamp(0.0, 1.0);
        }
        EyePose {
            position: neutral.position + right * (self.amount * max_distance),
            rotation: neutral.rotation * Quaternion::from_angle_z(Rad(-self.amount * ROLL)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cgmath::{Deg, One};
    use rapier3d::prelude::*;

    const DISTANCE: f32 = 2.0 / dark::SCALE_FACTOR;

    fn pose(lean: &mut FlatLean, axis: f32, dt: f32, physics: &PhysicsWorld) -> EyePose {
        lean.update(
            axis,
            dt,
            EyePose::flat(1.04, Quaternion::one()),
            vec3(0.0, 0.0, 0.0),
            Quaternion::one(),
            physics,
            &[],
        )
    }

    #[test]
    fn held_lean_is_symmetric_returns_and_is_frame_rate_independent() {
        let physics = PhysicsWorld::new();
        let mut lean = FlatLean::default();
        let right = pose(&mut lean, 1.0, SECONDS, &physics);
        assert!((right.position.x - DISTANCE).abs() < 1e-6);
        assert!(right.rotation.v.z < 0.0);
        let mut fine = FlatLean::default();
        for _ in 0..9 {
            pose(&mut fine, 1.0, 1.0 / 60.0, &physics);
        }
        assert!((fine.amount - lean.amount).abs() < 1e-6);
        let neutral = pose(&mut lean, 0.0, SECONDS, &physics);
        assert_eq!(neutral, EyePose::flat(1.04, Quaternion::one()));
        let left = pose(&mut lean, -1.0, SECONDS, &physics);
        assert!((left.position.x + right.position.x).abs() < 1e-6);
        assert!((left.rotation.v.z + right.rotation.v.z).abs() < 1e-6);
    }

    #[test]
    fn lean_follows_yaw_without_changing_crouched_height_or_aim_direction() {
        let rotation = Quaternion::from_angle_y(Deg(90.0)) * Quaternion::from_angle_x(Deg(70.0));
        let neutral = EyePose::flat(0.48, rotation);
        let eye = FlatLean::default().update(
            1.0,
            SECONDS,
            neutral,
            vec3(0.0, 0.0, 0.0),
            Quaternion::one(),
            &PhysicsWorld::new(),
            &[],
        );
        assert!((eye.position.z + DISTANCE).abs() < 1e-6);
        assert_eq!(eye.position.y, neutral.position.y);
        assert!(
            (eye.rotation * -Vector3::unit_z() - rotation * -Vector3::unit_z()).magnitude() < 1e-6
        );
    }

    #[test]
    fn wall_clamps_eye_and_roll_and_does_not_accumulate_hidden_lean() {
        let mut physics = PhysicsWorld::new();
        physics.add_collider(
            EntityId::from_inner(1).unwrap(),
            ColliderBuilder::cuboid(0.05, 2.0, 2.0)
                .translation(vector![0.45, 0.0, 0.0])
                .build(),
        );
        let mut player =
            physics.create_player(vec3(100.0, 100.0, 100.0), EntityId::from_inner(2).unwrap());
        physics.update(vec3(0.0, 0.0, 0.0), &mut player);
        let mut lean = FlatLean::default();
        let eye = pose(&mut lean, 1.0, SECONDS, &physics);
        assert!(
            eye.position.x > 0.1 && eye.position.x < 0.2,
            "near plane clearance: {eye:?}"
        );
        for _ in 0..60 {
            pose(&mut lean, 1.0, 1.0 / 60.0, &physics);
        }
        let clear = pose(&mut lean, 1.0, 1.0 / 60.0, &PhysicsWorld::new());
        assert!(
            clear.position.x < 0.3,
            "must ease outward after clearing a wall"
        );
        let left = pose(&mut lean, -1.0, 2.0 * SECONDS, &physics);
        assert!((left.position.x + DISTANCE).abs() < 1e-6);
    }
    #[test]
    fn initial_overlap_cannot_push_the_eye_through_a_thin_wall() {
        let mut physics = PhysicsWorld::new();
        physics.add_collider(
            EntityId::from_inner(1).unwrap(),
            ColliderBuilder::cuboid(0.02, 2.0, 2.0)
                .translation(vector![0.1, 0.0, 0.0])
                .build(),
        );
        let mut player =
            physics.create_player(vec3(100.0, 100.0, 100.0), EntityId::from_inner(2).unwrap());
        physics.update(vec3(0.0, 0.0, 0.0), &mut player);
        let eye = pose(&mut FlatLean::default(), 1.0, SECONDS, &physics);
        assert_eq!(eye, EyePose::flat(1.04, Quaternion::one()));
    }
    #[test]
    fn non_solid_selection_boxes_do_not_block_lean() {
        let mut physics = PhysicsWorld::new();
        physics.add_kinematic(
            EntityId::from_inner(1).unwrap(),
            vec3(0.3, 0.0, 0.0),
            Quaternion::one(),
            vec3(0.0, 0.0, 0.0),
            vec3(0.1, 2.0, 2.0),
            crate::physics::CollisionGroup::selectable().non_solid_to_player(),
            false,
        );
        let mut player =
            physics.create_player(vec3(100.0, 100.0, 100.0), EntityId::from_inner(2).unwrap());
        physics.update(vec3(0.0, 0.0, 0.0), &mut player);
        let eye = pose(&mut FlatLean::default(), 1.0, SECONDS, &physics);
        assert!((eye.position.x - DISTANCE).abs() < 1e-6);
    }
}
