use dark::properties::PropLog;
use engine::audio::AudioHandle;
use shipyard::{EntityId, Get, View, World};
use tracing::info;

use crate::physics::PhysicsWorld;

use super::{Effect, MessagePayload, Script, script_util::send_to_all_switch_links};

pub struct LogDiscScript {}
impl LogDiscScript {
    pub fn new() -> LogDiscScript {
        LogDiscScript {}
    }
}

fn format_number(num: u32) -> String {
    if num < 10 {
        format!("0{num}")
    } else {
        num.to_string()
    }
}

impl Script for LogDiscScript {
    fn handle_message(
        &mut self,
        entity_id: EntityId,
        world: &World,
        _physics: &PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        match msg {
            MessagePayload::Frob => {
                let v_sound = world.borrow::<View<PropLog>>().unwrap();
                let maybe_log_sound = v_sound.get(entity_id);
                let handle = AudioHandle::new();
                info!("frobbing a log... {:?}", maybe_log_sound);
                //self.playing_sounds.push(handle.clone());
                if let Ok(sound) = maybe_log_sound {
                    let email_effect = if sound.deck > 0 && sound.email > 0 {
                        Effect::PlaySound {
                            handle,
                            name: format!(
                                "LOG{}{}",
                                format_number(sound.deck),
                                format_number(sound.log)
                            ),
                        }
                    } else {
                        Effect::NoEffect
                    };

                    let destroy_self = Effect::DestroyEntity { entity_id };

                    let switchlink_effects = send_to_all_switch_links(
                        world,
                        entity_id,
                        MessagePayload::TurnOn { from: entity_id },
                    );
                    Effect::Combined {
                        effects: vec![email_effect, switchlink_effects, destroy_self],
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
