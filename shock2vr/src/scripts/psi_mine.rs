//! Script `PsiMine`: the projectile lobbed by the tier-5 psi power PsiMines
//! (External Psionic Detonation).
//!
//! The mine (`PsiMine Projectile`, -3397) is a slow physics projectile that,
//! unlike the other psi bolts, is not slain on impact (`BOUNCE`, no
//! `SLAY_ON_IMPACT`) and is authored weightless (`gravity_scale 0.0`): it
//! drifts and bounces rather than falling. It arms shortly after the cast,
//! then detonates when a creature comes within reach of its body or when it is
//! damaged, and expires if nothing trips it.
//!
//! The blast is authored data, not code: slaying the mine spawns its
//! `Corpse -> Psi Mine Explosion`(-3756), whose radius stim source
//! (30.0 @ r4.0 of `Psi Stim`) is resolved by `internal_explosion` like any
//! other explosion.

use cgmath::{InnerSpace, Point3, Transform, point3};
use collision::Aabb3;
use dark::properties::{PropCreature, PropPhysDimensions};
use serde::{Deserialize, Serialize};
use shipyard::{EntityId, Get, IntoIter, IntoWithId, View, World};
use std::time::Duration;

use crate::{physics::PhysicsWorld, runtime_props::RuntimePropTransform, time::Time};

use super::{
    Effect, MessagePayload, Script, ScriptRestoreContext, ScriptState, ScriptStateError,
    ai::ai_util::is_killed,
};

/// How long after the cast the mine starts sensing: enough to clear the
/// caster's own body (it is lobbed from their position, and the blast reaches
/// them too), short enough that a mine thrown at a creature a pace away is
/// live by the time it arrives.
const ARM_DELAY: Duration = Duration::from_millis(100);

/// Sensing reach when the mine authors no physics dimensions, which the shipped
/// `PsiMine Projectile` does (`radius0` 1.4 - seven times the projectile
/// default, the mine's own body).
const DEFAULT_TRIGGER_RADIUS: f32 = 1.4;

/// How long an untripped mine lasts before it quietly expires, so a cast that
/// found nothing does not leave a live sensor drifting through the level
/// forever.
const LIFETIME: Duration = Duration::from_secs(60);

const SCRIPT_STATE_KEY: &str = "shock2vr.psi_mine";

#[derive(Serialize, Deserialize)]
struct PsiMineState {
    age_secs: f32,
}

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
        physics: &PhysicsWorld,
        time: &Time,
    ) -> Effect {
        self.age += time.elapsed;

        if self.age < ARM_DELAY {
            return Effect::NoEffect;
        }
        if self.age >= LIFETIME {
            return Effect::DestroyEntity { entity_id };
        }

        if creature_in_reach(world, physics, entity_id) {
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

    fn script_state_key(&self) -> Option<&'static str> {
        Some(SCRIPT_STATE_KEY)
    }

    fn save_state(&self) -> Result<ScriptState, ScriptStateError> {
        ScriptState::encode(
            1,
            &PsiMineState {
                age_secs: self.age.as_secs_f32(),
            },
            SCRIPT_STATE_KEY,
        )
    }

    fn restore_state(
        &mut self,
        state: &ScriptState,
        _context: &ScriptRestoreContext<'_>,
    ) -> Result<(), ScriptStateError> {
        let restored: PsiMineState = state.decode(1, SCRIPT_STATE_KEY)?;
        self.age = Duration::from_secs_f32(restored.age_secs.max(0.0));
        Ok(())
    }
}

/// Whether a living creature has come within the mine's sensing reach - its own
/// authored body radius, measured to the creature's collider rather than to its
/// origin, so a body standing beside the mine trips it and one across the room
/// does not.
fn creature_in_reach(world: &World, physics: &PhysicsWorld, mine_entity_id: EntityId) -> bool {
    let v_transform = world.borrow::<View<RuntimePropTransform>>().unwrap();
    let Ok(mine_transform) = v_transform.get(mine_entity_id) else {
        return false;
    };
    let mine_position = mine_transform.0.transform_point(point3(0.0, 0.0, 0.0));
    let radius = world
        .borrow::<View<PropPhysDimensions>>()
        .ok()
        .and_then(|dimensions| dimensions.get(mine_entity_id).ok().map(|d| d.radius0.abs()))
        .filter(|radius| *radius > 0.0)
        .unwrap_or(DEFAULT_TRIGGER_RADIUS);

    let v_creature = world.borrow::<View<PropCreature>>().unwrap();

    v_creature.iter().with_id().any(
        |(entity_id, _creature)| match physics.get_aabb2(entity_id) {
            Some(bounds) => {
                !is_killed(entity_id, world) && distance_to_bounds(mine_position, &bounds) <= radius
            }
            // A creature with no live collider (contained, not yet in the
            // world) is nothing to trip on.
            None => false,
        },
    )
}

