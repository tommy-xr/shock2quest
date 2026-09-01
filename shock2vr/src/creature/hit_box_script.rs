use shipyard::{EntityId, World};

use crate::{
    physics::PhysicsWorld,
    scripts::{Effect, Message, MessagePayload, Script},
};

use super::HitBoxType;

/// A creature's per-joint damage proxy: it forwards what struck it to the
/// creature it belongs to, scaled by what that part is worth.
pub struct HitBoxScript {
    hit_box_type: HitBoxType,
    parent_entity_id: EntityId,
    #[allow(dead_code)]
    hit_box_joint_idx: u32,
}

impl HitBoxScript {
    pub fn new(
        hit_box_type: HitBoxType,
        parent_entity_id: EntityId,
        hit_box_joint_idx: u32,
    ) -> HitBoxScript {
        HitBoxScript {
            hit_box_type,
            parent_entity_id,
            hit_box_joint_idx,
        }
    }
}

impl Script for HitBoxScript {
    fn handle_message(
        &mut self,
        _entity_id: EntityId,
        _world: &World,
        _physics: &PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        match msg {
            MessagePayload::Damage { amount, impact } => {
                // What this part is worth. A blow on the creature's own weapon
                // is worth nothing, and is absorbed here rather than reaching
                // the creature as a zero that still reads as being hit.
                let multiplier = self.hit_box_type.damage_multiplier();
                if multiplier <= 0.0 {
                    return Effect::NoEffect;
                }
                Effect::Send {
                    msg: Message {
                        to: self.parent_entity_id,
                        // Forward to the owning creature, stamping which
                        // skeleton joint was struck so a death ragdoll can
                        // react at the right limb.
                        payload: MessagePayload::Damage {
                            amount: *amount * multiplier,
                            impact: impact.map(|i| crate::scripts::DamageImpact {
                                bone: Some(self.hit_box_joint_idx),
                                ..i
                            }),
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
    use crate::physics::PhysicsWorld;
    use crate::scripts::DamageImpact;
    use cgmath::vec3;

    fn blow(script: &mut HitBoxScript, amount: f32) -> Option<f32> {
        let world = World::new();
        let physics = PhysicsWorld::new();
        let effect = script.handle_message(
            EntityId::from_inner(1).unwrap(),
            &world,
            &physics,
            &MessagePayload::Damage {
                amount,
                impact: Some(DamageImpact {
                    direction: vec3(0.0, 0.0, 1.0),
                    point: vec3(0.0, 0.0, 0.0),
                    bone: None,
                }),
            },
        );
        match effect {
            Effect::Send { msg } => match msg.payload {
                MessagePayload::Damage { amount, impact } => {
                    // The forwarded blow always names the joint it struck.
                    assert_eq!(impact.and_then(|impact| impact.bone), Some(7));
                    Some(amount)
                }
                _ => None,
            },
            _ => None,
        }
    }

    fn script(hit_box_type: HitBoxType) -> HitBoxScript {
        HitBoxScript::new(hit_box_type, EntityId::from_inner(2).unwrap(), 7)
    }

    /// Where a blow lands is worth something: this is the whole point of
    /// aiming at limbs.
    ///
    /// Negative-first: forwarding the authored amount unchanged makes every
    /// part of a creature worth the same.
    #[test]
    fn a_blow_is_worth_what_the_part_it_struck_is_worth() {
        assert_eq!(blow(&mut script(HitBoxType::Head), 10.0), Some(12.5));
        assert_eq!(blow(&mut script(HitBoxType::Body), 10.0), Some(10.0));
        assert_eq!(blow(&mut script(HitBoxType::Limb), 10.0), Some(7.5));
        assert_eq!(blow(&mut script(HitBoxType::Extremity), 10.0), Some(5.0));
    }

    /// A creature's own weapon is not the creature: a blow that lands on the
    /// pipe it is holding is absorbed there, rather than reaching it as a zero
    /// that still reads as a hit.
    #[test]
    fn a_blow_on_the_weapon_it_holds_reaches_nothing() {
        assert_eq!(blow(&mut script(HitBoxType::NoDamage), 10.0), None);
    }
}
