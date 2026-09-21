//! Render-only Smasher feedback. Never write the tracked or physical pose.
use cgmath::{InnerSpace, Matrix4, SquareMatrix, Vector3, vec3};
use shipyard::{EntityId, Get, View, World};

use crate::runtime_props::{RuntimePropMeleeCharge, RuntimePropTransform};

/// Shared by the glove, weapon mesh and psi blade so their grip cannot separate.
pub(crate) fn transform(world: &World, entity: Option<EntityId>) -> Matrix4<f32> {
    let Some(entity) = entity.filter(|id| {
        crate::mission::presentation_is_vr(world) && crate::wielded_weapon::held_in_hand(world, *id)
    }) else {
        return Matrix4::identity();
    };
    let charges = world.borrow::<View<RuntimePropMeleeCharge>>().unwrap();
    let transforms = world.borrow::<View<RuntimePropTransform>>().unwrap();
    let (Ok(charge), Ok(pose)) = (charges.get(entity), transforms.get(entity)) else {
        return Matrix4::identity();
    };
    Matrix4::from_translation(offset(charge.held_seconds, pose.0))
}

fn offset(held_seconds: Option<f32>, pose: Matrix4<f32>) -> Vector3<f32> {
    let Some(seconds) = held_seconds.filter(|s| s.is_finite() && *s >= 0.0) else {
        return vec3(0.0, 0.0, 0.0);
    };
    let x = pose.x.truncate();
    let y = pose.y.truncate();
    if !x.magnitude2().is_finite()
        || !y.magnitude2().is_finite()
        || x.magnitude2() < 1e-8
        || y.magnitude2() < 1e-8
    {
        return vec3(0.0, 0.0, 0.0);
    }
    let charge_seconds = crate::scripts::melee_charge::CHARGE_SECONDS;
    let strength = (seconds / charge_seconds).min(1.0).powi(2);
    let phase = seconds * std::f32::consts::TAU;
    // A few millimetres of tremor in weapon-local axes. Normalize to avoid authored
    // model scale changing the perceived movement; rendering uses game units.
    let tremor_x = 0.003 * strength * (phase * 11.0).sin();
    let tremor_y = 0.0015 * strength * (phase * 17.0).sin();
    let ready_age = seconds - charge_seconds;
    let kick = if (0.0..0.12).contains(&ready_age) {
        0.007 * (std::f32::consts::PI * ready_age / 0.12).sin().powi(2)
    } else {
        0.0
    };
    (x.normalize() * tremor_x + y.normalize() * (tremor_y + kick)) / crate::METERS_PER_WORLD_UNIT
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scripts::melee_charge::CHARGE_SECONDS;
    use cgmath::{Deg, InnerSpace, Transform};

    #[test]
    fn tremor_builds_and_ready_kicks_once_then_release_stops() {
        let pose = Matrix4::identity();
        assert_eq!(offset(Some(0.0), pose), vec3(0.0, 0.0, 0.0));
        assert_eq!(offset(None, pose), vec3(0.0, 0.0, 0.0));
        assert!(offset(Some(0.31), pose).magnitude() > 0.001);
        let ready_peak = offset(Some(CHARGE_SECONDS + 0.06), pose);
        assert!(ready_peak.magnitude() * crate::METERS_PER_WORLD_UNIT > 0.005);
        for i in 0..600 {
            let elapsed = i as f32 / 60.0;
            let meters = offset(Some(elapsed), pose).magnitude() * crate::METERS_PER_WORLD_UNIT;
            assert!(meters < 0.012, "bounded cosmetic movement: {meters}");
            if elapsed > CHARGE_SECONDS + 0.12 {
                assert!(meters < 0.004, "ready kick is brief");
            }
        }
    }

    #[test]
    fn motion_follows_weapon_axes_without_scaling_or_moving_the_physical_pose() {
        let pose = Matrix4::from_translation(vec3(4.0, 2.0, 1.0))
            * Matrix4::from_angle_y(Deg(90.0))
            * Matrix4::from_scale(0.2);
        let local = offset(Some(0.31), Matrix4::identity());
        let expected = Matrix4::from_angle_y(Deg(90.0)).transform_vector(local);
        assert!((offset(Some(0.31), pose) - expected).magnitude() < 1e-5);
        for seconds in [f32::NAN, f32::INFINITY, -1.0] {
            assert_eq!(offset(Some(seconds), pose), vec3(0.0, 0.0, 0.0));
        }
    }

    #[test]
    fn only_a_held_vr_charge_moves_and_released_ready_is_still() {
        use crate::PresentationMode;
        use crate::mission::{GlobalPresentationMode, PlayerInfo};
        use cgmath::Quaternion;
        let mut world = World::new();
        let physical = Matrix4::from_translation(vec3(3.0, 2.0, 1.0));
        let weapon = world.add_entity((
            RuntimePropTransform(physical),
            RuntimePropMeleeCharge {
                fraction: 1.0,
                held_seconds: Some(0.44),
            },
        ));
        world.add_unique(GlobalPresentationMode(PresentationMode::Vr));
        world.add_unique(PlayerInfo {
            pos: vec3(0.0, 0.0, 0.0),
            rotation: Quaternion::new(1.0, 0.0, 0.0, 0.0),
            entity_id: EntityId::dead(),
            inventory_entity_id: EntityId::dead(),
            left_hand_entity_id: Some(weapon),
            right_hand_entity_id: None,
        });
        assert_ne!(transform(&world, Some(weapon)), Matrix4::identity());
        assert_eq!(
            world
                .borrow::<View<RuntimePropTransform>>()
                .unwrap()
                .get(weapon)
                .unwrap()
                .0,
            physical
        );
        assert_eq!(transform(&world, None), Matrix4::identity());
        world
            .borrow::<shipyard::UniqueViewMut<GlobalPresentationMode>>()
            .unwrap()
            .0 = PresentationMode::Flat;
        assert_eq!(transform(&world, Some(weapon)), Matrix4::identity());
        world
            .borrow::<shipyard::UniqueViewMut<GlobalPresentationMode>>()
            .unwrap()
            .0 = PresentationMode::Vr;
        world.add_component(
            weapon,
            RuntimePropMeleeCharge {
                fraction: 1.0,
                held_seconds: None,
            },
        );
        assert_eq!(transform(&world, Some(weapon)), Matrix4::identity());
        world.add_component(
            weapon,
            RuntimePropMeleeCharge {
                fraction: 1.0,
                held_seconds: Some(0.44),
            },
        );
        world
            .borrow::<shipyard::UniqueViewMut<PlayerInfo>>()
            .unwrap()
            .left_hand_entity_id = None;
        assert_eq!(transform(&world, Some(weapon)), Matrix4::identity());
    }
}
