use dark::{
    EnvSoundQuery,
    properties::{PropClassTag, PropPosition},
};
use engine::audio::AudioHandle;
use engine::script_log;
use shipyard::{EntityId, Get, View, World};

use crate::physics::PhysicsWorld;

use super::{Effect, Message, MessagePayload, Script};

pub struct EnergyStation;
impl EnergyStation {
    pub fn new() -> EnergyStation {
        EnergyStation
    }
}

impl Script for EnergyStation {
    fn handle_message(
        &mut self,
        entity_id: EntityId,
        world: &World,
        _physics: &PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        match msg {
            MessagePayload::Hover {
                held_entity_id,
                world_position: _,
                is_triggered: _,
                is_grabbing: _,
                hand: _,
            } => {
                if let Some(with) = held_entity_id {
                    do_recharge(world, entity_id, with)
                } else {
                    Effect::NoEffect
                }
            }
            MessagePayload::Collided { with } => do_recharge(world, entity_id, with),
            _ => Effect::NoEffect,
        }
    }
}

fn do_recharge(world: &World, entity_id: EntityId, with: &EntityId) -> Effect {
    let v_pos = world.borrow::<View<PropPosition>>().unwrap();
    let v_class_tag = world.borrow::<View<PropClassTag>>().unwrap();
    let mut class_tags = v_class_tag
        .get(entity_id)
        .map(|p| p.class_tags())
        .unwrap_or(vec![]);

    // log_property::<PropClassTag>(world);
    // panic!();
    //log_property::<PropDeviceTag>(world);
    let pos = v_pos.get(entity_id).unwrap();
    let recharge_effect = Effect::Send {
        msg: Message {
            to: *with,
            payload: MessagePayload::Recharge,
        },
    };

    let mut query = vec![("event", "activate")];
    query.append(&mut class_tags);
    script_log!(DEBUG, "Energy station activate query: {:?}", query);
    let sound_effect = Effect::PlayEnvironmentalSound {
        audio_handle: AudioHandle::new(),
        query: EnvSoundQuery::from_tag_values(query),
        position: pos.position,
    };

    Effect::combine(vec![recharge_effect, sound_effect])
}
