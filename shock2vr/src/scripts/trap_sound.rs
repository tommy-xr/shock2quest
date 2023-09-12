use dark::properties::PropObjectSound;
use engine::audio::AudioHandle;
use shipyard::{EntityId, Get, View, World};

use crate::physics::PhysicsWorld;

use super::{Effect, MessagePayload, Script};

pub struct TrapSound {
    playing_sounds: Vec<AudioHandle>,
}
impl TrapSound {
    pub fn new() -> TrapSound {
        TrapSound {
            playing_sounds: Vec::new(),
        }
    }
}
impl Script for TrapSound {
    fn handle_message(
        &mut self,
        entity_id: EntityId,
        world: &World,
        _physics: &PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        match msg {
            MessagePayload::TurnOn { from: _ } => {
                let v_sound = world.borrow::<View<PropObjectSound>>().unwrap();
                let maybe_trip_sound = v_sound.get(entity_id);
                let handle = AudioHandle::new();
                self.playing_sounds.push(handle.clone());
                if let Ok(sound) = maybe_trip_sound {
                    Effect::PlaySound {
                        handle,
                        name: sound.name.to_owned(),
                    }
                } else {
                    Effect::NoEffect
                }
            }
            MessagePayload::TurnOff { from: _ } => {
                let mut eff = Vec::new();
                for handle in &self.playing_sounds {
                    eff.push(Effect::StopSound {
                        handle: handle.clone(),
                    });
                }
                self.playing_sounds.clear();
                Effect::Combined { effects: eff }
            }
            _ => Effect::NoEffect,
        }
    }
}
