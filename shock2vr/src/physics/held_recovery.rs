//! Bounded escape for a carried item stranded behind world geometry.
use super::*;

const SEPARATION: f32 = 0.5 / crate::METERS_PER_WORLD_UNIT;
const DELAY: f32 = 0.15;
const ARM_RADIUS: f32 = 0.035 / crate::METERS_PER_WORLD_UNIT;

#[derive(Clone, Copy)]
pub struct HeldRecoveryContext {
    pub arm_origin: Vector3<f32>,
    /// Model-local grip, in the same scaled coordinates as the fitted collider.
    pub grip_anchor: Vector3<f32>,
    pub elapsed_seconds: f32,
}

impl HeldRecoveryContext {
    pub fn from_input(
        input: &crate::input_context::InputContext,
        hand: usize,
        pawn: Vector3<f32>,
        rotation: Quaternion<f32>,
        grip_anchor: Vector3<f32>,
        elapsed_seconds: f32,
    ) -> Option<Self> {
        use cgmath::Rotation;
        let controller = [&input.left_hand, &input.right_hand][hand];
        let valid = crate::vr_support::GripPose {
            position: controller.position,
            rotation: controller.rotation,
        }
        .is_tracked()
            && crate::vr_support::GripPose {
                position: input.head.position,
                rotation: input.head.rotation,
            }
            .is_tracked()
            && input.pose_tracking.is_none_or(|p| p.head && p.hands[hand]);
        (valid && controller.squeeze_value >= 0.5).then_some(Self {
            // Conservative chest-to-grip corridor; the head's tracked lateral
            // offset follows roomscale movement, and head pitch does not tilt it.
            arm_origin: pawn + rotation.rotate_vector(input.head.position)
                - vec3(0.0, 0.25 / crate::METERS_PER_WORLD_UNIT, 0.0),
            grip_anchor,
            elapsed_seconds,
        })
    }
}

#[derive(Clone, Copy, Default)]
pub(super) struct RecoveryState {
    context: Option<HeldRecoveryContext>,
    blocked_seconds: f32,
}

#[derive(Clone, Copy)]
pub struct HeldRecovery {
    pub from: Vector3<f32>,
    pub to: Vector3<f32>,
}

impl PhysicsWorld {
    /// Set once before the real physics step. Missing/invalid tracking or a
    /// released grip supplies None and disarms any partially elapsed recovery.
    pub fn set_held_recovery_context(
        &mut self,
        entity: EntityId,
        context: Option<HeldRecoveryContext>,
    ) {
        let Some(handle) = self.entity_id_to_body.get(&entity) else {
            return;
        };
        let Some(drive) = self.held_item_drives.get_mut(handle) else {
            return;
        };
        drive.recovery.context = context.filter(|c| {
            [
                c.arm_origin.x,
                c.arm_origin.y,
                c.arm_origin.z,
                c.grip_anchor.x,
                c.grip_anchor.y,
                c.grip_anchor.z,
            ]
            .into_iter()
            .all(f32::is_finite)
                && c.elapsed_seconds.is_finite()
                && c.elapsed_seconds > 0.0
        });
        if drive.recovery.context.is_none() {
            drive.recovery.blocked_seconds = 0.0;
        }
    }

    pub fn take_held_recoveries(&mut self) -> Vec<HeldRecovery> {
        std::mem::take(&mut self.held_recoveries)
    }

