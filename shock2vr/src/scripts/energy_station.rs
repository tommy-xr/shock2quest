use dark::{
    EnvSoundQuery,
    properties::{PropClassTag, PropPosition},
};
use engine::audio::AudioHandle;
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
            // Flat presentation: frobbing the station sends Recharge to every
            // item the player carries at once (there is no
            // hold-one-item-near-the-station step - that is VR-only, handled by
            // Hover/Collided above). Each item self-handles Recharge, so only
            // responders react - today that is the dead power cell. Energy-weapon
            // recharge would be a separate follow-up, once those gain a Recharge
            // handler; the mechanism here is already general.
            MessagePayload::Frob => do_recharge_all(world, entity_id),
            _ => Effect::NoEffect,
        }
    }
}

fn do_recharge_all(world: &World, entity_id: EntityId) -> Effect {
    let mut effects: Vec<Effect> = super::script_util::player_carried_items(world)
        .into_iter()
        .map(|item| Effect::Send {
            msg: Message {
                to: item,
                payload: MessagePayload::Recharge,
            },
        })
        .collect();
    effects.push(activate_sound(world, entity_id));
    Effect::combine(effects)
}

fn activate_sound(world: &World, entity_id: EntityId) -> Effect {
    let v_pos = world.borrow::<View<PropPosition>>().unwrap();
    let v_class_tag = world.borrow::<View<PropClassTag>>().unwrap();
    let mut class_tags = v_class_tag
        .get(entity_id)
        .map(|p| p.class_tags())
        .unwrap_or(vec![]);
    let pos = v_pos.get(entity_id).unwrap();
    let mut query = vec![("event", "activate")];
    query.append(&mut class_tags);
    Effect::PlayEnvironmentalSound {
        audio_handle: AudioHandle::new(),
        query: EnvSoundQuery::from_tag_values(query),
        position: pos.position,
    }
}

fn do_recharge(world: &World, entity_id: EntityId, with: &EntityId) -> Effect {
    let recharge_effect = Effect::Send {
        msg: Message {
            to: *with,
            payload: MessagePayload::Recharge,
        },
    };
    Effect::combine(vec![recharge_effect, activate_sound(world, entity_id)])
}
