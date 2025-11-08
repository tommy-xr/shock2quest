use dark::properties::PropDelayTime;
use shipyard::{EntityId, Get, View, World};
use tracing::info;

use crate::{physics::PhysicsWorld, scripts::script_util::template_id_string, time::Time};

use super::{Effect, MessagePayload, Script, script_util::send_to_all_switch_links};

pub struct TrapDelay {
    delay_time_in_seconds: f32,
    messages: Vec<(MessagePayload, f32)>,
}
impl TrapDelay {
    pub fn new() -> TrapDelay {
        TrapDelay {
            delay_time_in_seconds: 1.0,
            messages: Vec::new(),
        }
    }
}
impl Script for TrapDelay {
    fn initialize(&mut self, entity_id: EntityId, world: &World) -> Effect {
        let v_delay_time = world.borrow::<View<PropDelayTime>>().unwrap();
        let delay_time = if let Ok(v) = v_delay_time.get(entity_id) {
            v.delay.as_secs_f32()
        } else {
            1.0
        };
        self.delay_time_in_seconds = delay_time;
        Effect::NoEffect
    }

    fn handle_message(
        &mut self,
        entity_id: EntityId,
        world: &World,
        _physics: &PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        info!(
            "{}: receiving message to be delayed: {:?}",
            template_id_string(world, &entity_id),
            msg
        );
        self.messages
            .push((msg.clone(), self.delay_time_in_seconds));
        Effect::NoEffect
    }

    fn update(
        &mut self,
        entity_id: EntityId,
        world: &World,
        _physics: &PhysicsWorld,
        time: &Time,
    ) -> Effect {
        let delta_time = time.elapsed.as_secs_f32();

        let working_messages = self.messages.clone();
        let (remaining_messages, messages_to_dispatch): (
            Vec<(MessagePayload, f32)>,
            Vec<(MessagePayload, f32)>,
        ) = working_messages
            .into_iter()
            .map(|(msg, time)| {
                let new_time = time - delta_time;
                (msg, new_time)
            })
            .partition(|(_msg, time)| *time >= 0.0);

        self.messages = remaining_messages;
        let mut eff = Vec::new();
        for (msg, _) in messages_to_dispatch {
            let send_message = send_to_all_switch_links(world, entity_id, msg);
            eff.push(send_message);
        }
        Effect::Combined { effects: eff }
    }
}
