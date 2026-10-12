//! Route a held handheld's owning controller before world actions/locomotion.
use crate::{
    Handedness,
    input::{InputAction, InputActionState, MenuHold, MenuPress},
    input_context::InputContext,
    scripts::{
        Effect, Message, MessagePayload,
        gui::{NavStep, stick_nav},
    },
};
use cgmath::vec2;
use shipyard::{EntityId, World};
use std::time::Duration;

#[derive(Clone, Copy, Debug)]
pub enum HandheldControl {
    Focus,
    Navigate { x: i32, y: i32 },
    Primary,
    Secondary,
    Back,
}

#[derive(Default)]
struct HandCapture {
    owner: Option<EntityId>,
    upper: MenuHold,
    blocked_buttons: [bool; 2],
    blocked_stick: bool,
    stick_latched: bool,
}

#[derive(Default)]
pub(crate) struct HandheldInput {
    hands: [HandCapture; 2],
}

impl HandheldInput {
    pub fn cancel(&mut self) {
        for hand in &mut self.hands {
            // Only this handheld's owner must re-arm. A pause or transition
            // must not capture controls on the unrelated hand or in flat mode.
            if hand.owner.take().is_some() {
                hand.blocked_buttons = [true; 2];
                hand.blocked_stick = true;
            }
            hand.upper.cancel();
            hand.stick_latched = false;
        }
    }

