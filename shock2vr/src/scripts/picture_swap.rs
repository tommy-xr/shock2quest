use dark::properties::{
    PropTweqModelConfig, PropTweqModelState, TweqAnimationConfig, TweqAnimationState, TweqHalt,
};
use shipyard::{EntityId, Get, View, ViewMut, World};

use crate::physics::PhysicsWorld;

use super::{Effect, MessagePayload, Script};

/// `PictureSwap` - the Rec deck's `Code Art` picture frames.
///
/// The frames carry a model tweq (`PropTweqModelConfig` / `PropTweqModelState`)
/// whose frame list alternates artwork and static, ending with a digit of the
/// transmitter code. Frobbing a frame steps that tweq forward by one frame
/// instead of letting time drive it, so the player reads the code off the art.
pub struct PictureSwap {}

impl PictureSwap {
    pub fn new() -> PictureSwap {
        PictureSwap {}
    }
}

impl Script for PictureSwap {
    fn handle_message(
        &mut self,
        entity_id: EntityId,
        world: &World,
        _physics: &PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        match msg {
            MessagePayload::Frob => advance_frame(world, entity_id),
            _ => Effect::NoEffect,
        }
    }
}

/// Position within a model tweq: which frame is showing, and which way the
/// animation is currently walking the frame list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModelFrame {
    pub index: usize,
    pub reverse: bool,
}

/// Advance a model tweq by a single frame, honoring its animation config.
///
/// Mirrors the engine's per-tick model tweq step: forward until the last
/// authored model, then "hit the edge". At an edge, a halt action other than
/// `Continue` ends the tweq (`None`); otherwise a `WRAP` tweq jumps back to
/// the opposite end while a non-wrapping one bounces - it reverses direction
/// and steps inward. `ONEBOUNCE` only lets the halt action fire once the tweq
/// is already running in reverse (i.e. after the bounce off the top).
fn advance_frame_index(config: &PropTweqModelConfig, current: ModelFrame) -> Option<ModelFrame> {
    let top = config.model_names.len().checked_sub(1)?;

    let hit_edge = |top_edge: bool| -> Option<ModelFrame> {
        let one_bounce = config
            .animation_config
            .contains(TweqAnimationConfig::ONEBOUNCE);
        if (!one_bounce || current.reverse) && !matches!(config.halt, TweqHalt::Continue) {
            return None;
        }

        if config.animation_config.contains(TweqAnimationConfig::WRAP) {
            Some(ModelFrame {
                index: if top_edge { 0 } else { top },
                reverse: current.reverse,
            })
        } else {
            Some(ModelFrame {
                index: if top_edge {
                    top.saturating_sub(1)
                } else {
                    1.min(top)
                },
                reverse: !current.reverse,
            })
        }
    };

    if current.reverse {
        if current.index > 0 {
            Some(ModelFrame {
                index: current.index - 1,
                reverse: true,
            })
        } else {
            hit_edge(false)
        }
    } else if current.index < top {
        Some(ModelFrame {
            index: current.index + 1,
            reverse: false,
        })
    } else {
        hit_edge(true)
    }
}

/// Step the entity's model tweq one frame forward and swap in the new model.
/// The new position is written back to `PropTweqModelState` so it survives
/// save/load and further frobs continue the cycle.
fn advance_frame(world: &World, entity_id: EntityId) -> Effect {
    let v_config = world.borrow::<View<PropTweqModelConfig>>().unwrap();
    let mut v_state = world.borrow::<ViewMut<PropTweqModelState>>().unwrap();

    let Ok(config) = v_config.get(entity_id) else {
        return Effect::NoEffect;
    };
    let Ok(state) = (&mut v_state).get(entity_id) else {
        return Effect::NoEffect;
    };

    let current = ModelFrame {
        index: state.frame,
        reverse: state.animation_state.contains(TweqAnimationState::REVERSE),
    };

    let Some(next) = advance_frame_index(config, current) else {
        return Effect::NoEffect;
    };

    state.frame = next.index;
    state
        .animation_state
        .set(TweqAnimationState::REVERSE, next.reverse);

    Effect::ChangeModel {
        entity_id,
        model_name: config.model_names[next.index].to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(
        model_count: usize,
        animation_config: TweqAnimationConfig,
        halt: TweqHalt,
    ) -> PropTweqModelConfig {
        PropTweqModelConfig {
            animation_config,
            halt,
            model_names: (0..model_count).map(|i| format!("model{i}")).collect(),
        }
    }

    fn frame(index: usize, reverse: bool) -> ModelFrame {
        ModelFrame { index, reverse }
    }

    /// Walk `steps` frames from the start of the tweq, collecting each index.
    fn walk(config: &PropTweqModelConfig, steps: usize) -> Vec<Option<usize>> {
        let mut current = frame(0, false);
        (0..steps)
            .map(|_| match advance_frame_index(config, current) {
                Some(next) => {
                    current = next;
                    Some(next.index)
                }
                None => None,
            })
            .collect()
    }

    #[test]
    fn code_art_frames_bounce_off_the_last_model() {
        // The authored Rec deck frames: 6 models, no flags, halt Continue.
        let config = config(6, TweqAnimationConfig::empty(), TweqHalt::Continue);

        assert_eq!(
            walk(&config, 12),
            [1, 2, 3, 4, 5, 4, 3, 2, 1, 0, 1, 2].map(Some)
        );
    }

    #[test]
    fn wrapping_tweq_restarts_at_the_first_model() {
        let config = config(3, TweqAnimationConfig::WRAP, TweqHalt::Continue);

        assert_eq!(walk(&config, 5), [1, 2, 0, 1, 2].map(Some));
    }

    #[test]
    fn non_continue_halt_stops_at_the_edge() {
        let config = config(3, TweqAnimationConfig::empty(), TweqHalt::StopTweq);

        assert_eq!(walk(&config, 4), [Some(1), Some(2), None, None]);
    }

    #[test]
    fn one_bounce_halts_only_after_reversing() {
        let config = config(3, TweqAnimationConfig::ONEBOUNCE, TweqHalt::StopTweq);

        // Bounces off the top, runs back down, then halts at the bottom edge.
        assert_eq!(walk(&config, 5), [Some(1), Some(2), Some(1), Some(0), None]);
    }

    #[test]
    fn reverse_direction_steps_back_toward_the_first_model() {
        let config = config(6, TweqAnimationConfig::empty(), TweqHalt::Continue);

        assert_eq!(
            advance_frame_index(&config, frame(4, true)),
            Some(frame(3, true))
        );
    }

    #[test]
    fn a_single_model_has_nowhere_to_advance() {
        let config = config(1, TweqAnimationConfig::empty(), TweqHalt::Continue);

        assert_eq!(
            advance_frame_index(&config, frame(0, false)),
            Some(frame(0, true))
        );
    }

    #[test]
    fn an_empty_model_list_never_advances() {
        let config = config(0, TweqAnimationConfig::empty(), TweqHalt::Continue);

        assert_eq!(advance_frame_index(&config, frame(0, false)), None);
    }
}
