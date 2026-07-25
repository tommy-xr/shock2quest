use dark::properties::PropHitPoints;
use shipyard::{EntityId, Get, View, World};

use crate::physics::PhysicsWorld;

use super::{Effect, MessagePayload, Script};

/// The player's own damage handler.
///
/// The player is an ordinary damageable entity - it carries `P$HitPoints` /
/// `P$MAX_HP` from `The Player` archetype and the receptrons it inherits from
/// `Human Vulnerability` - but nothing was translating a `Damage` message into
/// an actual hit-point loss, so every attack aimed at the player was silently
/// dropped. This script closes that gap the same way `AnimatedMonsterAI` does
/// for creatures, so any damage source (AI melee, and later projectiles and
/// explosions) reaches the player through the one shared message path.
pub struct PlayerScript;

impl Script for PlayerScript {
    fn handle_message(
        &mut self,
        entity_id: EntityId,
        world: &World,
        _physics: &PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        match msg {
            MessagePayload::Damage { amount, .. } => {
                // A dead player takes no further hits (death handling itself
                // is not implemented yet - see #561 follow-ups).
                if is_dead(world, entity_id) {
                    return Effect::NoEffect;
                }
                Effect::AdjustHitPoints {
                    entity_id,
                    delta: -(amount.round() as i32),
                }
            }
            _ => Effect::NoEffect,
        }
    }
}

fn is_dead(world: &World, entity_id: EntityId) -> bool {
    world
        .borrow::<View<PropHitPoints>>()
        .ok()
        .and_then(|v| v.get(entity_id).ok().map(|hp| hp.hit_points <= 0))
        .unwrap_or(false)
}
