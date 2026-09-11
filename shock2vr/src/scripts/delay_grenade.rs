//! Script `DelayGrenade`: the grenade lobbed by the grenade hybrid
//! (`OG-Grenade`, -176) through its `AIProjectile -> Grunt Grenade` (-1600)
//! link.
//!
//! Unlike the bullet-style monster projectiles, the grenade is authored
//! `BOUNCE | FULL_COLLISION_SOUND` - it does NOT inherit `SLAY_ON_IMPACT` from
//! its parent `Monster Projectiles` (-672) - so nothing in the generic
//! collision path ends it. It goes off when something bumps into it, after a
//! short arming delay so it cannot detonate on the thrower as it leaves the
//! hand.
//!
//! The blast is authored data, not code: slaying the grenade spawns its
//! `Corpse -> HE Explosion` (-3933), whose stim source is resolved like any
//! other explosion.

use serde::{Deserialize, Serialize};
use shipyard::{EntityId, World};
use std::time::Duration;

use crate::{physics::PhysicsWorld, time::Time};

use super::{Effect, MessagePayload, Script, ScriptRestoreContext, ScriptState, ScriptStateError};

/// How long after the throw the grenade starts reacting to contact, so it
/// cannot go off against the thrower's own body on the frame it spawns. A
/// point-blank throw reaches the player around 100 ms, so this stays well
/// under that: a grenade that arrives before arming simply lands and waits to
/// be bumped, which is the same script behaviour a heartbeat later.
const ARM_DELAY: Duration = Duration::from_millis(50);

/// How long a grenade that nothing ever bumps lasts before it quietly
/// expires. Only entity contacts raise `Collided` - world geometry carries no
/// entity id - so a grenade that lands on the floor and is never walked into
/// would otherwise live forever, and one thrown off a ledge falls out of the
/// level still live. Not an authored value: it exists so misses cannot
/// accumulate.
const LIFETIME: Duration = Duration::from_secs(30);

const SCRIPT_STATE_KEY: &str = "shock2vr.delay_grenade";

#[derive(Serialize, Deserialize)]
struct DelayGrenadeState {
    age_secs: f32,
}

pub struct DelayGrenade {
    age: Duration,
}

impl DelayGrenade {
    pub fn new() -> DelayGrenade {
        DelayGrenade {
            age: Duration::ZERO,
        }
    }
}

impl Script for DelayGrenade {
    fn update(
        &mut self,
        entity_id: EntityId,
        _world: &World,
        _physics: &PhysicsWorld,
        time: &Time,
    ) -> Effect {
        self.age += time.elapsed;

        if self.age >= LIFETIME {
            // Expiry is not a detonation: an untripped grenade goes away
            // rather than blasting an empty room long after the fight.
            Effect::DestroyEntity { entity_id }
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
        match msg {
            MessagePayload::Collided { .. } if self.age >= ARM_DELAY => {
                Effect::SlayEntity { entity_id }
            }
            _ => Effect::NoEffect,
        }
    }

    fn script_state_key(&self) -> Option<&'static str> {
        Some(SCRIPT_STATE_KEY)
    }

    fn save_state(&self) -> Result<ScriptState, ScriptStateError> {
        ScriptState::encode(
            1,
            &DelayGrenadeState {
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
        let restored: DelayGrenadeState = state.decode(1, SCRIPT_STATE_KEY)?;
        self.age = Duration::from_secs_f32(restored.age_secs.max(0.0));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn tick(script: &mut DelayGrenade, grenade: EntityId, elapsed: Duration) -> Effect {
        script.update(
            grenade,
            &World::new(),
            &PhysicsWorld::new(),
            &Time {
                elapsed,
                ..Time::default()
            },
        )
    }

    fn collide(script: &mut DelayGrenade, grenade: EntityId) -> Effect {
        script.handle_message(
            grenade,
            &World::new(),
            &PhysicsWorld::new(),
            &MessagePayload::Collided {
                with: EntityId::dead(),
                contact: None,
            },
        )
    }

    /// The point of the script: a grenade that bumps something goes off, so
    /// the authored `Corpse -> HE Explosion` gets a chance to spawn.
    #[test]
    fn an_armed_grenade_detonates_when_something_bumps_it() {
        let mut world = World::new();
        let grenade = world.add_entity(());
        let mut script = DelayGrenade::new();
        tick(&mut script, grenade, ARM_DELAY);
        assert!(matches!(
            collide(&mut script, grenade),
            Effect::SlayEntity { entity_id } if entity_id == grenade
        ));
    }

    /// The delay in the name: a contact on the throw frame must not blow the
    /// grenade up against the thrower.
    #[test]
    fn a_grenade_still_in_the_hand_does_not_detonate() {
        let mut world = World::new();
        let grenade = world.add_entity(());
        let mut script = DelayGrenade::new();
        assert!(matches!(collide(&mut script, grenade), Effect::NoEffect));
        tick(&mut script, grenade, ARM_DELAY / 2);
        assert!(matches!(collide(&mut script, grenade), Effect::NoEffect));
    }

    /// A grenade nothing ever touches goes away instead of drifting through
    /// the level - and goes away quietly, without a blast.
    #[test]
    fn an_untripped_grenade_expires_without_detonating() {
        let mut world = World::new();
        let grenade = world.add_entity(());
        let mut script = DelayGrenade::new();
        assert!(matches!(
            tick(&mut script, grenade, LIFETIME - ARM_DELAY),
            Effect::NoEffect
        ));
        assert!(matches!(
            tick(&mut script, grenade, ARM_DELAY),
            Effect::DestroyEntity { entity_id } if entity_id == grenade
        ));
    }

    /// The fuse survives a save/load, so a grenade in flight when the game is
    /// saved neither re-arms from scratch nor expires early.
    #[test]
    fn the_fuse_survives_a_save_and_restore() {
        let mut script = DelayGrenade::new();
        let grenade = World::new().add_entity(());
        tick(&mut script, grenade, ARM_DELAY);
        let state = script.save_state().expect("save");
        let mut restored = DelayGrenade::new();
        let remap = HashMap::new();
        restored
            .restore_state(
                &state,
                &ScriptRestoreContext {
                    entity_id_map: &remap,
                },
            )
            .expect("restore");
        assert!(matches!(
            collide(&mut restored, grenade),
            Effect::SlayEntity { entity_id } if entity_id == grenade
        ));
    }
}