    pub(super) fn recover_held_item(
        &mut self,
        weapon: RigidBodyHandle,
        current: Isometry<Real>,
        desired: Isometry<Real>,
        controller: Isometry<Real>,
        blocked: bool,
    ) -> bool {
        let drive = self.held_item_drives.get_mut(&weapon).unwrap();
        // Consume the live context: stale tracking cannot keep a timer armed
        // if a caller forgets to publish the next frame.
        let Some(context) = drive.recovery.context.take() else {
            drive.recovery.blocked_seconds = 0.0;
            return false;
        };
        let anchor = Point::from(vec_to_nvec(context.grip_anchor));
        let from = current * anchor;
        let to = controller * anchor;
        if !blocked || !drive.seated || (to - from).norm() < SEPARATION {
            drive.recovery.blocked_seconds = 0.0;
            return false;
        }
        // A single hitch must not spend the entire dwell period.
        drive.recovery.blocked_seconds += context.elapsed_seconds.min(0.05);
        if drive.recovery.blocked_seconds + 1.0e-6 < DELAY {
            return false;
        }
        let target = drive.target;
        if !self.held_recovery_pose_is_clear(weapon, &controller, context)
            || !self.held_recovery_pose_is_clear(weapon, &desired, context)
        {
            return false;
        }

        // Both endpoints start the normal Rapier step at their destination.
        // It therefore derives zero teleport velocity, not a damaging swing.
        self.rigid_body_set[weapon].set_position(desired, true);
        self.rigid_body_set[target].set_position(controller, true);
        let drive = self.held_item_drives.get_mut(&weapon).unwrap();
        drive.stopped_on = None;
        drive.recovered_this_step = true;
        drive.recovery.blocked_seconds = 0.0;
        self.held_recoveries.push(HeldRecovery {
            from: nvec_to_cgmath(from.coords),
            to: nvec_to_cgmath((desired * anchor).coords),
        });
        true
    }

