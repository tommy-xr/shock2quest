use dark::properties::PropHitPoints;
use shipyard::{EntityId, Get, View, World};

use crate::physics::PhysicsWorld;

use super::{Effect, Message, MessagePayload, Script};

// Script to handle simple health behavior for non-creature entities that carry
// a `PropHitPoints` (e.g. breakable crates, canisters, computer panels).
// Creatures route damage through their AI / hitbox scripts instead.
pub struct InternalSimpleHealth {}

impl InternalSimpleHealth {
    pub fn new() -> InternalSimpleHealth {
        InternalSimpleHealth {}
    }
}

impl Script for InternalSimpleHealth {
    fn handle_message(
        &mut self,
        entity_id: EntityId,
        world: &World,
        _physics: &PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        match msg {
            MessagePayload::Damage { amount, impact: _ } => {
                // Damage amounts are authored as floats but hit points are an
                // integer pool; round to match the AI damage path
                // (animated_monster_ai).
                let damage = amount.round() as i32;

                let remaining_after = {
                    let v_hit_points = world.borrow::<View<PropHitPoints>>().unwrap();
                    v_hit_points
                        .get(entity_id)
                        .ok()
                        .map(|hp| hp.hit_points - damage)
                };

                match remaining_after {
                    // Still alive: decrement the pool and keep the entity around.
                    Some(remaining) if remaining > 0 => Effect::AdjustHitPoints {
                        entity_id,
                        delta: -damage,
                    },
                    // Route death through the script queue before teardown so
                    // authored death triggers (TriggerDestroy) can react.
                    _ => Effect::Send {
                        msg: Message {
                            to: entity_id,
                            payload: MessagePayload::Slay,
                        },
                    },
                }
            }
            _ => Effect::NoEffect,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn world_with_hit_points(hit_points: i32) -> (World, EntityId) {
        let mut world = World::new();
        let entity_id = world.add_entity((PropHitPoints { hit_points },));
        (world, entity_id)
    }

    fn damage(world: &World, entity_id: EntityId, amount: f32) -> Effect {
        let physics = PhysicsWorld::new();
        let mut script = InternalSimpleHealth::new();
        script.handle_message(
            entity_id,
            world,
            &physics,
            &MessagePayload::Damage {
                amount,
                impact: None,
            },
        )
    }

    #[test]
    fn non_lethal_damage_decrements_hit_points_without_slaying() {
        let (world, entity_id) = world_with_hit_points(3);

        match damage(&world, entity_id, 1.0) {
            Effect::AdjustHitPoints {
                entity_id: id,
                delta,
            } => {
                assert_eq!(id, entity_id);
                assert_eq!(delta, -1);
            }
            other => panic!("expected AdjustHitPoints, got {:?}", other),
        }
    }

    fn assert_requests_slay(effect: Effect, entity_id: EntityId) {
        match effect {
            Effect::Send { msg } => {
                assert_eq!(msg.to, entity_id);
                assert!(matches!(msg.payload, MessagePayload::Slay));
            }
            other => panic!("expected queued Slay message, got {other:?}"),
        }
    }

    #[test]
    fn damage_meeting_hit_points_requests_slay() {
        let (world, entity_id) = world_with_hit_points(1);

        assert_requests_slay(damage(&world, entity_id, 1.0), entity_id);
    }

    #[test]
    fn damage_exceeding_hit_points_requests_slay() {
        let (world, entity_id) = world_with_hit_points(5);

        assert_requests_slay(damage(&world, entity_id, 6.0), entity_id);
    }

    #[test]
    fn entity_without_hit_points_requests_slay() {
        use dark::properties::PropMaxHitPoints;
        let mut world = World::new();
        // An entity that has *some* component but no PropHitPoints pool.
        let entity_id = world.add_entity((PropMaxHitPoints { hit_points: 10 },));

        assert_requests_slay(damage(&world, entity_id, 1.0), entity_id);
    }
}
