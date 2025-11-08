use dark::properties::PropLog;
use engine::audio::AudioHandle;
use shipyard::{EntityId, Get, View, World};

use crate::physics::PhysicsWorld;

use super::{Effect, MessagePayload, Script, script_util::send_to_all_switch_links};

pub struct TrapEmail {}
impl TrapEmail {
    pub fn new() -> TrapEmail {
        TrapEmail {}
    }
}

impl Script for TrapEmail {
    fn handle_message(
        &mut self,
        entity_id: EntityId,
        world: &World,
        _physics: &PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        match msg {
            MessagePayload::TurnOn { from: _ } => {
                let v_sound = world.borrow::<View<PropLog>>().unwrap();
                let maybe_trip_sound = v_sound.get(entity_id);
                let _handle = AudioHandle::new();
                //self.playing_sounds.push(handle.clone());
                if let Ok(sound) = maybe_trip_sound {
                    let email_effect = if sound.deck > 0 && sound.email > 0 {
                        Effect::PlayEmail {
                            deck: sound.deck,
                            email: sound.email,
                            force: false,
                        }
                    } else {
                        Effect::NoEffect
                    };

                    let switchlink_effects = send_to_all_switch_links(
                        world,
                        entity_id,
                        MessagePayload::TurnOn { from: entity_id },
                    );
                    Effect::Combined {
                        effects: vec![
                            email_effect,
                            switchlink_effects,
                            Effect::DestroyEntity { entity_id },
                        ],
                    }
                } else {
                    Effect::NoEffect
                }
            }
            // Does turn off need to be done for email?
            _ => Effect::NoEffect,
        }
    }
}
