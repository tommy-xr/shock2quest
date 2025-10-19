use dark::properties::{PropConsumeType, PropSymName};
use engine::audio::AudioHandle;
use shipyard::{EntityId, Get, View, World};

use crate::{physics::PhysicsWorld, scripts::script_util::play_environmental_sound};

use super::{script_util::send_to_all_switch_links_and_self, Effect, MessagePayload, Script};

pub struct ObjConsumeButton;
impl ObjConsumeButton {
    pub fn new() -> ObjConsumeButton {
        ObjConsumeButton
    }
}
impl Script for ObjConsumeButton {
    fn handle_message(
        &mut self,
        entity_id: EntityId,
        world: &World,
        _physics: &PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        match msg {
            MessagePayload::ProvideForConsumption { entity } => {
                if can_consume_entity(world, entity_id, *entity) {
                    let switch_link_efect = send_to_all_switch_links_and_self(
                        world,
                        entity_id,
                        MessagePayload::TurnOn { from: entity_id },
                    );
                    let destroy_consumee = Effect::DestroyEntity { entity_id: *entity };

                    let sound_effect = play_environmental_sound(
                        world,
                        entity_id,
                        "activate",
                        vec![],
                        AudioHandle::new(),
                    );
                    Effect::combine(vec![switch_link_efect, destroy_consumee, sound_effect])
                } else {
                    Effect::NoEffect
                }
            }
            _ => Effect::NoEffect,
        }
    }
}

fn can_consume_entity(world: &World, self_id: EntityId, entity_to_consume_id: EntityId) -> bool {
    let v_consume_type = world.borrow::<View<PropConsumeType>>().unwrap();
    let v_sym_name = world.borrow::<View<PropSymName>>().unwrap();

    if let (Ok(consume_type), Ok(sym_name)) = (
        v_consume_type.get(self_id),
        v_sym_name.get(entity_to_consume_id),
    ) {
        return consume_type.0.eq_ignore_ascii_case(&sym_name.0);
    }
    false
}
