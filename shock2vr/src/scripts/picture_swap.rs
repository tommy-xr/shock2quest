use dark::properties::{PropTweqModelConfig, PropTweqModelState};
use shipyard::{EntityId, Get, View, ViewMut, World};

use crate::{physics::PhysicsWorld, time::Time};

use super::{Effect, MessagePayload, Script, script_util::send_to_all_switch_links};

/// Retail `PictureSwap` transition length (`StaticOver`, 1.0 seconds).
const STATIC_DURATION_SECONDS: f32 = 1.0;

/// The Recreation deck's interactive code-art display.
///
/// The original script inherits `ModelSwappable`: an accepted `FrobWorldEnd`
/// or `TurnOn` advances once to the interstitial static model, starts the
/// one-second `StaticOver` timer, then advances once more to the next picture.
/// It rejects another activation while that transition is underway and relays
/// `TurnOn` over its authored SwitchLinks.
pub struct PictureSwap {
    static_time_remaining: Option<f32>,
}

impl PictureSwap {
    pub fn new() -> Self {
        Self {
            static_time_remaining: None,
        }
    }

    fn activate(&mut self, entity_id: EntityId, world: &World) -> Effect {
        if self.static_time_remaining.is_some() {
            return Effect::NoEffect;
        }

        let Some(change_to_static) = advance_model(world, entity_id) else {
            return Effect::NoEffect;
        };
        self.static_time_remaining = Some(STATIC_DURATION_SECONDS);

        Effect::combine(vec![
            change_to_static,
            send_to_all_switch_links(world, entity_id, MessagePayload::TurnOn { from: entity_id }),
        ])
    }
}

impl Script for PictureSwap {
    fn initialize(&mut self, entity_id: EntityId, world: &World) -> Effect {
        // A save taken during the retail script's one-second static phase
        // persists the odd model-tweq frame. Script-local timers are not part
        // of shock2quest saves, so reconstruct that pending completion.
        let v_state = world.borrow::<View<PropTweqModelState>>().unwrap();
        if v_state
            .get(entity_id)
            .is_ok_and(|state| state.frame % 2 == 1)
        {
            self.static_time_remaining = Some(STATIC_DURATION_SECONDS);
        }
        Effect::NoEffect
    }

    fn handle_message(
        &mut self,
        entity_id: EntityId,
        world: &World,
        _physics: &PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        match msg {
            MessagePayload::Frob | MessagePayload::TurnOn { .. } => self.activate(entity_id, world),
            _ => Effect::NoEffect,
        }
    }

    fn update(
        &mut self,
        entity_id: EntityId,
        world: &World,
        _physics: &PhysicsWorld,
        time: &Time,
    ) -> Effect {
        let Some(remaining) = self.static_time_remaining else {
            return Effect::NoEffect;
        };
        let remaining = remaining - time.elapsed.as_secs_f32();
        if remaining > 0.0 {
            self.static_time_remaining = Some(remaining);
            return Effect::NoEffect;
        }

        self.static_time_remaining = None;
        advance_model(world, entity_id).unwrap_or(Effect::NoEffect)
    }
}