    pub fn update(
        &mut self,
        world: &World,
        panel: Option<EntityId>,
        actions: &mut InputActionState,
        input: &InputContext,
        elapsed: Duration,
        allowed: bool,
    ) -> (InputContext, Vec<Effect>, [bool; 2]) {
        let mut output = input.clone();
        let mut effects = Vec::new();
        let mut captured_lower = [false; 2];
        for (i, (hand, lower, upper)) in [
            (
                Handedness::Left,
                InputAction::LeftHandLowerButton,
                InputAction::LeftHandUpperButton,
            ),
            (
                Handedness::Right,
                InputAction::RightHandLowerButton,
                InputAction::RightHandUpperButton,
            ),
        ]
        .into_iter()
        .enumerate()
        {
            let tracked = input
                .pose_tracking
                .is_none_or(|tracking| tracking.head && tracking.hands[i]);
            let owner = crate::wielded_weapon::held_by_hand(world, hand).filter(|entity| {
                allowed
                    && tracked
                    && (panel.is_none() || Some(*entity) == panel)
                    && crate::scripts::script_util::entity_has_script(world, *entity, "MiniGameBoy")
            });
            let capture = &mut self.hands[i];
            let stick = if i == 0 {
                input.left_hand.thumbstick
            } else {
                input.right_hand.thumbstick
            };
            if capture.owner != owner {
                capture.owner = owner;
                capture.upper.cancel();
                capture.blocked_buttons = [lower, upper].map(|action| {
                    actions.is_held(action) && (owner.is_some() || !actions.just_triggered(action))
                });
                capture.blocked_stick = stick.x.abs().max(stick.y.abs()) >= 0.3;
                capture.stick_latched = false;
            }
            for (button, action) in [lower, upper].into_iter().enumerate() {
                if !actions.is_held(action) && !actions.just_triggered(action) {
                    capture.blocked_buttons[button] = false;
                }
            }
            if stick.x.abs().max(stick.y.abs()) < 0.3 {
                capture.blocked_stick = false;
            }
            let send = |control| Effect::Send {
                msg: Message {
                    to: owner.unwrap(),
                    payload: MessagePayload::HandheldInput(control),
                },
            };
            if let Some(entity) = owner {
                let opening = panel != owner;
                if opening
                    && ((!capture.blocked_buttons[0] && actions.just_triggered(lower))
                        || (!capture.blocked_stick && stick.x.abs().max(stick.y.abs()) >= 0.5))
                {
                    effects.push(Effect::OpenPanel { entity });
                    effects.push(send(HandheldControl::Focus));
                    capture.blocked_buttons = [actions.is_held(lower), actions.is_held(upper)];
                    capture.blocked_stick = true;
                }
                if !opening && !capture.blocked_stick {
                    let (step, latch) = stick_nav(stick, capture.stick_latched);
                    capture.stick_latched = latch;
                    if let Some(step) = step {
                        let (x, y) = match step {
                            NavStep::TierPrev => (0, 1),
                            NavStep::TierNext => (0, -1),
                            NavStep::PowerPrev => (-1, 0),
                            NavStep::PowerNext => (1, 0),
                        };
                        effects.push(send(HandheldControl::Navigate { x, y }));
                    }
                }
                if !opening && !capture.blocked_buttons[0] && actions.just_triggered(lower) {
                    effects.push(send(HandheldControl::Primary));
                }
                if !opening && !capture.blocked_buttons[1] {
                    if actions.just_triggered(upper) {
                        capture.upper.press();
                    }
                    let press = if actions.is_held(upper) {
                        capture.upper.tick(elapsed)
                    } else {
                        capture.upper.release()
                    };
                    match press {
                        MenuPress::Short => effects.push(send(HandheldControl::Secondary)),
                        MenuPress::Long => effects.push(send(HandheldControl::Back)),
                        MenuPress::None => {}
                    }
                }
            }
            captured_lower[i] = owner.is_some() || capture.blocked_buttons[0];
            for (button, action) in [lower, upper].into_iter().enumerate() {
                if owner.is_some() || capture.blocked_buttons[button] {
                    actions.consume_trigger(action);
                }
            }
            if owner.is_some() || capture.blocked_stick {
                if i == 0 {
                    output.left_hand.thumbstick = vec2(0.0, 0.0);
                } else {
                    output.right_hand.thumbstick = vec2(0.0, 0.0);
                }
            }
        }
        (output, effects, captured_lower)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{input_context::PoseTracking, mission::PlayerInfo};
    use cgmath::{Quaternion, vec3};

    fn held_game() -> (World, EntityId) {
        let mut world = World::new();
        let entity = world.add_entity(dark::properties::PropScripts {
            scripts: vec!["MiniGameBoy".into()],
            inherits: true,
        });
        world.add_unique(PlayerInfo {
            pos: vec3(0.0, 0.0, 0.0),
            rotation: Quaternion::new(1.0, 0.0, 0.0, 0.0),
            entity_id: entity,
            inventory_entity_id: entity,
            left_hand_entity_id: Some(entity),
            right_hand_entity_id: None,
        });
        (world, entity)
    }

    #[test]
    fn only_owner_controls_are_consumed_and_drop_requires_release_and_neutral() {
        let (world, entity) = held_game();
        let mut router = HandheldInput::default();
        let mut actions = InputActionState::default();
        let mut input = InputContext::default();
        router.update(
            &world,
            Some(entity),
            &mut actions,
            &input,
            Duration::ZERO,
            true,
        );
        input.left_hand.thumbstick = vec2(1.0, 0.0);
        input.right_hand.thumbstick = vec2(0.0, 1.0);
        actions.trigger(InputAction::LeftHandLowerButton);
        actions.trigger(InputAction::RightHandLowerButton);
        let (out, effects, lower) = router.update(
            &world,
            Some(entity),
            &mut actions,
            &input,
            Duration::ZERO,
            true,
        );
        assert_eq!(out.left_hand.thumbstick, vec2(0.0, 0.0));
        assert_eq!(out.right_hand.thumbstick, input.right_hand.thumbstick);
        assert_eq!(effects.len(), 2);
        assert_eq!(lower, [true, false]);
        assert!(!actions.just_triggered(InputAction::LeftHandLowerButton));
        assert!(actions.just_triggered(InputAction::RightHandLowerButton));
        actions.clear_triggered();
        world
            .borrow::<shipyard::UniqueViewMut<PlayerInfo>>()
            .unwrap()
            .left_hand_entity_id = None;
        let (out, effects, lower) =
            router.update(&world, None, &mut actions, &input, Duration::ZERO, true);
        assert_eq!(out.left_hand.thumbstick, vec2(0.0, 0.0));
        assert!(effects.is_empty());
        assert_eq!(lower, [true, false]);
        // Reacquiring while still held cannot mint a fresh button/stick edge.
        world
            .borrow::<shipyard::UniqueViewMut<PlayerInfo>>()
            .unwrap()
            .left_hand_entity_id = Some(entity);
        let (_, effects, _) = router.update(
            &world,
            Some(entity),
            &mut actions,
            &input,
            Duration::ZERO,
            true,
        );
        assert!(effects.is_empty());
        world
            .borrow::<shipyard::UniqueViewMut<PlayerInfo>>()
            .unwrap()
            .left_hand_entity_id = None;
        actions.release(InputAction::LeftHandLowerButton);
        input.left_hand.thumbstick = vec2(0.0, 0.0);
        router.update(&world, None, &mut actions, &input, Duration::ZERO, true);
        input.left_hand.thumbstick = vec2(1.0, 0.0);
        actions.trigger(InputAction::LeftHandLowerButton);
        let (out, _, lower) =
            router.update(&world, None, &mut actions, &input, Duration::ZERO, true);
        assert_eq!(out.left_hand.thumbstick, input.left_hand.thumbstick);
        assert_eq!(lower, [false, false]);
        assert!(actions.just_triggered(InputAction::LeftHandLowerButton));
    }

    #[test]
    fn cancellation_never_captures_an_unrelated_hand_or_flat_controls() {
        let (world, entity) = held_game();
        for allowed in [false, true] {
            let mut router = HandheldInput::default();
            let mut input = InputContext::default();
            input.right_hand.thumbstick = vec2(1.0, 0.0);
            let mut actions = InputActionState::default();
            actions.trigger(InputAction::RightHandLowerButton);
            router.cancel();
            let (out, _, lower) =
                router.update(&world, None, &mut actions, &input, Duration::ZERO, allowed);
            assert_eq!(out.right_hand.thumbstick, input.right_hand.thumbstick);
            assert!(actions.just_triggered(InputAction::RightHandLowerButton));
            assert!(!lower[1]);
            // Even after the left hand actually owns the device, cancellation
            // cannot absorb the right hand's next world gesture.
            router.update(
                &world,
                Some(entity),
                &mut actions,
                &InputContext::default(),
                Duration::ZERO,
                true,
            );
            router.cancel();
            let (out, _, lower) =
                router.update(&world, None, &mut actions, &input, Duration::ZERO, false);
            assert_eq!(out.right_hand.thumbstick, input.right_hand.thumbstick);
            assert!(actions.just_triggered(InputAction::RightHandLowerButton));
            assert!(!lower[1]);
        }
    }

    #[test]
    fn long_press_never_emits_secondary_and_tracking_loss_cancels_tap() {
        let (world, entity) = held_game();
        let mut router = HandheldInput::default();
        let mut actions = InputActionState::default();
        let mut input = InputContext::default();
        router.update(
            &world,
            Some(entity),
            &mut actions,
            &input,
            Duration::ZERO,
            true,
        );
        actions.trigger(InputAction::LeftHandUpperButton);
        let mut effects = Vec::new();
        for _ in 0..10 {
            effects.extend(
                router
                    .update(
                        &world,
                        Some(entity),
                        &mut actions,
                        &input,
                        Duration::from_millis(100),
                        true,
                    )
                    .1,
            );
            actions.clear_triggered();
        }
        actions.release(InputAction::LeftHandUpperButton);
        effects.extend(
            router
                .update(
                    &world,
                    Some(entity),
                    &mut actions,
                    &input,
                    Duration::ZERO,
                    true,
                )
                .1,
        );
        assert_eq!(effects.len(), 1);
        assert!(matches!(
            &effects[0],
            Effect::Send {
                msg: Message {
                    payload: MessagePayload::HandheldInput(HandheldControl::Back),
                    ..
                }
            }
        ));
        actions.trigger(InputAction::LeftHandUpperButton);
        router.update(
            &world,
            Some(entity),
            &mut actions,
            &input,
            Duration::ZERO,
            true,
        );
        actions.clear_triggered();
        input.pose_tracking = Some(PoseTracking {
            head: true,
            hands: [false, true],
        });
        assert!(
            router
                .update(
                    &world,
                    Some(entity),
                    &mut actions,
                    &input,
                    Duration::ZERO,
                    true
                )
                .1
                .is_empty()
        );
        actions.release(InputAction::LeftHandUpperButton);
        input.pose_tracking = None;
        assert!(
            router
                .update(
                    &world,
                    Some(entity),
                    &mut actions,
                    &input,
                    Duration::ZERO,
                    true
                )
                .1
                .is_empty()
        );
    }
}