    fn held_recovery_pose_is_clear(
        &self,
        weapon: RigidBodyHandle,
        pose: &Isometry<Real>,
        context: HeldRecoveryContext,
    ) -> bool {
        let body = &self.rigid_body_set[weapon];
        // Include loose props and creatures at the destination too. Ignore
        // the owner, sensors, and other held items; none should block an arm.
        let obstacle = |_: ColliderHandle, c: &Collider| {
            let groups = c.collision_groups().memberships.bits();
            groups
                & (InternalCollisionGroups::WORLD.bits
                    | InternalCollisionGroups::ENTITY.bits
                    | InternalCollisionGroups::ACTOR.bits
                    | InternalCollisionGroups::HITBOX.bits)
                != 0
                && c.parent()
                    .is_none_or(|h| !self.held_item_drives.contains_key(&h))
        };
        let queries = self.broad_phase.as_query_pipeline(
            self.narrow_phase.query_dispatcher(),
            &self.rigid_body_set,
            &self.collider_set,
            QueryFilter::new()
                .exclude_rigid_body(weapon)
                .exclude_sensors()
                .predicate(&obstacle),
        );
        let arm = Ball::new(ARM_RADIUS);
        let from = vec_to_nvec(context.arm_origin);
        let to = (pose * Point::from(vec_to_nvec(context.grip_anchor))).coords;
        if shape_intersects(&queries, from, &arm)
            || shape_intersects(&queries, to, &arm)
            || !shape_sweep_is_clear(&queries, from, to, &arm)
        {
            return false;
        }
        // Validate every collider at its full final orientation and local
        // offset. A clear grip point alone doesn't clear a long barrel.
        !body.colliders().is_empty()
            && body.colliders().iter().all(|handle| {
                let c = &self.collider_set[*handle];
                let at = *pose * c.position_wrt_parent().copied().unwrap_or_default();
                queries.intersect_shape(at, c.shape()).next().is_none()
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::physics::held_item_drive::{spawn_held_wrench, world_with_floor};
    use cgmath::{Deg, Rotation3};

    fn fixture() -> (PhysicsWorld, PlayerHandle, EntityId, RigidBodyHandle) {
        let (mut world, mut player) = world_with_floor();
        world.add_collider(
            EntityId::from_inner(3).unwrap(),
            ColliderBuilder::cuboid(0.05, 2.0, 2.0)
                .translation(vector![0.0, 1.2, 0.0])
                .build(),
        );
        let (weapon, handle) = spawn_held_wrench(&mut world, vec3(-1.0, 1.2, 0.0));
        world.set_position_rotation2(
            weapon,
            vec3(-1.0, 1.2, 0.0),
            Quaternion::new(1.0, 0.0, 0.0, 0.0),
        );
        world.update(vec3(0.0, 0.0, 0.0), &mut player);
        world.set_position_rotation2(
            weapon,
            vec3(1.0, 1.2, 0.0),
            Quaternion::new(1.0, 0.0, 0.0, 0.0),
        );
        (world, player, weapon, handle)
    }

    fn context(x: f32) -> HeldRecoveryContext {
        HeldRecoveryContext {
            arm_origin: vec3(x, 1.2, 0.5),
            grip_anchor: vec3(0.0, 0.0, 0.0),
            elapsed_seconds: 1.0 / 60.0,
        }
    }

    fn advance(
        world: &mut PhysicsWorld,
        player: &mut PlayerHandle,
        weapon: EntityId,
        context: Option<HeldRecoveryContext>,
        frames: usize,
    ) -> Vec<CollisionEvent> {
        let mut events = Vec::new();
        for _ in 0..frames {
            world.set_held_recovery_context(weapon, context);
            events.extend(world.update(vec3(0.0, 0.0, 0.0), player).1);
        }
        events
    }

    #[test]
    fn held_recovery_waits_then_returns_without_a_swing_or_contact() {
        let (mut world, mut player, weapon, handle) = fixture();
        advance(&mut world, &mut player, weapon, Some(context(1.0)), 8);
        assert!(world.get_position(handle).unwrap().x < 0.0);
        let events = advance(&mut world, &mut player, weapon, Some(context(1.0)), 1);
        let position = world.get_position(handle).unwrap();
        assert!((position - vec3(1.0, 1.2, 0.0)).magnitude() < 0.001);
        assert!(
            events.is_empty(),
            "the teleport must not report a swept blow: {events:?}"
        );
        assert!(world.get_velocity(weapon).unwrap().magnitude() < 0.001);
        assert!(
            (world
                .held_melee_target_velocity_at_point(weapon, position)
                .unwrap()
                - world.player_velocity())
            .magnitude()
                < 0.001
        );
        assert_eq!(world.take_held_recoveries().len(), 1);
        assert_eq!(world.entity_id_to_body[&weapon], handle);
        advance(&mut world, &mut player, weapon, Some(context(1.0)), 20);
        assert!(world.take_held_recoveries().is_empty());
    }

    #[test]
    fn held_recovery_requires_clear_arm_corridor_not_a_clear_old_weapon_path() {
        let (mut world, mut player, weapon, handle) = fixture();
        advance(&mut world, &mut player, weapon, Some(context(-1.0)), 30);
        assert!(world.get_position(handle).unwrap().x < 0.0);
        assert!(world.take_held_recoveries().is_empty());
        advance(&mut world, &mut player, weapon, Some(context(1.0)), 1);
        assert!(world.get_position(handle).unwrap().x > 0.9);
    }

    #[test]
    fn held_recovery_refuses_an_overlapping_destination() {
        let (mut world, mut player, weapon, handle) = fixture();
        world.add_collider(
            EntityId::from_inner(4).unwrap(),
            ColliderBuilder::cuboid(0.1, 0.1, 0.1)
                .translation(vector![1.0, 0.8, 0.0])
                .build(),
        );
        advance(&mut world, &mut player, weapon, Some(context(1.0)), 30);
        assert!(world.get_position(handle).unwrap().x < 0.0);
        assert!(world.take_held_recoveries().is_empty());
    }

    #[test]
    fn held_recovery_checks_rotated_offset_colliders_not_only_the_grip() {
        let (mut world, mut player, weapon, handle) = fixture();
        world.add_collider(
            EntityId::from_inner(4).unwrap(),
            ColliderBuilder::cuboid(0.08, 0.08, 0.08)
                .translation(vector![1.5, 1.2, 0.0])
                .build(),
        );
        world.update(vec3(0.0, 0.0, 0.0), &mut player);
        let straight = Isometry::translation(1.0, 1.2, 0.0);
        let rotated = Isometry::from_parts(
            Translation::new(1.0, 1.2, 0.0),
            quat_to_nquat(Quaternion::from_angle_z(Deg(90.0))),
        );
        assert!(world.held_recovery_pose_is_clear(handle, &straight, context(1.0)));
        assert!(!world.held_recovery_pose_is_clear(handle, &rotated, context(1.0)));
        assert!(world.is_held_item(weapon));
    }

    #[test]
    fn held_recovery_tracking_loss_disarms_the_dwell() {
        let (mut world, mut player, weapon, handle) = fixture();
        advance(&mut world, &mut player, weapon, Some(context(1.0)), 5);
        advance(&mut world, &mut player, weapon, None, 60);
        advance(&mut world, &mut player, weapon, Some(context(1.0)), 8);
        assert!(world.get_position(handle).unwrap().x < 0.0);
        advance(&mut world, &mut player, weapon, Some(context(1.0)), 1);
        assert!(world.get_position(handle).unwrap().x > 0.9);
    }

    #[test]
    fn held_recovery_needs_live_tracking_and_a_held_grip() {
        let mut input = crate::input_context::InputContext::default();
        input.right_hand.squeeze_value = 1.0;
        let read = |input: &crate::input_context::InputContext| {
            HeldRecoveryContext::from_input(
                input,
                1,
                vec3(0.0, 0.0, 0.0),
                Quaternion::new(1.0, 0.0, 0.0, 0.0),
                vec3(0.0, 0.0, 0.0),
                1.0 / 60.0,
            )
        };
        assert!(read(&input).is_some());
        input.pose_tracking = Some(crate::input_context::PoseTracking {
            head: true,
            hands: [true, false],
        });
        assert!(read(&input).is_none());
        input.pose_tracking = None;
        input.right_hand.squeeze_value = 0.0;
        assert!(read(&input).is_none());
    }
    #[test]
    fn held_recovery_while_walking_has_no_relative_swing_velocity() {
        let (mut world, mut player, weapon, handle) = fixture();
        advance(&mut world, &mut player, weapon, Some(context(1.0)), 7);
        world.set_held_recovery_context(weapon, Some(context(1.0)));
        world.update(vec3(0.2, 0.0, 0.0), &mut player);
        advance(&mut world, &mut player, weapon, Some(context(1.0)), 1);
        assert_eq!(world.take_held_recoveries().len(), 1);
        assert!(world.player_velocity().x > 1.0);
        let velocity = world
            .held_melee_target_velocity_at_point(weapon, world.get_position(handle).unwrap())
            .unwrap();
        assert!((velocity - world.player_velocity()).magnitude() < 0.001);
    }

    #[test]
    fn held_recovery_dwell_uses_elapsed_time_and_a_hitch_cannot_skip_it() {
        for hz in [60.0_f32, 90.0, 120.0] {
            let (mut world, mut player, weapon, handle) = fixture();
            let mut input = context(1.0);
            input.elapsed_seconds = 1.0 / hz;
            let frames = (DELAY * hz).ceil() as usize;
            advance(&mut world, &mut player, weapon, Some(input), frames - 1);
            assert!(world.get_position(handle).unwrap().x < 0.0);
            advance(&mut world, &mut player, weapon, Some(input), 1);
            assert_eq!(world.take_held_recoveries().len(), 1);
        }
        let (mut world, mut player, weapon, handle) = fixture();
        let mut input = context(1.0);
        input.elapsed_seconds = 1.0;
        advance(&mut world, &mut player, weapon, Some(input), 1);
        assert!(world.get_position(handle).unwrap().x < 0.0);
    }
}
