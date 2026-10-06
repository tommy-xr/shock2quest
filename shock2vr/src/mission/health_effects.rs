//! Property-only health transitions. Combat feedback and death remain in the
//! ordinary damage effect; shield regeneration must not synthesize a hit.
use dark::properties::{PropHitPoints, PropMaxHitPoints, PropRenderAlpha};
use shipyard::{EntityId, Get, View, ViewMut, World};

pub(super) fn regenerate_hit_points(world: &World, entity_id: EntityId, amount: i32, cap: i32) {
    let mut health = world.borrow::<ViewMut<PropHitPoints>>().unwrap();
    if let Ok(hp) = (&mut health).get(entity_id) {
        // A lethal hit may already have queued the authored death path.
        if hp.hit_points > 0 {
            hp.hit_points = hp.hit_points.saturating_add(amount).min(cap).max(0);
        }
    }
}

pub(super) fn set_alpha_from_hit_points(world: &mut World, entity_id: EntityId) {
    let alpha = {
        let health = world.borrow::<View<PropHitPoints>>().unwrap();
        let maximums = world.borrow::<View<PropMaxHitPoints>>().unwrap();
        let (Ok(hp), Ok(maximum)) = (health.get(entity_id), maximums.get(entity_id)) else {
            return;
        };
        if maximum.hit_points == 0 {
            0.0
        } else {
            (hp.hit_points as f32 / maximum.hit_points as f32).clamp(0.0, 1.0)
        }
    };
    world.add_component(entity_id, PropRenderAlpha(alpha));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hp_and_alpha(world: &World, id: EntityId) -> (i32, f32) {
        (
            world
                .borrow::<View<PropHitPoints>>()
                .unwrap()
                .get(id)
                .unwrap()
                .hit_points,
            world
                .borrow::<View<PropRenderAlpha>>()
                .unwrap()
                .get(id)
                .unwrap()
                .0,
        )
    }

    #[test]
    fn regeneration_clamps_authored_pool_and_heals_only_to_eighty_percent() {
        let mut world = World::new();
        let id = world.add_entity((
            PropHitPoints { hit_points: 115 },
            PropMaxHitPoints { hit_points: 140 },
        ));
        regenerate_hit_points(&world, id, 1, 112);
        set_alpha_from_hit_points(&mut world, id);
        assert_eq!(hp_and_alpha(&world, id), (112, 0.8));
        world.add_component(id, PropHitPoints { hit_points: 102 });
        // Damage followed by a tick must heal the new 102 HP pool, not replace
        // the shot with a value calculated from the pre-message 112 HP.
        regenerate_hit_points(&world, id, 1, 112);
        set_alpha_from_hit_points(&mut world, id);
        assert_eq!(hp_and_alpha(&world, id), (103, 103.0 / 140.0));
        for _ in 0..20 {
            regenerate_hit_points(&world, id, 1, 112);
        }
        set_alpha_from_hit_points(&mut world, id);
        assert_eq!(hp_and_alpha(&world, id), (112, 0.8));
    }

    #[test]
    fn alpha_normalizes_current_health_and_regeneration_does_not_revive() {
        for (hp, maximum, expected) in [
            (70, 140, 0.5),
            (160, 140, 1.0),
            (-5, 140, 0.0),
            (10, 0, 0.0),
        ] {
            let mut world = World::new();
            let id = world.add_entity((
                PropHitPoints { hit_points: hp },
                PropMaxHitPoints {
                    hit_points: maximum,
                },
            ));
            set_alpha_from_hit_points(&mut world, id);
            assert_eq!(hp_and_alpha(&world, id).1, expected);
        }
        let mut world = World::new();
        let id = world.add_entity((
            PropHitPoints { hit_points: 0 },
            PropMaxHitPoints { hit_points: 140 },
        ));
        regenerate_hit_points(&world, id, 1, 112);
        set_alpha_from_hit_points(&mut world, id);
        assert_eq!(hp_and_alpha(&world, id), (0, 0.0));
        let missing = world.add_entity(());
        regenerate_hit_points(&world, missing, 1, 112);
        set_alpha_from_hit_points(&mut world, missing);
        assert!(
            world
                .borrow::<View<PropRenderAlpha>>()
                .unwrap()
                .get(missing)
                .is_err()
        );
    }
}
