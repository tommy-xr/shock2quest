use engine::audio::AudioHandle;
use shipyard::{EntityId, World};

use crate::physics::PhysicsWorld;

use super::{
    Effect, MessagePayload, Script,
    script_util::{play_environmental_sound, send_to_all_switch_links},
};

pub struct BaseButton {}
impl BaseButton {
    pub fn new() -> BaseButton {
        BaseButton {}
    }

    pub fn is_locked(&self, entity_id: EntityId, world: &World) -> bool {
        super::script_util::is_entity_locked(world, entity_id)
    }

    /// The refusal a locked button gives instead of activating, or `None` when
    /// it is free to activate. Button subclasses gate their own action on this.
    pub fn locked_effect(&self, entity_id: EntityId, world: &World) -> Option<Effect> {
        self.is_locked(entity_id, world).then(|| Effect::PlaySound {
            handle: AudioHandle::new(),
            name: "hackfail".to_owned(),
        })
    }

    /// What every button does when it is pressed, apart from its own action:
    /// relay `TurnOn` to its switch links and play the activate sound. The
    /// button's *own* action is the caller's business - `BaseButton` notifies
    /// itself, subclasses run theirs directly.
    pub fn activate_effect(&self, entity_id: EntityId, world: &World) -> Effect {
        let switch_link_effect =
            send_to_all_switch_links(world, entity_id, MessagePayload::TurnOn { from: entity_id });
        let sound_effect =
            play_environmental_sound(world, entity_id, "activate", vec![], AudioHandle::new());
        Effect::combine(vec![switch_link_effect, sound_effect])
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
            MessagePayload::Frob => self.locked_effect(entity_id, world).unwrap_or_else(|| {
                // A plain button's own action is to notify itself, so proxy
                // scripts on the same entity see the press as a TurnOn.
                let notify_self = Effect::Send {
                    msg: super::Message {
                        payload: MessagePayload::TurnOn { from: entity_id },
                        to: entity_id,
                    },
                };
                Effect::combine(vec![self.activate_effect(entity_id, world), notify_self])
            }),

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
