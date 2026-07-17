use shipyard::{EntityId, World};

use crate::{
    physics::PhysicsWorld,
    scripts::{Effect, Message, MessagePayload, Script},
};

use super::HitBoxType;

// Script to handle simple health behavior
pub struct HitBoxScript {
    #[allow(dead_code)]
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
            MessagePayload::Damage { amount, impact } => Effect::Send {
                msg: Message {
                    to: self.parent_entity_id,
                    // Forward to the owning creature, stamping which skeleton
                    // joint was struck so a death ragdoll can react at the
                    // right limb.
                    payload: MessagePayload::Damage {
                        amount: *amount,
                        impact: impact.map(|i| crate::scripts::DamageImpact {
                            bone: Some(self.hit_box_joint_idx),
                            ..i
                        }),
                    },
                },
            },
            _ => Effect::NoEffect,
        }
    }
}
