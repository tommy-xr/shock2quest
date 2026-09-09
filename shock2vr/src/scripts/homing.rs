//! Annelid lock-on and fixed-cadence steering (Dark `shkhome.cpp`).
use cgmath::{EuclideanSpace, InnerSpace, Point3, Vector3, vec3};
use dark::properties::{PropHasRefs, PropHitPoints, PropHoming, PropPosition, PropTargetType};
use serde::{Deserialize, Serialize};
use shipyard::{EntityId, Get, IntoIter, IntoWithId, View, World};

use super::{Effect, Script, ScriptRestoreContext, ScriptState, ScriptStateError};
use crate::{
    physics::{InternalCollisionGroups, PhysicsWorld},
    time::Time,
};

const STATE_KEY: &str = "shock2vr.homing";

#[derive(Default, Serialize, Deserialize)]
pub struct Homing {
    initialized: bool,
    target: Option<u64>,
    remaining_seconds: f32,
}

impl Script for Homing {
    fn update(
        &mut self,
        entity: EntityId,
        world: &World,
        physics: &PhysicsWorld,
        time: &Time,
    ) -> Effect {
        let (homing, positions, health) = world
            .borrow::<(View<PropHoming>, View<PropPosition>, View<PropHitPoints>)>()
            .unwrap();
        let (Ok(config), Ok(position), Some(velocity)) = (
            homing.get(entity),
            positions.get(entity),
            physics.get_velocity(entity),
        ) else {
            return Effect::NoEffect;
        };
        if !self.initialized {
            self.initialized = true;
            // Use the actual launch aim in both presentations: in VR the hand
            // can aim independently of the head. The original scans player aim.
            self.target =
                acquire_target(world, physics, entity, position.position, velocity, config)
                    .map(EntityId::inner);
            self.remaining_seconds = config.update_interval.as_secs_f32().max(0.001);
            return Effect::NoEffect;
        }
        let Some(target) = self.target.and_then(EntityId::from_inner) else {
            return Effect::NoEffect;
        };
        let Ok(target_position) = positions.get(target) else {
            self.target = None;
            return Effect::NoEffect;
        };
        if !health.get(target).is_ok_and(|hp| hp.hit_points > 0) {
            self.target = None;
            return Effect::NoEffect;
        }
        self.remaining_seconds -= time.elapsed.as_secs_f32();
        if self.remaining_seconds > 0.0 {
            return Effect::NoEffect;
        }
        let interval = config.update_interval.as_secs_f32().max(0.001);
        let pulses = 1.0 + (-self.remaining_seconds / interval).floor();
        self.remaining_seconds += pulses * interval;
        Effect::SetLinearVelocity {
            entity_id: entity,
            velocity: homing_velocity(
                velocity,
                target_position.position - position.position,
                config.max_turn * pulses,
            ),
        }
    }

    fn script_state_key(&self) -> Option<&'static str> {
        Some(STATE_KEY)
    }
    fn save_state(&self) -> Result<ScriptState, ScriptStateError> {
        ScriptState::encode(1, self, STATE_KEY)
    }
    fn restore_state(
        &mut self,
        state: &ScriptState,
        context: &ScriptRestoreContext<'_>,
    ) -> Result<(), ScriptStateError> {
        let mut restored: Self = state.decode(1, STATE_KEY)?;
        restored.target = match restored.target {
            Some(target) => match context.remap_entity(target) {
                Ok(target) => Some(target.inner()),
                // A target deleted before the save no longer has a live link.
                Err(ScriptStateError::MissingEntityReference(_)) => None,
                Err(error) => return Err(error),
            },
            None => None,
        };
        *self = restored;
        Ok(())
    }
}

fn acquire_target(
    world: &World,
    physics: &PhysicsWorld,
    projectile: EntityId,
    origin: Vector3<f32>,
    forward: Vector3<f32>,
    config: &PropHoming,
) -> Option<EntityId> {
    if forward.magnitude2() < 1e-8 {
        return None;
    }
    let (types, positions, health, refs) = world
        .borrow::<(
            View<PropTargetType>,
            View<PropPosition>,
            View<PropHitPoints>,
            View<PropHasRefs>,
        )>()
        .unwrap();
    let (yaw, pitch) = direction_angles(forward);
    let mut best = None;
    let mut best_angle = f32::INFINITY;
    for (target, (kind, position, hp)) in (&types, &positions, &health).iter().with_id() {
        if target == projectile
            || kind.0 & config.target_types == 0
            || hp.hit_points <= 0
            || refs.get(target).is_ok_and(|r| !r.0)
        {
            continue;
        }
        let delta = position.position - origin;
        // Symmetric axis-distance prefilter; avoid the original signed-delta
        // typo that admits arbitrarily distant targets on negative axes.
        if delta.x.abs() >= config.distance
            || delta.y.abs() >= config.distance
            || delta.z.abs() >= config.distance
            || delta.magnitude2() < 1e-8
        {
            continue;
        }
        let (target_yaw, target_pitch) = direction_angles(delta);
        let dy = angle_delta(yaw, target_yaw).abs();
        let dp = angle_delta(pitch, target_pitch).abs();
        if dy >= config.heading_limit || dp >= config.heading_limit || dy + dp >= best_angle {
            continue;
        }
        // Dark PortalRaycast tests level geometry, not other target bodies.
        if physics
            .ray_cast2(
                Point3::from_vec(origin),
                delta.normalize(),
                delta.magnitude(),
                InternalCollisionGroups::WORLD,
                Some(projectile),
                true,
            )
            .is_some()
        {
            continue;
        }
        best = Some(target);
        best_angle = dy + dp;
    }
    best
}