/// Advance the persisted model-tweq frame by one, wrapping after its last
/// authored model just like retail `ModelSwappable`.
fn advance_model(world: &World, entity_id: EntityId) -> Option<Effect> {
    let v_config = world.borrow::<View<PropTweqModelConfig>>().unwrap();
    let mut v_state = world.borrow::<ViewMut<PropTweqModelState>>().unwrap();
    let config = v_config.get(entity_id).ok()?;
    let state = (&mut v_state).get(entity_id).ok()?;
    let model_count = config.model_names.len();
    if model_count == 0 {
        return None;
    }
    let next_frame = (state.frame + 1) % model_count;
    let model_name = config.model_names.get(next_frame)?.clone();
    state.frame = next_frame;

    Some(Effect::ChangeModel {
        entity_id,
        model_name,
    })
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use dark::properties::{
        Link, Links, PropTweqModelConfig, PropTweqModelState, ToLink, TweqAnimationConfig,
        TweqAnimationState, TweqHalt, WrappedEntityId,
    };
    use shipyard::{Get, View, World};

    use super::*;

    fn picture_world() -> (World, EntityId) {
        let mut world = World::new();
        let target = world.add_entity(());
        let picture = world.add_entity((
            PropTweqModelConfig {
                animation_config: TweqAnimationConfig::empty(),
                halt: TweqHalt::Continue,
                model_names: vec![
                    "pic05".to_owned(),
                    "static".to_owned(),
                    "pic03".to_owned(),
                    "static".to_owned(),
                    "code10".to_owned(),
                    "static".to_owned(),
                ],
            },
            PropTweqModelState {
                animation_state: TweqAnimationState::empty(),
                frame: 0,
            },
            Links {
                to_links: vec![ToLink {
                    to_template_id: 1,
                    to_entity_id: Some(WrappedEntityId(target)),
                    link: Link::SwitchLink,
                }],
            },
        ));
        (world, picture)
    }

    fn changed_model(effect: Effect) -> Option<String> {
        Effect::flatten(vec![effect])
            .into_iter()
            .find_map(|effect| match effect {
                Effect::ChangeModel { model_name, .. } => Some(model_name),
                _ => None,
            })
    }

    fn frame(world: &World, entity_id: EntityId) -> usize {
        world
            .borrow::<View<PropTweqModelState>>()
            .unwrap()
            .get(entity_id)
            .unwrap()
            .frame
    }

    #[test]
    fn frob_uses_static_then_advances_to_the_next_picture_after_one_second() {
        let (world, picture) = picture_world();
        let physics = PhysicsWorld::new();
        let mut script = PictureSwap::new();

        let effect = script.handle_message(picture, &world, &physics, &MessagePayload::Frob);
        assert_eq!(changed_model(effect), Some("static".to_owned()));
        assert_eq!(frame(&world, picture), 1);

        let effect = script.update(
            picture,
            &world,
            &physics,
            &Time {
                elapsed: Duration::from_millis(999),
                total: Duration::from_millis(999),
            },
        );
        assert_eq!(changed_model(effect), None);
        assert_eq!(frame(&world, picture), 1);

        let effect = script.update(
            picture,
            &world,
            &physics,
            &Time {
                elapsed: Duration::from_millis(1),
                total: Duration::from_secs(1),
            },
        );
        assert_eq!(changed_model(effect), Some("pic03".to_owned()));
        assert_eq!(frame(&world, picture), 2);
    }

    #[test]
    fn activation_relays_turn_on_and_rejects_reentry_during_static() {
        let (world, picture) = picture_world();
        let physics = PhysicsWorld::new();
        let mut script = PictureSwap::new();

        let effects = Effect::flatten(vec![script.handle_message(
            picture,
            &world,
            &physics,
            &MessagePayload::TurnOn { from: picture },
        )]);
        assert!(effects.iter().any(|effect| {
            matches!(
                effect,
                Effect::Send {
                    msg: super::super::Message {
                        payload: MessagePayload::TurnOn { from },
                        ..
                    }
                } if *from == picture
            )
        }));

        let ignored = script.handle_message(picture, &world, &physics, &MessagePayload::Frob);
        assert!(matches!(ignored, Effect::NoEffect));
        assert_eq!(frame(&world, picture), 1);
    }

    #[test]
    fn model_swappable_wraps_after_the_sixth_authored_frame() {
        let (world, picture) = picture_world();
        {
            let mut state = world.borrow::<ViewMut<PropTweqModelState>>().unwrap();
            (&mut state).get(picture).unwrap().frame = 5;
        }

        assert_eq!(
            changed_model(advance_model(&world, picture).unwrap()),
            Some("pic05".to_owned())
        );
        assert_eq!(frame(&world, picture), 0);
    }
}