/// Distance from a point to the nearest point of a box - zero inside it.
fn distance_to_bounds(point: Point3<f32>, bounds: &Aabb3<f32>) -> f32 {
    let nearest = point3(
        point.x.clamp(bounds.min.x, bounds.max.x),
        point.y.clamp(bounds.min.y, bounds.max.y),
        point.z.clamp(bounds.min.z, bounds.max.z),
    );
    (point - nearest).magnitude()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::physics::CollisionGroup;
    use cgmath::{Matrix4, Quaternion, vec3};
    use dark::properties::PropHitPoints;
    use std::collections::HashMap;

    /// A mine at the origin and one human creature, its collider a 1-unit cube
    /// centered on `creature_position`.
    fn mine_world(
        creature_position: cgmath::Vector3<f32>,
        hit_points: i32,
    ) -> (World, PhysicsWorld, EntityId) {
        let mut world = World::new();
        let mut physics = PhysicsWorld::new();

        let mine = world.add_entity((
            RuntimePropTransform(Matrix4::from_translation(vec3(0.0, 0.0, 0.0))),
            PropPhysDimensions {
                radius0: DEFAULT_TRIGGER_RADIUS,
                radius1: 0.0,
                offset0: vec3(0.0, 0.0, 0.0),
                offset1: vec3(0.0, 0.0, 0.0),
                size: vec3(0.0, 0.0, 0.0),
                unk1: 1,
                unk2: 1,
            },
        ));
        let creature = world.add_entity((PropCreature(0), PropHitPoints { hit_points }));
        physics.add_kinematic(
            creature,
            creature_position,
            Quaternion::new(1.0, 0.0, 0.0, 0.0),
            vec3(0.0, 0.0, 0.0),
            vec3(1.0, 1.0, 1.0),
            CollisionGroup::actor(),
            false,
        );
        let mut player =
            physics.create_player(vec3(0.0, 0.0, -50.0), EntityId::from_inner(9).unwrap());
        physics.update(vec3(0.0, 0.0, 0.0), &mut player);

        (world, physics, mine)
    }

    fn update(
        script: &mut PsiMine,
        world: &World,
        physics: &PhysicsWorld,
        mine: EntityId,
        elapsed: Duration,
    ) -> Effect {
        script.update(
            mine,
            world,
            physics,
            &Time {
                elapsed,
                total: Duration::ZERO,
            },
        )
    }

    #[test]
    fn an_armed_mine_detonates_on_a_creature_within_reach() {
        // Collider face at x = 1.4, exactly the mine's authored reach.
        let (world, physics, mine) = mine_world(vec3(1.9, 0.0, 0.0), 12);
        let mut script = PsiMine::new();

        let effect = update(&mut script, &world, &physics, mine, ARM_DELAY);

        assert!(
            matches!(effect, Effect::SlayEntity { entity_id } if entity_id == mine),
            "a creature within reach sets the mine off, spawning its authored corpse explosion",
        );
    }

    /// The mine leaves the amp at the caster's own position: it must not trip
    /// on whatever stands there before it has left the hand.
    #[test]
    fn an_unarmed_mine_ignores_a_creature_on_top_of_it() {
        let (world, physics, mine) = mine_world(vec3(0.5, 0.0, 0.0), 12);
        let mut script = PsiMine::new();

        let effect = update(&mut script, &world, &physics, mine, ARM_DELAY / 2);

        assert!(matches!(effect, Effect::NoEffect));
    }

    #[test]
    fn an_armed_mine_ignores_a_creature_out_of_reach() {
        let (world, physics, mine) = mine_world(vec3(3.0, 0.0, 0.0), 12);
        let mut script = PsiMine::new();

        let effect = update(&mut script, &world, &physics, mine, ARM_DELAY);

        assert!(matches!(effect, Effect::NoEffect));
    }

    #[test]
    fn a_corpse_does_not_trip_a_mine() {
        let (world, physics, mine) = mine_world(vec3(1.0, 0.0, 0.0), 0);
        let mut script = PsiMine::new();

        let effect = update(&mut script, &world, &physics, mine, ARM_DELAY);

        assert!(matches!(effect, Effect::NoEffect));
    }

    #[test]
    fn an_untripped_mine_expires() {
        let (world, physics, mine) = mine_world(vec3(100.0, 0.0, 0.0), 12);
        let mut script = PsiMine::new();

        let effect = update(&mut script, &world, &physics, mine, LIFETIME);

        assert!(matches!(effect, Effect::DestroyEntity { entity_id } if entity_id == mine));
    }

    #[test]
    fn a_damaged_mine_detonates() {
        let (world, physics, mine) = mine_world(vec3(100.0, 0.0, 0.0), 12);
        let mut script = PsiMine::new();

        let effect = script.handle_message(
            mine,
            &world,
            &physics,
            &MessagePayload::Damage {
                amount: 1.0,
                impact: None,
            },
        );

        assert!(matches!(effect, Effect::SlayEntity { entity_id } if entity_id == mine));
    }

    /// A mine live in the world when the game is saved keeps its fuse: it is
    /// still armed on load, and still expires on schedule.
    #[test]
    fn the_fuse_survives_save_and_load() {
        let (world, physics, mine) = mine_world(vec3(100.0, 0.0, 0.0), 12);
        let mut before_save = PsiMine::new();
        update(
            &mut before_save,
            &world,
            &physics,
            mine,
            LIFETIME - Duration::from_millis(10),
        );

        let state = before_save.save_state().unwrap();
        let mut after_load = PsiMine::new();
        after_load
            .restore_state(&state, &ScriptRestoreContext::new(&HashMap::new()))
            .unwrap();

        let effect = update(
            &mut after_load,
            &world,
            &physics,
            mine,
            Duration::from_millis(10),
        );
        assert!(
            matches!(effect, Effect::DestroyEntity { entity_id } if entity_id == mine),
            "a restored mine expires on its original schedule, not 60s after the load",
        );
    }
}
