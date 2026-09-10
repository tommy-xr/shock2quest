//! Shkproj.cpp's per-projectile spray, independent of gun accuracy and recoil.
use std::collections::{HashMap, HashSet};

use cgmath::{EuclideanSpace, InnerSpace, Matrix4, Transform, point3, vec3};
use dark::{
    properties::{Link, PropProjectile},
    ss2_entity_info::SystemShock2EntityInfo,
};
use rand::Rng;
use shipyard::Unique;

use crate::{
    scripts::{Effect, script_util::hydrate_template_component},
    util::get_rotation_from_forward_vector,
};

#[derive(Unique, Default)]
pub(crate) struct GlobalProjectileSprays(pub HashMap<i32, PropProjectile>);

impl GlobalProjectileSprays {
    pub fn from_entity_info(info: &SystemShock2EntityInfo) -> Self {
        // Only hydrate archetypes a gun can launch, including inherited links.
        // The existing hydrator resolves mission overrides and property donors.
        let projectiles: HashSet<_> = info
            .template_to_links
            .values()
            .flat_map(|links| &links.to_links)
            .filter(|link| matches!(link.link, Link::Projectile(_)))
            .map(|link| link.to_template_id)
            .collect();
        Self(
            projectiles
                .into_iter()
                .filter_map(|id| {
                    hydrate_template_component::<PropProjectile>(id, info).map(|prop| (id, prop))
                })
                .collect(),
        )
    }
}

/// Expand just the launch effect: ammo, sound, flash, casing and wear remain
/// once per shell. Each pellet retains its owner, damage modifiers and origins.
pub(crate) fn expand(spray: PropProjectile, launch: Effect, rng: &mut impl Rng) -> Effect {
    if spray == PropProjectile::default() {
        return launch;
    }
    Effect::Multiple(
        (0..spray.count)
            .map(|_| deviate(launch.clone(), spray.spread, rng))
            .collect(),
    )
}

/// Apply a single global heading/pitch error before any per-pellet spray.
/// Dark uses the same randomization routine for weapon error and pellet spread.
/// Zero error must not consume RNG or alter the resolved launch transform.
pub(crate) fn deviate(mut launch: Effect, spread: u16, rng: &mut impl Rng) -> Effect {
    if spread == 0 {
        return launch;
    }
    if let Effect::CreateEntity { root_transform, .. } = &mut launch {
        let forward = root_transform
            .transform_vector(vec3(0.0, 0.0, 1.0))
            .normalize();
        // Independent global heading/pitch angles, not a circular cone; gun
        // roll does not rotate the distribution.
        let angle = i32::from(spread);
        let radians = std::f32::consts::TAU / 65536.0;
        let heading = forward.x.atan2(forward.z) + rng.gen_range(-angle..=angle) as f32 * radians;
        let pitch =
            forward.y.clamp(-1.0, 1.0).asin() + rng.gen_range(-angle..=angle) as f32 * radians;
        let direction = vec3(
            heading.sin() * pitch.cos(),
            pitch.sin(),
            heading.cos() * pitch.cos(),
        );
        let origin = root_transform.transform_point(point3(0.0, 0.0, 0.0));
        *root_transform = Matrix4::from_translation(origin.to_vec())
            * Matrix4::from(get_rotation_from_forward_vector(direction));
    }
    launch
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mission::entity_creator::CreateEntityOptions;
    use cgmath::{Deg, Quaternion, Rotation3};
    use rand::{SeedableRng, rngs::StdRng};

    fn launch() -> Effect {
        Effect::CreateEntity {
            template_id: -524,
            position: point3(0.0, 0.0, 0.0),
            orientation: Quaternion::from_angle_y(Deg(90.0)),
            root_transform: Matrix4::from_translation(vec3(2.0, 3.0, 4.0)),
            options: CreateEntityOptions {
                player_fired_projectile: true,
                projectile_raycast_origin: Some(point3(1.0, 3.0, 4.0)),
                ..Default::default()
            },
        }
    }

    #[test]
    fn weapon_error_is_sampled_once_before_pellets_and_zero_preserves_rng() {
        let mut rng = StdRng::seed_from_u64(81);
        let unchanged = deviate(launch(), 0, &mut rng);
        assert!(matches!(unchanged, Effect::CreateEntity { .. }));
        let mut untouched_rng = StdRng::seed_from_u64(81);
        assert_eq!(rng.r#gen::<u32>(), untouched_rng.r#gen::<u32>());
        let aimed = deviate(launch(), 640, &mut rng);
        let Effect::CreateEntity {
            root_transform: expected,
            ..
        } = &aimed
        else {
            panic!("launch")
        };
        assert_ne!(
            expected.transform_vector(vec3(0.0, 0.0, 1.0)),
            vec3(0.0, 0.0, 1.0)
        );
        let expected = *expected;
        let pellets = Effect::flatten(vec![expand(
            PropProjectile {
                count: 6,
                spread: 0,
            },
            aimed,
            &mut rng,
        )]);
        assert_eq!(pellets.len(), 6);
        for pellet in pellets {
            let Effect::CreateEntity { root_transform, .. } = pellet else {
                panic!("pellet")
            };
            assert_eq!(
                root_transform, expected,
                "one common shell error, then independent pellet spread"
            );
        }
    }

    #[test]
    fn six_pellets_preserve_origins_and_have_independent_bounded_angles() {
        let mut rng = StdRng::seed_from_u64(42);
        let effects = Effect::flatten(vec![expand(
            PropProjectile {
                count: 6,
                spread: 1024,
            },
            launch(),
            &mut rng,
        )]);
        assert_eq!(effects.len(), 6);
        let mut directions = Vec::new();
        for effect in effects {
            let Effect::CreateEntity {
                root_transform,
                options,
                template_id,
                ..
            } = effect
            else {
                panic!("launch")
            };
            assert_eq!(template_id, -524);
            assert!(options.player_fired_projectile);
            assert_eq!(
                options.projectile_raycast_origin,
                Some(point3(1.0, 3.0, 4.0))
            );
            assert_eq!(
                root_transform.transform_point(point3(0.0, 0.0, 0.0)),
                point3(2.0, 3.0, 4.0)
            );
            let direction = root_transform.transform_vector(vec3(0.0, 0.0, 1.0));
            assert!((direction.magnitude() - 1.0).abs() < 0.00001);
            assert!(direction.x.atan2(direction.z).abs() <= 5.625_f32.to_radians());
            assert!(direction.y.asin().abs() <= 5.625_f32.to_radians());
            assert!(
                !directions.contains(&direction),
                "each pellet samples independently"
            );
            directions.push(direction);
        }
        let next = expand(
            PropProjectile {
                count: 6,
                spread: 1024,
            },
            launch(),
            &mut rng,
        );
        assert_ne!(
            format!("{next:?}"),
            format!(
                "{:?}",
                expand(
                    PropProjectile {
                        count: 6,
                        spread: 1024
                    },
                    launch(),
                    &mut StdRng::seed_from_u64(42)
                )
            ),
            "successive shells must not repeat the same pattern"
        );
    }

    #[test]
    fn slug_without_projectile_property_stays_a_single_straight_launch() {
        let effect = expand(
            PropProjectile::default(),
            launch(),
            &mut StdRng::seed_from_u64(42),
        );
        let Effect::CreateEntity { root_transform, .. } = effect else {
            panic!("single launch")
        };
        assert_eq!(
            root_transform.transform_vector(vec3(0.0, 0.0, 1.0)),
            vec3(0.0, 0.0, 1.0)
        );
    }
}
