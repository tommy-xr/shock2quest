use dark::properties::{
    ObjectState, PropConsumeType, PropModelName, PropObjState, PropSymName, PropTweqModelConfig,
};
use engine::audio::AudioHandle;
use serde::{Deserialize, Serialize};
use shipyard::{EntityId, Get, View, World};

use crate::{physics::PhysicsWorld, scripts::script_util::play_environmental_sound, time::Time};

use super::{
    Effect, MessagePayload, Script, ScriptRestoreContext, ScriptState, ScriptStateError,
    script_util::send_to_all_switch_links_and_self,
};

const SCRIPT_STATE_KEY: &str = "shock2vr.obj_consume_button";

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
enum OneShotPhase {
    #[default]
    Idle,
    PendingDelivery,
    AwaitingFinalModel,
}

#[derive(Serialize, Deserialize)]
struct ObjConsumeButtonState {
    one_shot_phase: OneShotPhase,
}

pub struct ObjConsumeButton {
    one_shot_phase: OneShotPhase,
    activation_sent: bool,
}
impl ObjConsumeButton {
    pub fn new() -> ObjConsumeButton {
        ObjConsumeButton {
            one_shot_phase: OneShotPhase::Idle,
            activation_sent: false,
        }
    }

    fn consume_if_eligible(
        &mut self,
        world: &World,
        receptor: EntityId,
        candidate: EntityId,
    ) -> Effect {
        let one_shot = receptor_has_final_model(world, receptor);
        if (one_shot && self.one_shot_phase != OneShotPhase::Idle)
            || !can_consume_entity(world, receptor, candidate)
        {
            return Effect::NoEffect;
        }

        // The TurnOn/model-change effects apply after this message batch. Latch
        // immediately so two VR hands cannot provision distinct vials before
        // the authored final model becomes observable in the ECS. Reusable
        // consumers without a final model retain their original behavior.
        if one_shot {
            self.one_shot_phase = OneShotPhase::PendingDelivery;
            self.activation_sent = true;
        }
        consume(world, receptor, candidate)
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
                self.consume_if_eligible(world, entity_id, *entity)
            }
            // Flat: frobbing the receptor consumes the first matching item the
            // player carries (no hold-item-near-the-receptor step - that is
            // VR-only, handled above).
            MessagePayload::Frob => {
                if self.one_shot_phase != OneShotPhase::Idle {
                    return Effect::NoEffect;
                }
                match super::script_util::player_carried_items(world)
                    .into_iter()
                    .find(|item| can_consume_entity(world, entity_id, *item))
                {
                    Some(item) => self.consume_if_eligible(world, entity_id, item),
                    None => Effect::NoEffect,
                }
            }
            MessagePayload::TurnOn { from }
                if *from == entity_id && self.one_shot_phase == OneShotPhase::PendingDelivery =>
            {
                // This acknowledgement is handled in the same batch as every
                // linked recipient. Persist it separately from the final model
                // so a mid-animation save does not replay the plot activation.
                self.one_shot_phase = OneShotPhase::AwaitingFinalModel;
                Effect::NoEffect
            }
            _ => Effect::NoEffect,
        }
    }

    fn accepts_tool(&self, entity_id: EntityId, world: &World, tool: EntityId) -> bool {
        (!receptor_has_final_model(world, entity_id) || self.one_shot_phase == OneShotPhase::Idle)
            && can_consume_entity(world, entity_id, tool)
    }

    fn update(
        &mut self,
        entity_id: EntityId,
        world: &World,
        _physics: &PhysicsWorld,
        _time: &Time,
    ) -> Effect {
        match self.one_shot_phase {
            OneShotPhase::Idle => Effect::NoEffect,
            OneShotPhase::PendingDelivery if self.activation_sent => Effect::NoEffect,
            OneShotPhase::PendingDelivery => {
                // A save can capture the consumed item after DestroyEntity but
                // before the deferred TurnOn is delivered. Re-emit that
                // activation once after restoring the pending delivery.
                self.activation_sent = true;
                send_to_all_switch_links_and_self(
                    world,
                    entity_id,
                    MessagePayload::TurnOn { from: entity_id },
                )
            }
            OneShotPhase::AwaitingFinalModel if receptor_is_used(world, entity_id) => {
                self.one_shot_phase = OneShotPhase::Idle;
                self.activation_sent = false;
                Effect::NoEffect
            }
            OneShotPhase::AwaitingFinalModel => Effect::NoEffect,
        }
    }

    fn script_state_key(&self) -> Option<&'static str> {
        Some(SCRIPT_STATE_KEY)
    }

    fn save_state(&self) -> Result<ScriptState, ScriptStateError> {
        ScriptState::encode(
            1,
            &ObjConsumeButtonState {
                one_shot_phase: self.one_shot_phase,
            },
            SCRIPT_STATE_KEY,
        )
    }

    fn restore_state(
        &mut self,
        state: &ScriptState,
        _context: &ScriptRestoreContext<'_>,
    ) -> Result<(), ScriptStateError> {
        let restored: ObjConsumeButtonState = state.decode(1, SCRIPT_STATE_KEY)?;
        self.one_shot_phase = restored.one_shot_phase;
        self.activation_sent = false;
        Ok(())
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
    if receptor_is_used(world, self_id) {
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

fn receptor_has_final_model(world: &World, entity_id: EntityId) -> bool {
    let models = world.borrow::<View<PropModelName>>().unwrap();
    let configs = world.borrow::<View<PropTweqModelConfig>>().unwrap();
    models.get(entity_id).is_ok()
        && configs
            .get(entity_id)
            .is_ok_and(|config| !config.model_names.is_empty())
}

fn receptor_is_used(world: &World, entity_id: EntityId) -> bool {
    let models = world.borrow::<View<PropModelName>>().unwrap();
    let configs = world.borrow::<View<PropTweqModelConfig>>().unwrap();
    match (models.get(entity_id), configs.get(entity_id)) {
        (Ok(model), Ok(config)) => model_is_final(&model.0, &config.model_names),
        _ => false,
    }
}

fn model_is_final(current: &str, frames: &[String]) -> bool {
    frames
        .last()
        .map(|last| current.eq_ignore_ascii_case(last))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use std::{cell::Cell, collections::HashMap, rc::Rc};

    use cgmath::{Quaternion, vec3};
    use dark::properties::{
        Link, Links, PropConsumeType, PropModelName, PropObjState, PropSymName,
        PropTweqModelConfig, ToLink, TweqAnimationConfig, TweqHalt, WrappedEntityId,
    };
    use shipyard::{IntoIter, World};

    use crate::{
        mission::{GlobalTemplateIdMap, PlayerInfo},
        save_load::{EntitySaveData, to_save_data_with_scripts},
        scripts::{Message, ScriptWorld},
    };

    use super::*;

    fn one_shot_receptor(world: &mut World) -> EntityId {
        world.add_entity((
            PropConsumeType("AAToxin".to_owned()),
            PropModelName("air_reof".to_owned()),
            PropTweqModelConfig {
                animation_config: TweqAnimationConfig::empty(),
                halt: TweqHalt::StopTweq,
                model_names: vec!["air_reof".to_owned(), "air_re".to_owned()],
            },
        ))
    }

    struct TurnOnRecorder(Rc<Cell<u32>>);

    impl Script for TurnOnRecorder {
        fn handle_message(
            &mut self,
            _entity_id: EntityId,
            _world: &World,
            _physics: &PhysicsWorld,
            msg: &MessagePayload,
        ) -> Effect {
            if matches!(msg, MessagePayload::TurnOn { .. }) {
                self.0.set(self.0.get() + 1);
            }
            Effect::NoEffect
        }
    }

    #[derive(Clone, Copy, Debug)]
    enum SaveWindow {
        BeforeDelivery,
        AfterDelivery,
        AfterFinalModel,
    }

    fn round_trip_consumption_at(window: SaveWindow) -> (u32, u32) {
        let mut world = World::new();
        let receptor = one_shot_receptor(&mut world);
        let objective = world.add_entity(());
        world.add_component(
            receptor,
            Links {
                to_links: vec![ToLink {
                    to_template_id: 42,
                    to_entity_id: Some(WrappedEntityId(objective)),
                    link: Link::SwitchLink,
                }],
            },
        );
        let toxin = world.add_entity(PropSymName("AAToxin".to_owned()));
        let player = world.add_entity(());
        let inventory = world.add_entity(Links::empty());
        world.add_unique(PlayerInfo {
            pos: vec3(0.0, 0.0, 0.0),
            rotation: Quaternion::new(1.0, 0.0, 0.0, 0.0),
            entity_id: player,
            left_hand_entity_id: None,
            right_hand_entity_id: None,
            inventory_entity_id: inventory,
        });
        world.add_unique(GlobalTemplateIdMap(HashMap::from([
            (1001, WrappedEntityId(receptor)),
            (1002, WrappedEntityId(objective)),
        ])));

        let before_count = Rc::new(Cell::new(0));
        let mut scripts = ScriptWorld::new();
        scripts.add_entity2(receptor, Box::new(ObjConsumeButton::new()));
        scripts.add_entity2(objective, Box::new(TurnOnRecorder(before_count.clone())));
        scripts.dispatch(Message {
            to: receptor,
            payload: MessagePayload::ProvideForConsumption { entity: toxin },
        });
        let effects = scripts.update(&world, &PhysicsWorld::new(), &Time::default());
        assert!(effects.iter().any(|effect| matches!(
            effect,
            Effect::DestroyEntity { entity_id } if *entity_id == toxin
        )));
        world.delete_entity(toxin);

        if matches!(
            window,
            SaveWindow::AfterDelivery | SaveWindow::AfterFinalModel
        ) {
            scripts.update(&world, &PhysicsWorld::new(), &Time::default());
            assert_eq!(before_count.get(), 1);
        }
        if matches!(window, SaveWindow::AfterFinalModel) {
            world.add_component(receptor, PropModelName("air_re".to_owned()));
            scripts.update(&world, &PhysicsWorld::new(), &Time::default());
        }

        let (saved_world, _) = to_save_data_with_scripts(&world, Some(&scripts));
        let saved_world: EntitySaveData =
            serde_json::from_value(serde_json::to_value(saved_world).unwrap()).unwrap();
        assert!(!saved_world.all_entities.contains(&toxin.inner()));

        let mut loaded_world = World::new();
        let (_, entity_map) = saved_world.instantiate(&mut loaded_world);
        assert!(!entity_map.contains_key(&toxin));
        assert!(
            loaded_world
                .borrow::<View<PropSymName>>()
                .unwrap()
                .iter()
                .all(|name| !name.0.eq_ignore_ascii_case("AAToxin"))
        );
        let loaded_receptor = entity_map[&receptor];
        let loaded_objective = entity_map[&objective];
        let after_count = Rc::new(Cell::new(0));
        let mut loaded_scripts = ScriptWorld::new();
        loaded_scripts.add_entity2(loaded_receptor, Box::new(ObjConsumeButton::new()));
        loaded_scripts.add_entity2(
            loaded_objective,
            Box::new(TurnOnRecorder(after_count.clone())),
        );
        loaded_scripts
            .restore_states(&saved_world.script_states, &entity_map)
            .unwrap();

        for _ in 0..3 {
            loaded_scripts.update(&loaded_world, &PhysicsWorld::new(), &Time::default());
        }
        (before_count.get(), after_count.get())
    }

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

    #[test]
    fn two_same_tick_provisions_consume_only_the_first_item() {
        let mut world = World::new();
        let receptor = one_shot_receptor(&mut world);
        let first_toxin = world.add_entity(PropSymName("AAToxin".to_owned()));
        let second_toxin = world.add_entity(PropSymName("AAToxin".to_owned()));
        let physics = PhysicsWorld::new();
        let mut script = ObjConsumeButton::new();

        let first = Effect::flatten(vec![script.handle_message(
            receptor,
            &world,
            &physics,
            &MessagePayload::ProvideForConsumption {
                entity: first_toxin,
            },
        )]);
        let second = Effect::flatten(vec![script.handle_message(
            receptor,
            &world,
            &physics,
            &MessagePayload::ProvideForConsumption {
                entity: second_toxin,
            },
        )]);

        assert!(first.iter().any(|effect| matches!(
            effect,
            Effect::DestroyEntity { entity_id } if *entity_id == first_toxin
        )));
        assert!(
            !second.iter().any(|effect| matches!(
                effect,
                Effect::DestroyEntity { entity_id } if *entity_id == second_toxin
            )),
            "the receptor must latch before its deferred model change, got {second:?}"
        );
    }

    #[test]
    fn consumer_without_a_final_model_remains_reusable() {
        let mut world = World::new();
        let receptor = world.add_entity(PropConsumeType("AccessCard".to_owned()));
        let first_card = world.add_entity(PropSymName("AccessCard".to_owned()));
        let second_card = world.add_entity(PropSymName("AccessCard".to_owned()));
        let physics = PhysicsWorld::new();
        let mut script = ObjConsumeButton::new();

        let first = Effect::flatten(vec![script.handle_message(
            receptor,
            &world,
            &physics,
            &MessagePayload::ProvideForConsumption { entity: first_card },
        )]);
        let second = Effect::flatten(vec![script.handle_message(
            receptor,
            &world,
            &physics,
            &MessagePayload::ProvideForConsumption {
                entity: second_card,
            },
        )]);

        assert!(first.iter().any(|effect| matches!(
            effect,
            Effect::DestroyEntity { entity_id } if *entity_id == first_card
        )));
        assert!(second.iter().any(|effect| matches!(
            effect,
            Effect::DestroyEntity { entity_id } if *entity_id == second_card
        )));
    }

    #[test]
    fn consumption_save_windows_preserve_exactly_one_activation() {
        for window in [
            SaveWindow::BeforeDelivery,
            SaveWindow::AfterDelivery,
            SaveWindow::AfterFinalModel,
        ] {
            let (before, after) = round_trip_consumption_at(window);
            assert_eq!(
                (before, after),
                match window {
                    SaveWindow::BeforeDelivery => (0, 1),
                    SaveWindow::AfterDelivery | SaveWindow::AfterFinalModel => (1, 0),
                },
                "activation count mismatch for {window:?}"
            );
        }
    }
}