fn direction_angles(direction: Vector3<f32>) -> (f32, f32) {
    (
        direction.z.atan2(direction.x),
        direction.y.atan2(direction.x.hypot(direction.z)),
    )
}

fn angle_delta(from: f32, to: f32) -> f32 {
    (to - from + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI
}

fn homing_velocity(
    velocity: Vector3<f32>,
    target_delta: Vector3<f32>,
    max_turn: f32,
) -> Vector3<f32> {
    if velocity.magnitude2() < 1e-8 || target_delta.magnitude2() < 1e-8 {
        return velocity;
    }
    let (yaw, pitch) = direction_angles(velocity);
    let (target_yaw, target_pitch) = direction_angles(target_delta);
    let max_turn = max_turn.max(0.0);
    let yaw = yaw + angle_delta(yaw, target_yaw).clamp(-max_turn, max_turn);
    let pitch = pitch + angle_delta(pitch, target_pitch).clamp(-max_turn, max_turn);
    vec3(
        yaw.cos() * pitch.cos(),
        pitch.sin(),
        yaw.sin() * pitch.cos(),
    ) * velocity.magnitude()
}

#[cfg(test)]
mod tests {
    use super::*;
    use cgmath::{One, Quaternion};
    use std::{collections::HashMap, time::Duration};

    fn config(mask: u32) -> PropHoming {
        PropHoming {
            target_types: mask,
            distance: 30.0,
            heading_limit: 30f32.to_radians(),
            max_turn: 11.25f32.to_radians(),
            update_interval: Duration::from_millis(200),
        }
    }

    fn target(
        world: &mut World,
        mask: u32,
        position: Vector3<f32>,
        hp: i32,
        visible: bool,
    ) -> EntityId {
        world.add_entity((
            PropTargetType(mask),
            PropHitPoints { hit_points: hp },
            PropHasRefs(visible),
            PropPosition {
                position,
                rotation: Quaternion::one(),
                cell: 0,
            },
        ))
    }

    #[test]
    fn target_masks_health_visibility_range_and_angular_priority() {
        let mut world = World::new();
        let shot = world.add_entity(());
        let human = target(&mut world, 1, vec3(10.0, 0.0, 2.0), 10, true);
        let annelid = target(&mut world, 2, vec3(10.0, 0.0, 1.0), 10, true);
        target(&mut world, 3, vec3(10.0, 0.0, 0.0), 0, true);
        target(&mut world, 3, vec3(10.0, 0.0, 0.0), 10, false);
        target(&mut world, 3, vec3(100.0, 0.0, 0.0), 10, true);
        target(&mut world, 3, vec3(-100.0, 0.0, 0.0), 10, true);
        target(&mut world, 3, vec3(1.0, 0.0, 10.0), 10, true);
        let physics = PhysicsWorld::new();
        assert_eq!(
            acquire_target(
                &world,
                &physics,
                shot,
                vec3(0.0, 0.0, 0.0),
                vec3(1.0, 0.0, 0.0),
                &config(1)
            ),
            Some(human)
        );
        assert_eq!(
            acquire_target(
                &world,
                &physics,
                shot,
                vec3(0.0, 0.0, 0.0),
                vec3(1.0, 0.0, 0.0),
                &config(2)
            ),
            Some(annelid)
        );
        let aligned = target(&mut world, 3, vec3(20.0, 0.0, 0.0), 10, true);
        assert_eq!(
            acquire_target(
                &world,
                &physics,
                shot,
                vec3(0.0, 0.0, 0.0),
                vec3(1.0, 0.0, 0.0),
                &config(1)
            ),
            Some(aligned)
        );
    }

    #[test]
    fn steering_caps_both_axes_preserves_speed_and_wraps_heading() {
        let velocity = vec3(10.0, 0.0, 0.0);
        let turned = homing_velocity(velocity, vec3(1.0, 1.0, 1.0), 11.25f32.to_radians());
        let (yaw, pitch) = direction_angles(turned);
        assert!((yaw.to_degrees() - 11.25).abs() < 0.001);
        assert!((pitch.to_degrees() - 11.25).abs() < 0.001);
        assert!((turned.magnitude() - 10.0).abs() < 0.001);
        let from = vec3(
            (-179f32).to_radians().cos(),
            0.0,
            (-179f32).to_radians().sin(),
        );
        let to = vec3(179f32.to_radians().cos(), 0.0, 179f32.to_radians().sin());
        assert!((homing_velocity(from, to, 3f32.to_radians()) - to).magnitude() < 0.001);
        assert_eq!(
            homing_velocity(velocity, vec3(0.0, 0.0, 0.0), 1.0),
            velocity
        );
    }

    #[test]
    fn save_remaps_lock_and_preserves_pulse_remainder() {
        let mut world = World::new();
        let old = world.add_entity(());
        let new = world.add_entity(());
        let script = Homing {
            initialized: true,
            target: Some(old.inner()),
            remaining_seconds: 0.125,
        };
        let state = script.save_state().unwrap();
        let map = HashMap::from([(old, new)]);
        let mut restored = Homing::default();
        restored
            .restore_state(&state, &ScriptRestoreContext::new(&map))
            .unwrap();
        assert!(restored.initialized);
        assert_eq!(restored.target, Some(new.inner()));
        assert_eq!(restored.remaining_seconds, 0.125);
        restored
            .restore_state(&state, &ScriptRestoreContext::new(&HashMap::new()))
            .unwrap();
        assert_eq!(restored.target, None);
    }
}
