use dark::properties::PropLocked;
use engine::audio::AudioHandle;
use shipyard::{EntityId, Get, UniqueView, View, World};

use crate::{physics::PhysicsWorld, quest_info::QuestInfo};

use super::{
    Effect, MessagePayload, Script,
    script_util::{
        play_environmental_sound, send_to_all_switch_links, send_to_all_switch_links_and_self,
    },
};

pub struct BaseButton {}
impl BaseButton {
    pub fn new() -> BaseButton {
        BaseButton {}
    }

    pub fn is_locked(&self, entity_id: EntityId, world: &World) -> bool {
        let v_prop_locked = world.borrow::<shipyard::View<PropLocked>>().unwrap();
        let v_prop_key_dst = world
            .borrow::<View<dark::properties::PropKeyDst>>()
            .unwrap();

        let _v_prop_key_src = world
            .borrow::<View<dark::properties::PropKeySrc>>()
            .unwrap();

        let quest_bits = world.borrow::<UniqueView<QuestInfo>>().unwrap();

        let maybe_prop_locked = v_prop_locked.get(entity_id);

        if let Ok(prop_locked) = maybe_prop_locked {
            // We're locked... check if we have a key that can open it!
            if prop_locked.0 {
                let maybe_prop_key_dst = v_prop_key_dst.get(entity_id);
                if let Ok(key_dst) = maybe_prop_key_dst {
                    let is_unlocked = quest_bits.can_unlock(&key_dst.0);
                    !is_unlocked
                } else {
                    true
                }
            } else {
                false
            }
        } else {
            false
        }
    }
}
impl Script for BaseButton {
    fn handle_message(
        &mut self,
        entity_id: EntityId,
        world: &World,
        _physics: &PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        match msg {
            MessagePayload::Frob => {
                if self.is_locked(entity_id, world) {
                    Effect::PlaySound {
                        handle: AudioHandle::new(),
                        name: "hackfail".to_owned(),
                    }
                } else {
                    let switch_link_effect = send_to_all_switch_links_and_self(
                        world,
                        entity_id,
                        MessagePayload::TurnOn { from: entity_id },
                    );
                    let sound_effect = play_environmental_sound(
                        world,
                        entity_id,
                        "activate",
                        vec![],
                        AudioHandle::new(),
                    );
                    Effect::combine(vec![switch_link_effect, sound_effect])
                }
            }

            // In some places (like the computer for the engine room in eng1), invisible buttons are used as proxies -
            // there will be an actual button that sends a 'TurnOn' message to an invisible button. Not sure why
            // this pattern is used.
            MessagePayload::TurnOn { from: _ } => send_to_all_switch_links(
                world,
                entity_id,
                MessagePayload::TurnOn { from: entity_id },
            ),

            _ => Effect::NoEffect,
        }
    }
}
