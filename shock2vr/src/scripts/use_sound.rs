use engine::audio::AudioHandle;
use engine::script_log;
use shipyard::{EntityId, World};

use crate::physics::PhysicsWorld;

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
                script_log!(DEBUG, "Playing turn on sound for entity");
                // TODO: Get sound from schema
                // let v_sound = world.borrow::<View<PropObjectSound>>().unwrap();
                // let maybe_trip_sound = v_sound.get(entity_id);
                let handle = AudioHandle::new();
                // // self.playing_sounds.push(handle.clone());
                // if let Ok(sound) = maybe_trip_sound {
                //     script_log!(debug, "Playing sound: {}", sound.name);
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
