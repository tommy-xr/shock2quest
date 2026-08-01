use dark::properties::{
    ObjectState, PropConsumeType, PropModelName, PropObjState, PropSymName, PropTweqModelConfig,
};
use engine::audio::AudioHandle;
use shipyard::{EntityId, Get, View, World};

use crate::{physics::PhysicsWorld, scripts::script_util::play_environmental_sound};

use super::{Effect, MessagePayload, Script, script_util::send_to_all_switch_links_and_self};

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
            // VR: the player holds a specific item against the receptor.
            MessagePayload::ProvideForConsumption { entity } => {
                if can_consume_entity(world, entity_id, *entity) {
                    consume(world, entity_id, *entity)
                } else {
                    Effect::NoEffect
                }
            }
            // Flat: frobbing the receptor consumes the first matching item the
            // player carries (no hold-item-near-the-receptor step - that is
            // VR-only, handled above).
            MessagePayload::Frob => {
                match super::script_util::player_carried_items(world)
                    .into_iter()
                    .find(|item| can_consume_entity(world, entity_id, *item))
                {
                    Some(item) => consume(world, entity_id, item),
                    None => Effect::NoEffect,
                }
            }
            _ => Effect::NoEffect,
        }
    }
}

fn consume(world: &World, entity_id: EntityId, entity_to_consume: EntityId) -> Effect {
    let switch_link_efect = send_to_all_switch_links_and_self(
        world,
        entity_id,
        MessagePayload::TurnOn { from: entity_id },
    );
    let destroy_consumee = Effect::DestroyEntity {
        entity_id: entity_to_consume,
    };
    let sound_effect =
        play_environmental_sound(world, entity_id, "activate", vec![], AudioHandle::new());
    Effect::combine(vec![switch_link_efect, destroy_consumee, sound_effect])
}

fn can_consume_entity(world: &World, self_id: EntityId, entity_to_consume_id: EntityId) -> bool {
    let already_used = {
        let models = world.borrow::<View<PropModelName>>().unwrap();
        let configs = world.borrow::<View<PropTweqModelConfig>>().unwrap();
        match (models.get(self_id), configs.get(self_id)) {
            (Ok(model), Ok(config)) => model_is_final(&model.0, &config.model_names),
            _ => false,
        }
    };
    if already_used {
        return false;
    }

    let unresearched = world
        .borrow::<View<PropObjState>>()
        .unwrap()
        .get(entity_to_consume_id)
        .map(|state| state.0 == ObjectState::Unresearched)
        .unwrap_or(false);
    if unresearched {
        return false;
    }

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

fn model_is_final(current: &str, frames: &[String]) -> bool {
    frames
        .last()
        .map(|last| current.eq_ignore_ascii_case(last))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use dark::properties::{PropConsumeType, PropModelName, PropObjState, PropSymName};
    use shipyard::World;

    use super::*;

    #[test]
    fn unresearched_candidate_is_rejected() {
        let mut world = World::new();
        let receptor = world.add_entity(PropConsumeType("AAToxin".to_owned()));
        let toxin = world.add_entity((
            PropSymName("AAToxin".to_owned()),
            PropObjState(ObjectState::Unresearched),
        ));
        assert!(!can_consume_entity(&world, receptor, toxin));

        world.add_component(toxin, PropObjState(ObjectState::Normal));
        assert!(can_consume_entity(&world, receptor, toxin));
    }

    #[test]
    fn final_tweq_model_marks_receptor_used() {
        let frames = vec!["air_reof".to_owned(), "air_re".to_owned()];
        assert!(!model_is_final("air_reof", &frames));
        assert!(model_is_final("AIR_RE", &frames));

        // A model without frames cannot accidentally become one-shot.
        assert!(!model_is_final(&PropModelName("air_re".to_owned()).0, &[]));
    }
}
