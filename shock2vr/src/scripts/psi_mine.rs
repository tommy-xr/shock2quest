//! Script `PsiMine`: the projectile lobbed by the tier-5 psi power PsiMines
//! (External Psionic Detonation).
//!
//! The mine (`PsiMine Projectile`, -3397) is a slow physics projectile -
//! unlike the other psi bolts it has no `SLAY_ON_IMPACT`, so it comes to rest
//! where it lands. It arms shortly after the cast, then detonates when a
//! creature comes within its trigger radius or when it is damaged. The blast
//! itself is authored data, not code: slaying the mine spawns its
//! `Corpse -> Psi Mine Explosion`(-3756), whose radius stim source
//! (30.0 @ r4.0 of `Psi Stim`) is resolved by `internal_explosion` like any
//! other explosion.

use cgmath::{InnerSpace, Transform, point3};
use dark::properties::{PropAI, PropHitPoints};
use shipyard::{EntityId, Get, IntoIter, IntoWithId, View, World};
use std::time::Duration;

use crate::{physics::PhysicsWorld, runtime_props::RuntimePropTransform, time::Time};

use super::{Effect, MessagePayload, Script};

/// How long after the cast the mine starts sensing: long enough to leave the
/// caster (it is lobbed from their own position, and the blast reaches them
/// too), short enough that a mine thrown at a creature a few paces away is
/// live by the time it gets there.
const ARM_DELAY: Duration = Duration::from_millis(250);

/// How close a creature must come to set the mine off, measured to the
/// creature's origin. Inside the authored blast radius (`Psi Mine Explosion`'s
/// stim source is 30.0 @ r4.0), whose damage falls off linearly to nothing at
/// its edge - a mine that tripped at the full radius would deal nothing to
/// what tripped it - but with room for the body's own extent around that
/// origin.
const TRIGGER_RADIUS: f32 = 3.0;

/// How long an untripped mine lasts before it quietly expires, so a cast that
/// found nothing does not leave a live sensor in the level forever.
const LIFETIME: Duration = Duration::from_secs(60);

pub struct PsiMine {
    age: Duration,
}

impl PsiMine {
    pub fn new() -> PsiMine {
        PsiMine {
            age: Duration::ZERO,
        }
    }
}

impl Script for PsiMine {
    fn update(
        &mut self,
        entity_id: EntityId,
        world: &World,
        _physics: &PhysicsWorld,
        time: &Time,
    ) -> Effect {
        self.age += time.elapsed;

        if self.age < ARM_DELAY {
            return Effect::NoEffect;
        }
        if self.age >= LIFETIME {
            return Effect::DestroyEntity { entity_id };
        }

        if creature_in_range(world, entity_id, TRIGGER_RADIUS) {
            Effect::SlayEntity { entity_id }
        } else {
            Effect::NoEffect
        }
    }

    fn handle_message(
        &mut self,
        entity_id: EntityId,
        _world: &World,
        _physics: &PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        // A shot mine goes off where it lies.
        match msg {
            MessagePayload::Damage { .. } => Effect::SlayEntity { entity_id },
            _ => Effect::NoEffect,
        }
    }
}

/// Whether a living creature stands within `radius` of the mine. Creatures are
/// the AI-driven entities (`P$AI`); a corpse - hit points spent - no longer
/// trips a mine.
fn creature_in_range(world: &World, mine_entity_id: EntityId, radius: f32) -> bool {
    let v_transform = world.borrow::<View<RuntimePropTransform>>().unwrap();
    let Ok(mine_transform) = v_transform.get(mine_entity_id) else {
        return false;
    };
    let mine_position = mine_transform.0.transform_point(point3(0.0, 0.0, 0.0));

    let v_ai = world.borrow::<View<PropAI>>().unwrap();
    let v_hit_points = world.borrow::<View<PropHitPoints>>().unwrap();

    (&v_ai, &v_hit_points, &v_transform).iter().with_id().any(
        |(entity_id, (_ai, hit_points, transform))| {
            entity_id != mine_entity_id
                && hit_points.hit_points > 0
                && (transform.0.transform_point(point3(0.0, 0.0, 0.0)) - mine_position).magnitude()
                    <= radius
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use cgmath::{Matrix4, vec3};

    fn mine_world(creature_position: cgmath::Vector3<f32>, hit_points: i32) -> (World, EntityId) {
        let mut world = World::new();
        let mine = world.add_entity(RuntimePropTransform(Matrix4::from_translation(vec3(
            0.0, 0.0, 0.0,
        ))));
        world.add_entity((
            PropAI("Grunt".to_owned()),
            PropHitPoints { hit_points },
            RuntimePropTransform(Matrix4::from_translation(creature_position)),
        ));
        (world, mine)
    }

    fn update(script: &mut PsiMine, world: &World, mine: EntityId, elapsed: Duration) -> Effect {
        script.update(
            mine,
            world,
            &PhysicsWorld::new(),
            &Time {
                elapsed,
                total: Duration::ZERO,
            },
        )
    }

    #[test]
    fn an_armed_mine_detonates_on_a_creature_within_its_radius() {
        let (world, mine) = mine_world(vec3(2.0, 0.0, 0.0), 12);
        let mut script = PsiMine::new();

        let effect = update(&mut script, &world, mine, ARM_DELAY);

        assert!(
            matches!(effect, Effect::SlayEntity { entity_id } if entity_id == mine),
            "a creature inside the trigger radius sets the mine off, spawning its authored corpse explosion",
        );
    }

    /// The mine leaves the amp at the caster's own position: it must not trip
    /// on whatever stands there before it has left the hand.
    #[test]
    fn an_unarmed_mine_ignores_a_creature_on_top_of_it() {
        let (world, mine) = mine_world(vec3(0.5, 0.0, 0.0), 12);
        let mut script = PsiMine::new();

        let effect = update(&mut script, &world, mine, ARM_DELAY / 2);

        assert!(matches!(effect, Effect::NoEffect));
    }

    #[test]
    fn an_armed_mine_ignores_a_creature_out_of_range() {
        let (world, mine) = mine_world(vec3(TRIGGER_RADIUS + 1.0, 0.0, 0.0), 12);
        let mut script = PsiMine::new();

        let effect = update(&mut script, &world, mine, ARM_DELAY);

        assert!(matches!(effect, Effect::NoEffect));
    }

    #[test]
    fn a_corpse_does_not_trip_a_mine() {
        let (world, mine) = mine_world(vec3(1.0, 0.0, 0.0), 0);
        let mut script = PsiMine::new();

        let effect = update(&mut script, &world, mine, ARM_DELAY);

        assert!(matches!(effect, Effect::NoEffect));
    }

    #[test]
    fn an_untripped_mine_expires() {
        let (world, mine) = mine_world(vec3(100.0, 0.0, 0.0), 12);
        let mut script = PsiMine::new();

        let effect = update(&mut script, &world, mine, LIFETIME);

        assert!(matches!(effect, Effect::DestroyEntity { entity_id } if entity_id == mine));
    }

    #[test]
    fn a_damaged_mine_detonates() {
        let (world, mine) = mine_world(vec3(100.0, 0.0, 0.0), 12);
        let mut script = PsiMine::new();

        let effect = script.handle_message(
            mine,
            &world,
            &PhysicsWorld::new(),
            &MessagePayload::Damage {
                amount: 1.0,
                impact: None,
            },
        );

        assert!(matches!(effect, Effect::SlayEntity { entity_id } if entity_id == mine));
    }
}
