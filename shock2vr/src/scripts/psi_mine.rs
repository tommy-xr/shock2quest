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
use dark::properties::{PropAI, PropHitPoints, PropPhysDimensions};
use serde::{Deserialize, Serialize};
use shipyard::{EntityId, Get, IntoIter, IntoWithId, View, World};
use std::time::Duration;

use crate::{physics::PhysicsWorld, runtime_props::RuntimePropTransform, time::Time};

use super::{Effect, MessagePayload, Script, ScriptRestoreContext, ScriptState, ScriptStateError};

/// How long after the cast the mine starts sensing: enough to clear the
/// caster's own body (it is lobbed from their position, and the blast reaches
/// them too), short enough that a mine thrown at a creature a pace away is
/// live by the time it arrives.
const ARM_DELAY: Duration = Duration::from_millis(100);

/// Sensing reach for a mine that authors no physics dimensions. The shipped
/// `PsiMine Projectile` authors `radius0` 1.4 - seven times the projectile
/// default, the mine's own body - and that authored value is what is used.
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

/// Whether a live AI has come within the mine's sensing reach - its own
/// authored body radius, measured to the bounds of the creature's collider
/// rather than to its origin, so a body standing beside the mine trips it and
/// one across the room does not.
///
/// "Live AI" is `P$AI` plus hit points left, not `PropCreature`: the authored
/// corpses scattered through the levels carry `PropCreature` with no hit
/// points at all, and a mine must not go off on the scenery.
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

    let v_ai = world.borrow::<View<PropAI>>().unwrap();
    let v_hit_points = world.borrow::<View<PropHitPoints>>().unwrap();

    (&v_ai, &v_hit_points)
        .iter()
        .with_id()
        .any(|(entity_id, (_ai, hit_points))| {
            hit_points.hit_points > 0
                // No live collider (contained, not yet in the world) is
                // nothing to trip on.
                && physics
                    .get_aabb2(entity_id)
                    .is_some_and(|bounds| distance_to_bounds(mine_position, &bounds) <= radius)
        })
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
    use dark::properties::PropCreature;
    use std::collections::HashMap;

    fn dimensions(radius0: f32) -> PropPhysDimensions {
        PropPhysDimensions {
            radius0,
            radius1: 0.0,
            offset0: vec3(0.0, 0.0, 0.0),
            offset1: vec3(0.0, 0.0, 0.0),
            size: vec3(0.0, 0.0, 0.0),
            point_vs_terrain: 1,
            point_vs_not_special: 1,
        }
    }

    /// A mine at the origin with the authored sensing radius, and one live AI
    /// whose collider is a 1-unit cube centered on `creature_position`.
    fn mine_world(
        creature_position: cgmath::Vector3<f32>,
        hit_points: i32,
        mine_radius: f32,
    ) -> (World, PhysicsWorld, EntityId) {
        let (mut world, mut physics, mine) = empty_mine_world(mine_radius);
        let creature = world.add_entity((PropAI("Grunt".to_owned()), PropHitPoints { hit_points }));
        add_body(&mut physics, creature, creature_position);
        settle(&mut physics);
        (world, physics, mine)
    }

    fn empty_mine_world(mine_radius: f32) -> (World, PhysicsWorld, EntityId) {
        let mut world = World::new();
        let physics = PhysicsWorld::new();
        let mine = world.add_entity((
            RuntimePropTransform(Matrix4::from_translation(vec3(0.0, 0.0, 0.0))),
            dimensions(mine_radius),
        ));
        (world, physics, mine)
    }

    fn add_body(physics: &mut PhysicsWorld, entity_id: EntityId, position: cgmath::Vector3<f32>) {
        physics.add_kinematic(
            entity_id,
            position,
            Quaternion::new(1.0, 0.0, 0.0, 0.0),
            vec3(0.0, 0.0, 0.0),
            vec3(1.0, 1.0, 1.0),
            CollisionGroup::actor(),
            false,
        );
    }

    /// One step so the broad phase sees the fresh bodies.
    fn settle(physics: &mut PhysicsWorld) {
        let mut player =
            physics.create_player(vec3(0.0, 0.0, -50.0), EntityId::from_inner(9).unwrap());
        physics.update(vec3(0.0, 0.0, 0.0), &mut player);
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
        let (world, physics, mine) = mine_world(vec3(1.9, 0.0, 0.0), 12, DEFAULT_TRIGGER_RADIUS);
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
        let (world, physics, mine) = mine_world(vec3(0.5, 0.0, 0.0), 12, DEFAULT_TRIGGER_RADIUS);
        let mut script = PsiMine::new();

        let effect = update(&mut script, &world, &physics, mine, ARM_DELAY / 2);

        assert!(matches!(effect, Effect::NoEffect));
    }

    #[test]
    fn an_armed_mine_ignores_a_creature_out_of_reach() {
        let (world, physics, mine) = mine_world(vec3(3.0, 0.0, 0.0), 12, DEFAULT_TRIGGER_RADIUS);
        let mut script = PsiMine::new();

        let effect = update(&mut script, &world, &physics, mine, ARM_DELAY);

        assert!(matches!(effect, Effect::NoEffect));
    }

    /// The reach is the mine's own authored radius, not a constant: the same
    /// creature that is out of reach above trips a wider-bodied mine.
    #[test]
    fn the_reach_comes_from_the_mines_authored_dimensions() {
        let (world, physics, mine) = mine_world(vec3(3.0, 0.0, 0.0), 12, 3.0);
        let mut script = PsiMine::new();

        let effect = update(&mut script, &world, &physics, mine, ARM_DELAY);

        assert!(matches!(effect, Effect::SlayEntity { entity_id } if entity_id == mine));
    }

    #[test]
    fn a_dead_creature_does_not_trip_a_mine() {
        let (world, physics, mine) = mine_world(vec3(1.0, 0.0, 0.0), 0, DEFAULT_TRIGGER_RADIUS);
        let mut script = PsiMine::new();

        let effect = update(&mut script, &world, &physics, mine, ARM_DELAY);

        assert!(matches!(effect, Effect::NoEffect));
    }

    /// The levels are littered with authored corpse props: `PropCreature` with
    /// no AI and no hit points. A mine must not go off on the scenery.
    #[test]
    fn an_authored_corpse_does_not_trip_a_mine() {
        let (mut world, mut physics, mine) = empty_mine_world(DEFAULT_TRIGGER_RADIUS);
        let corpse = world.add_entity(PropCreature(0));
        add_body(&mut physics, corpse, vec3(1.0, 0.0, 0.0));
        settle(&mut physics);
        let mut script = PsiMine::new();

        let effect = update(&mut script, &world, &physics, mine, ARM_DELAY);

        assert!(matches!(effect, Effect::NoEffect));
    }

    #[test]
    fn an_untripped_mine_expires() {
        let (world, physics, mine) = mine_world(vec3(100.0, 0.0, 0.0), 12, DEFAULT_TRIGGER_RADIUS);
        let mut script = PsiMine::new();

        let effect = update(&mut script, &world, &physics, mine, LIFETIME);

        assert!(matches!(effect, Effect::DestroyEntity { entity_id } if entity_id == mine));
    }

    #[test]
    fn a_damaged_mine_detonates() {
        let (world, physics, mine) = mine_world(vec3(100.0, 0.0, 0.0), 12, DEFAULT_TRIGGER_RADIUS);
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
        let (world, physics, mine) = mine_world(vec3(100.0, 0.0, 0.0), 12, DEFAULT_TRIGGER_RADIUS);
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
