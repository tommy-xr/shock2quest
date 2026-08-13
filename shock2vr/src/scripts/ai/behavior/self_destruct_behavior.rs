use cgmath::Deg;
use dark::motion::MotionQueryItem;

use shipyard::*;

use crate::{
    physics::PhysicsWorld,
    scripts::{
        Effect,
        ai::steering::{ChasePlayerSteeringStrategy, Steering, SteeringOutput, SteeringStrategy},
    },
    time::Time,
};

use super::{Behavior, NextBehavior};

/// How long the droid holds on the player before it goes off. Long enough for
/// the attack bark to register, and for a reacting player to shoot it - a
/// droid killed mid-fuse still detonates (its `Corpse` link is the blast
/// either way), so the reward for reacting is putting it down before it
/// closes, not after. Like every other behavior's state, the fuse doesn't
/// survive save/load: a restored droid re-acquires and lights a fresh one.
const FUSE_SECONDS: f32 = 1.0;

/// A protocol droid's attack is its own destruction: the template carries no
/// `L$Weapon` archetype to resolve a melee blow, but does link a `Corpse`
/// Incendiary Explosion. Reaching the player starts a short fuse; when it
/// expires the droid slays itself and that authored explosion - stimming
/// everything in its radius - is the damage.
pub struct SelfDestructBehavior {
    fuse_remaining: f32,
}

impl SelfDestructBehavior {
    pub fn new() -> SelfDestructBehavior {
        SelfDestructBehavior {
            fuse_remaining: FUSE_SECONDS,
        }
    }
}

impl Behavior for SelfDestructBehavior {
    fn name(&self) -> &'static str {
        "SelfDestruct"
    }

    /// The melee schema: this IS the droid's melee attack, and querying it
    /// under those tags is also what makes the AI play the `comattack` bark
    /// on the way in (see `is_attack_animation`).
    fn animation(self: &SelfDestructBehavior) -> Vec<MotionQueryItem> {
        vec![
            MotionQueryItem::new("meleecombat"),
            MotionQueryItem::new("attack").optional(),
            MotionQueryItem::new("direction").optional(),
        ]
    }

    fn steer(
        &mut self,
        current_heading: Deg<f32>,
        world: &World,
        physics: &PhysicsWorld,
        entity_id: EntityId,
        time: &Time,
    ) -> Option<(SteeringOutput, Effect)> {
        // Burning past zero is the detonation. The slay removes the entity
        // (and this script with it), so it can only happen once.
        let was_burning = self.fuse_remaining > 0.0;
        self.fuse_remaining -= time.elapsed.as_secs_f32();
        let detonation_effect = if was_burning && self.fuse_remaining <= 0.0 {
            Effect::SlayEntity { entity_id }
        } else {
            Effect::NoEffect
        };

        // Keep closing on the player while the fuse burns, like a melee swing
        let (steering_output, steering_effect) = ChasePlayerSteeringStrategy
            .steer(current_heading, world, physics, entity_id, time)
            .unwrap_or((Steering::from_current(current_heading), Effect::NoEffect));

        Some((
            steering_output,
            Effect::combine(vec![detonation_effect, steering_effect]),
        ))
    }

    fn preempted_by_alertness(&self) -> bool {
        // Escalating from Moderate to High mid-windup would otherwise build a
        // fresh behavior and restart the fuse (and re-play the bark).
        false
    }

    fn next_behavior(
        &mut self,
        _world: &World,
        _physics: &PhysicsWorld,
        _entity_id: EntityId,
    ) -> NextBehavior {
        // Once lit, the fuse burns: a droid that has committed doesn't
        // disengage to chase a player backing out of range.
        NextBehavior::Stay
    }
}
