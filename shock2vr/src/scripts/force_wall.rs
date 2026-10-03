//! Retail ForceWallStructure: a damageable authored wall with a four-minute life.
//! HP curve: retail manual Psi37; lifetime: Telliamed's ForceWallStructure reference.
use super::{Effect, Script, ScriptRestoreContext, ScriptState, ScriptStateError};
use crate::{physics::PhysicsWorld, time::Time};
use serde::{Deserialize, Serialize};
use shipyard::{EntityId, World};

pub(crate) const POWER: i32 = -3162;
pub(crate) const STRUCTURE: i32 = -3450;
const STATE_KEY: &str = "shock2vr.force_wall";
const LIFETIME: f32 = 240.0;

pub(crate) fn hit_points(data: [f32; 4], psi: i32) -> i32 {
    (data[0] + data[1] * (psi as f32 - data[2]).max(0.0))
        .max(1.0)
        .round() as i32
}

/// Keep local Y upright even for exactly antiparallel headings.
pub(crate) fn upright_rotation(forward: cgmath::Vector3<f32>) -> cgmath::Quaternion<f32> {
    use cgmath::Rotation3;
    cgmath::Quaternion::from_angle_y(cgmath::Rad((-forward.z).atan2(forward.x)))
}

/// Leave space past both the body and the held amp, so a cast cannot create
/// a wall behind the muzzle that the next projectile immediately bypasses.
pub(crate) fn clearance_distance(muzzle_ahead: f32, half_thickness: f32) -> f32 {
    crate::physics::PLAYER_STANDING_RADIUS_WORLD.max(muzzle_ahead) + half_thickness + 0.5
}

#[derive(Default, Serialize, Deserialize)]
pub struct ForceWallStructure {
    age: f32,
}
impl Script for ForceWallStructure {
    fn update(
        &mut self,
        entity_id: EntityId,
        _world: &World,
        _physics: &PhysicsWorld,
        time: &Time,
    ) -> Effect {
        self.age += time.elapsed.as_secs_f32();
        if self.age >= LIFETIME {
            Effect::DestroyEntity { entity_id }
        } else {
            Effect::NoEffect
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
        _context: &ScriptRestoreContext<'_>,
    ) -> Result<(), ScriptStateError> {
        *self = state.decode(1, STATE_KEY)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn placement_stays_upright_and_in_front_of_the_casting_hand() {
        use cgmath::{InnerSpace, vec3};
        for forward in [
            vec3(-1.0, 0.0, 0.0),
            vec3(1.0, 0.0, 0.0),
            vec3(0.0, 0.0, 1.0),
            vec3(0.0, 0.0, -1.0),
        ] {
            let rotation = upright_rotation(forward);
            assert!((rotation * vec3(0.0, 1.0, 0.0) - vec3(0.0, 1.0, 0.0)).magnitude() < 0.0001);
            assert!((rotation * vec3(1.0, 0.0, 0.0) - forward).magnitude() < 0.0001);
        }
        assert!((clearance_distance(1.5, 0.2) - 2.2).abs() < 0.0001);
        assert!((clearance_distance(-0.5, 0.2) - 1.18).abs() < 0.0001);
    }

    #[test]
    fn full_barrier_volume_rejects_side_obstructions_but_ignores_sensors() {
        use crate::physics::CollisionGroup;
        use cgmath::{Quaternion, vec3};
        let mut physics = PhysicsWorld::new();
        let id = EntityId::from_inner(7).unwrap();
        let identity = Quaternion::new(1.0, 0.0, 0.0, 0.0);
        let center = vec3(0.0, 2.02, 0.0);
        let size = vec3(0.4, 4.0, 4.8);
        let side = vec3(0.0, 2.02, 2.3); // Off the centre ray, inside the wall's edge.
        physics.add_kinematic(
            id,
            side,
            identity,
            vec3(0.0, 0.0, 0.0),
            vec3(0.2, 1.0, 0.2),
            CollisionGroup::entity(),
            false,
        );
        assert!(!physics.placement_box_is_clear(center, identity, size, &[]));
        assert!(physics.placement_box_is_clear(center, identity, size, &[id]));
        physics.remove(id);
        assert!(physics.placement_box_is_clear(center, identity, size, &[]));
        physics.add_kinematic(
            EntityId::from_inner(8).unwrap(),
            side,
            identity,
            vec3(0.0, 0.0, 0.0),
            vec3(0.2, 1.0, 0.2),
            CollisionGroup::entity(),
            true,
        );
        assert!(physics.placement_box_is_clear(center, identity, size, &[]));
    }

    #[test]
    fn barrier_health_uses_all_three_authored_values() {
        for (psi, hp) in [(1, 150), (5, 150), (6, 200), (8, 300), (10, 400)] {
            assert_eq!(hit_points([150.0, 50.0, 5.0, 0.0], psi), hp);
        }
    }
    #[test]
    fn barrier_age_round_trips_without_refreshing_the_lifetime() {
        use std::{collections::HashMap, time::Duration};
        let mut world = World::new();
        let entity = world.add_entity(());
        let physics = PhysicsWorld::new();
        let mut wall = ForceWallStructure::default();
        assert!(matches!(
            wall.update(
                entity,
                &world,
                &physics,
                &Time {
                    elapsed: Duration::from_secs(239),
                    total: Duration::ZERO,
                }
            ),
            Effect::NoEffect
        ));
        let mut restored = ForceWallStructure::default();
        restored
            .restore_state(
                &wall.save_state().unwrap(),
                &ScriptRestoreContext::new(&HashMap::new()),
            )
            .unwrap();
        assert!(matches!(restored.update(entity, &world, &physics, &Time {
            elapsed: Duration::from_secs(1), total: Duration::ZERO,
        }), Effect::DestroyEntity { entity_id } if entity_id == entity));
    }
}
