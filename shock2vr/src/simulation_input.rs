//! Retain sampled control transitions until a simulation tick consumes them.
//! Continuous poses/sticks stay current; queued clicks keep their screen point.
use std::collections::VecDeque;

use crate::{input_context::InputContext, scripts::Effect};

pub(crate) struct TickInput {
    observed: InputContext,
    pending: VecDeque<(InputContext, Vec<Effect>)>,
}

impl Default for TickInput {
    fn default() -> Self {
        Self {
            observed: InputContext::default(),
            pending: VecDeque::new(),
        }
    }
}

impl TickInput {
    pub fn clear(&mut self) {
        *self = Self::default();
    }

    pub fn push(&mut self, input: &InputContext, effects: Vec<Effect>) {
        if controls_changed(&self.observed, input) || !effects.is_empty() {
            self.pending.push_back((input.clone(), effects));
        }
        self.observed = input.clone();
    }

    pub fn next(&mut self, latest: &InputContext) -> (InputContext, Vec<Effect>) {
        let Some((sample, effects)) = self.pending.pop_front() else {
            return (latest.clone(), Vec::new());
        };
        let mut input = latest.clone();
        for (to, from) in [
            (&mut input.left_hand, &sample.left_hand),
            (&mut input.right_hand, &sample.right_hand),
        ] {
            to.trigger_value = from.trigger_value;
            to.squeeze_value = from.squeeze_value;
            to.a_value = from.a_value;
        }
        input.pointer = sample.pointer;
        input.jump = sample.jump;
        input.jump_button_held = sample.jump_button_held;
        input.crouch = sample.crouch;
        (input, effects)
    }
}

fn controls_changed(a: &InputContext, b: &InputContext) -> bool {
    // These are the existing scene control thresholds: debug forces (0.05),
    // cutscene touch (0.2), and gameplay/UI presses (0.5). Preserve equality
    // as well: consumers use both strict and inclusive comparisons. Analog
    // noise within a region must not build an input backlog at high refresh.
    let crossed = |a: f32, b: f32| {
        [0.0, 0.05, 0.2, 0.5]
            .into_iter()
            .any(|threshold| a.partial_cmp(&threshold) != b.partial_cmp(&threshold))
    };
    a.jump != b.jump
        || a.jump_button_held != b.jump_button_held
        || a.crouch != b.crouch
        || a.pointer.map(|p| p.pressed) != b.pointer.map(|p| p.pressed)
        || [(&a.left_hand, &b.left_hand), (&a.right_hand, &b.right_hand)]
            .into_iter()
            .any(|(a, b)| {
                crossed(a.trigger_value, b.trigger_value)
                    || crossed(a.squeeze_value, b.squeeze_value)
                    || crossed(a.a_value, b.a_value)
            })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input_context::Pointer2D;
    use cgmath::{vec2, vec3};

    #[test]
    fn short_presses_releases_and_repeated_actions_survive_zero_tick_frames() {
        let mut queue = TickInput::default();
        let mut input = InputContext::default();
        for pressed in [true, false, true, false] {
            input.right_hand.trigger_value = if pressed { 1.0 } else { 0.0 };
            input.jump = pressed;
            queue.push(
                &input,
                if pressed {
                    vec![Effect::NoEffect]
                } else {
                    vec![]
                },
            );
        }
        for pressed in [true, false, true, false] {
            let (tick, effects) = queue.next(&input);
            assert_eq!(tick.right_hand.trigger_value > 0.5, pressed);
            assert_eq!(tick.jump, pressed);
            assert_eq!(effects.len(), usize::from(pressed));
        }
        for _ in 0..8 {
            let (tick, effects) = queue.next(&input);
            assert_eq!(tick.right_hand.trigger_value, 0.0);
            assert!(effects.is_empty());
        }
    }

    #[test]
    fn queued_click_keeps_its_point_but_uses_current_tracking() {
        let mut queue = TickInput::default();
        let mut input = InputContext::default();
        input.pointer = Some(Pointer2D {
            position: vec2(0.2, 0.3),
            pressed: true,
        });
        queue.push(&input, vec![]);
        input.head.position = vec3(1.0, 2.0, 3.0);
        input.right_hand.position = vec3(4.0, 5.0, 6.0);
        input.right_hand.thumbstick = vec2(0.0, 1.0);
        input.pointer.as_mut().unwrap().position = vec2(0.9, 0.9);
        let (tick, _) = queue.next(&input);
        assert_eq!(tick.pointer.unwrap().position, vec2(0.2, 0.3));
        assert_eq!(tick.head.position, input.head.position);
        assert_eq!(tick.right_hand.position, input.right_hand.position);
        assert_eq!(tick.right_hand.thumbstick, input.right_hand.thumbstick);
    }

    #[test]
    fn analog_noise_does_not_queue_and_suspend_discards_pending_actions() {
        let mut queue = TickInput::default();
        let mut input = InputContext::default();
        input.left_hand.squeeze_value = 0.7;
        queue.push(&input, vec![]);
        queue.next(&input);
        for value in [0.71, 0.69, 0.75] {
            input.left_hand.squeeze_value = value;
            queue.push(&input, vec![]);
        }
        assert!(queue.pending.is_empty());
        queue.push(&input, vec![Effect::NoEffect]);
        queue.clear();
        assert!(queue.next(&input).1.is_empty());
    }
}
