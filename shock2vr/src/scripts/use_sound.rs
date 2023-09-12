

use engine::audio::{AudioHandle};
use shipyard::{EntityId, World};


use crate::{physics::PhysicsWorld};

use super::{Effect, MessagePayload, Script};

pub struct UseSound {}
impl UseSound {
    pub fn new() -> UseSound {
        UseSound {}
    }
}
impl Script for UseSound {
    fn handle_message(
        &mut self,
        _entity_id: EntityId,
        _world: &World,
        _physics: &PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        match msg {
            MessagePayload::Frob => {
                println!("Playing - Turn on message");
                // TODO: Get sound from schema
                // let v_sound = world.borrow::<View<PropObjectSound>>().unwrap();
                // let maybe_trip_sound = v_sound.get(entity_id);
                let handle = AudioHandle::new();
                // // self.playing_sounds.push(handle.clone());
                // if let Ok(sound) = maybe_trip_sound {
                // println!("Playing - Sound: {}", sound.name);
                Effect::PlaySound {
                    handle,
                    name: "TELEPHON".to_owned(),
                }
                // } else {
                //     Effect::NoEffect
                // }
            }
            _ => Effect::NoEffect,
        }
    }
}
