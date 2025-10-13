use std::time::Duration;

use cgmath::{Deg, Quaternion, Rotation3};
use dark::properties::{
    PropPosition, PropTweqDeleteConfig, PropTweqDeleteState, PropTweqEmitterConfig,
    PropTweqEmitterState, PropTweqRotateState, TweqAnimationState, TweqHalt,
};
use shipyard::{EntityId, Get, IntoIter, IntoWithId, UniqueView, UniqueViewMut, View, ViewMut};

use crate::{mission::EffectQueue, scripts::Effect, time::Time, util::vec3_to_point3};

///
/// run_tweq
///
/// Runs all tweq components
///
pub fn run_tweq(
    u_time: UniqueView<Time>,
    v_prop_position: View<PropPosition>,
    v_tweq_rotate_state: View<PropTweqRotateState>,
    mut v_tweq_emit_state: ViewMut<PropTweqEmitterState>,
    mut v_tweq_emit_config: ViewMut<PropTweqEmitterConfig>,
    mut v_tweq_delete_state: ViewMut<PropTweqDeleteState>,
    mut v_tweq_delete_config: ViewMut<PropTweqDeleteConfig>,
    mut effects: UniqueViewMut<EffectQueue>,
) {
    for (id, tweq) in v_tweq_rotate_state.iter().with_id() {
        if tweq.animation_state.contains(TweqAnimationState::ON) {
            effects.push(Effect::SetRotation {
                entity_id: id,
                rotation: Quaternion::from_angle_y(Deg(u_time.total.as_secs_f32() * 20.0)),
            });
        }
    }

    // Run emit tweq
    for (_id, (tweq_state, tweq_config, position)) in (
        &mut v_tweq_emit_state,
        &mut v_tweq_emit_config,
        &v_prop_position,
    )
        .iter()
        .with_id()
    {
        if tweq_state.animation_state.contains(TweqAnimationState::ON) {
            let time_since_last_event = tweq_state.time_since_last_event + u_time.elapsed;
            tweq_state.time_since_last_event = time_since_last_event;

            if time_since_last_event > tweq_config.rate
                && tweq_state.num_iterations < tweq_config.max_frames
            {
                tweq_state.num_iterations += 1;
                tweq_state.time_since_last_event = Duration::from_secs(0);
                effects.push(Effect::CreateEntityByTemplateName {
                    template_name: "HE Explosion".to_string(),
                    position: vec3_to_point3(position.position),
                    orientation: position.rotation,
                });
            }

            // Did we finish emitting frames?
            if tweq_state.num_iterations >= tweq_config.max_frames {
                tweq_state.animation_state = tweq_state
                    .animation_state
                    .difference(TweqAnimationState::ON);
            }
        }
    }

    // Run destroy tweq
    for (id, (tweq_state, tweq_config)) in (&mut v_tweq_delete_state, &mut v_tweq_delete_config)
        .iter()
        .with_id()
    {
        if tweq_state.animation_state.contains(TweqAnimationState::ON) {
            let time_since_last_event = tweq_state.time_since_last_event + u_time.elapsed;
            tweq_state.time_since_last_event = time_since_last_event;

            if time_since_last_event > tweq_config.rate {
                match tweq_config.halt {
                    TweqHalt::SlayObj => effects.push(Effect::SlayEntity { entity_id: id }),
                    _ => effects.push(Effect::DestroyEntity { entity_id: id }),
                }
            }
        }
    }
}

pub fn turn_on_tweqs(
    entity_id: EntityId,
    mut v_tweq_emit_state: ViewMut<PropTweqEmitterState>,
    mut v_tweq_delete_state: ViewMut<PropTweqDeleteState>,
) {
    if let Ok(tweq_state) = (&mut v_tweq_emit_state).get(entity_id) {
        tweq_state.animation_state.insert(TweqAnimationState::ON);
        tweq_state.time_since_last_event = Duration::from_secs(0);
    }

    if let Ok(tweq_state) = (&mut v_tweq_delete_state).get(entity_id) {
        tweq_state.animation_state.insert(TweqAnimationState::ON);
        tweq_state.time_since_last_event = Duration::from_secs(0);
    }
}

pub fn turn_off_tweqs(
    entity_id: EntityId,
    mut v_tweq_emit_state: ViewMut<PropTweqEmitterState>,
    mut v_tweq_delete_state: ViewMut<PropTweqDeleteState>,
) {
    if let Ok(tweq_state) = (&mut v_tweq_emit_state).get(entity_id) {
        tweq_state.animation_state.remove(TweqAnimationState::ON);
    }

    if let Ok(tweq_state) = (&mut v_tweq_delete_state).get(entity_id) {
        tweq_state.animation_state.remove(TweqAnimationState::ON);
    }
}
