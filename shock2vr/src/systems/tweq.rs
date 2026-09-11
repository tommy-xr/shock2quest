use std::time::Duration;

use cgmath::{Deg, Quaternion, Rotation3};
use dark::{
    SCALE_FACTOR,
    properties::{
        PropPosition, PropTweqDeleteConfig, PropTweqDeleteState, PropTweqEmitterConfig,
        PropTweqEmitterState, PropTweqRotateState, TweqAnimationState, TweqHalt,
    },
};
use shipyard::{EntityId, Get, IntoIter, IntoWithId, UniqueView, UniqueViewMut, View, ViewMut};

use crate::{
    mission::{EffectQueue, entity_creator::CreateEntityOptions},
    scripts::Effect,
    time::Time,
    util::vec3_to_point3,
};

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
    for (id, (tweq, position)) in (&v_tweq_rotate_state, &v_prop_position).iter().with_id() {
        if tweq.animation_state.contains(TweqAnimationState::ON) {
            effects.push(Effect::SetRotation {
                entity_id: id,
                // Advance from the current pose; absolute world-time yaw erased
                // authored pitch/roll (including a sideways ejected casing).
                rotation: Quaternion::from_angle_y(Deg(u_time.elapsed.as_secs_f32() * 20.0))
                    * position.rotation,
            });
        }
    }

    // Run emit tweq
    for (id, (tweq_state, tweq_config, position)) in (
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
                // `read_vec3` has already changed the Dark vector into the
                // runtime basis. Relative Velocity then applies the emitter's
                // authored facing, exactly as Dark's TWEQ_MC_RELVEL path does.
                // `angle_random` is deliberately deferred: the stock Ops4
                // emitters author zero, while non-zero values are Dark fixed-
                // angle ranges and need deterministic RNG/save semantics.
                let authored_velocity = if tweq_config.relative_velocity {
                    position.rotation * tweq_config.velocity
                } else {
                    tweq_config.velocity
                };
                effects.push(Effect::CreateEntityByTemplateName {
                    source_entity_id: id,
                    template_name: tweq_config.emit_what.clone(),
                    position: vec3_to_point3(position.position),
                    orientation: position.rotation,
                    initial_velocity: authored_velocity / SCALE_FACTOR,
                    options: CreateEntityOptions {
                        launch_projectile: true,
                        ..CreateEntityOptions::default()
                    },
                });
            }

            // Did we finish emitting frames?
            if tweq_state.num_iterations >= tweq_config.max_frames {
                tweq_state.animation_state = tweq_state
                    .animation_state
                    .difference(TweqAnimationState::ON);
                if matches!(tweq_config.halt, TweqHalt::DestroyObject) {
                    effects.push(Effect::DestroyEntity { entity_id: id });
                }
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

#[cfg(test)]
mod tests {
    use cgmath::{Deg, InnerSpace, Quaternion, Rotation3, vec3};
    use dark::properties::{
        PropPosition, PropTweqDeleteConfig, PropTweqDeleteState, PropTweqEmitterConfig,
        PropTweqEmitterState, PropTweqRotateState, TweqAnimationConfig, TweqAnimationState,
        TweqHalt,
    };
    use shipyard::{EntityId, Get, UniqueViewMut, View, World};

    use super::run_tweq;
    use crate::{mission::EffectQueue, scripts::Effect, time::Time};

    fn world_with_ops4_emitter(
        velocity: cgmath::Vector3<f32>,
        rotation: Quaternion<f32>,
        relative_velocity: bool,
        max_frames: u32,
    ) -> (World, EntityId) {
        let mut world = World::new();
        world.add_unique(Time {
            elapsed: std::time::Duration::from_millis(501),
            total: std::time::Duration::from_millis(501),
        });
        world.add_unique(EffectQueue::default());
        let emitter = world.add_entity((
            PropTweqEmitterState {
                animation_state: TweqAnimationState::ON,
                time_since_last_event: std::time::Duration::ZERO,
                num_iterations: 0,
            },
            PropTweqEmitterConfig {
                animation_config: TweqAnimationConfig::SIM,
                halt: TweqHalt::DestroyObject,
                relative_velocity,
                rate: std::time::Duration::from_millis(500),
                max_frames,
                emit_what: "gRuB".to_owned(),
                velocity,
                angle_random: vec3(0.0, 0.0, 0.0),
            },
            PropPosition {
                position: vec3(31.275366, -14.658457, -105.01082),
                cell: u16::MAX,
                rotation,
            },
        ));

        // Register the other Tweq component storages borrowed by `run_tweq`.
        world.add_entity(PropTweqRotateState {
            animation_state: TweqAnimationState::empty(),
            axis1_animation_state: TweqAnimationState::empty(),
            axis2_animation_state: TweqAnimationState::empty(),
            axis3_animation_state: TweqAnimationState::empty(),
        });
        world.add_entity((
            PropTweqDeleteState {
                animation_state: TweqAnimationState::empty(),
                time_since_last_event: std::time::Duration::ZERO,
                num_iterations: 0,
            },
            PropTweqDeleteConfig {
                animation_config: TweqAnimationConfig::SIM,
                halt: TweqHalt::StopTweq,
                rate: std::time::Duration::from_secs(1),
            },
        ));
        (world, emitter)
    }

    #[test]
    fn rotate_tweq_preserves_launch_tilt_and_uses_elapsed_time() {
        let tilt = Quaternion::from_angle_x(Deg(90.0));
        let (mut world, emitter) = world_with_ops4_emitter(vec3(0.0, 0.0, 0.0), tilt, false, 0);
        world.add_component(
            emitter,
            PropTweqRotateState {
                animation_state: TweqAnimationState::ON,
                axis1_animation_state: TweqAnimationState::ON,
                axis2_animation_state: TweqAnimationState::empty(),
                axis3_animation_state: TweqAnimationState::empty(),
            },
        );
        world.borrow::<UniqueViewMut<Time>>().unwrap().total = std::time::Duration::from_secs(1000);
        world.run(run_tweq);
        let effects = world
            .borrow::<UniqueViewMut<EffectQueue>>()
            .unwrap()
            .flush();
        let rotation = effects
            .iter()
            .find_map(|effect| match effect {
                Effect::SetRotation {
                    entity_id,
                    rotation,
                } if *entity_id == emitter => Some(*rotation),
                _ => None,
            })
            .unwrap();
        let expected = Quaternion::from_angle_y(Deg(0.501 * 20.0)) * tilt;
        assert!((rotation - expected).magnitude2() < 1.0e-6);
    }

    #[test]
    fn ops4_emitter_682_uses_authored_template_world_velocity_and_destroy_completion() {
        let (world, _) = world_with_ops4_emitter(
            vec3(-25.0, 0.0, 0.0),
            Quaternion::from_angle_y(Deg(-90.0)),
            false,
            1,
        );

        world.run(run_tweq);
        let effects = world
            .borrow::<UniqueViewMut<EffectQueue>>()
            .unwrap()
            .flush();

        assert!(matches!(
            effects.first(),
            Some(Effect::CreateEntityByTemplateName {
                template_name,
                initial_velocity,
                ..
            }) if template_name == "gRuB"
                && (*initial_velocity - vec3(-10.0, 0.0, 0.0)).magnitude2() < 1.0e-6
        ));
        assert!(matches!(effects.get(1), Some(Effect::DestroyEntity { .. })));
    }

    #[test]
    fn ops4_emitter_689_keeps_its_differently_rotated_world_velocity() {
        let (world, _) = world_with_ops4_emitter(
            vec3(10.0, 0.0, 0.0),
            Quaternion::from_angle_y(Deg(180.0)),
            false,
            5,
        );

        world.run(run_tweq);
        let effects = world
            .borrow::<UniqueViewMut<EffectQueue>>()
            .unwrap()
            .flush();

        assert!(matches!(
            effects.first(),
            Some(Effect::CreateEntityByTemplateName {
                initial_velocity,
                ..
            }) if (*initial_velocity - vec3(4.0, 0.0, 0.0)).magnitude2() < 1.0e-6
        ));
        assert_eq!(effects.len(), 1);
    }

    #[test]
    fn relative_velocity_rotates_the_dark_converted_vector_by_emitter_facing() {
        let (world, _) = world_with_ops4_emitter(
            vec3(-25.0, 0.0, 0.0),
            Quaternion::from_angle_y(Deg(-90.0)),
            true,
            3,
        );

        world.run(run_tweq);
        let effects = world
            .borrow::<UniqueViewMut<EffectQueue>>()
            .unwrap()
            .flush();

        assert!(matches!(
            effects.first(),
            Some(Effect::CreateEntityByTemplateName {
                initial_velocity,
                ..
            }) if (*initial_velocity - vec3(0.0, 0.0, -10.0)).magnitude2() < 1.0e-6
        ));
    }

    #[test]
    fn mid_burst_state_roundtrip_resumes_only_the_remaining_emissions() {
        let (world, emitter) = world_with_ops4_emitter(
            vec3(-25.0, 0.0, 0.0),
            Quaternion::from_angle_y(Deg(-90.0)),
            false,
            3,
        );

        world.run(run_tweq);
        world
            .borrow::<UniqueViewMut<EffectQueue>>()
            .unwrap()
            .flush();
        world.borrow::<UniqueViewMut<Time>>().unwrap().elapsed =
            std::time::Duration::from_millis(250);
        world.run(run_tweq);
        assert!(
            world
                .borrow::<UniqueViewMut<EffectQueue>>()
                .unwrap()
                .flush()
                .is_empty()
        );

        let saved_state = world
            .borrow::<View<PropTweqEmitterState>>()
            .unwrap()
            .get(emitter)
            .unwrap()
            .clone();
        let serialized = serde_json::to_string(&saved_state).unwrap();
        let restored_state: PropTweqEmitterState = serde_json::from_str(&serialized).unwrap();
        assert_eq!(restored_state.num_iterations, 1);
        assert_eq!(
            restored_state.time_since_last_event,
            std::time::Duration::from_millis(250)
        );

        let (mut restored_world, restored_emitter) = world_with_ops4_emitter(
            vec3(-25.0, 0.0, 0.0),
            Quaternion::from_angle_y(Deg(-90.0)),
            false,
            3,
        );
        restored_world.add_component(restored_emitter, restored_state);
        restored_world
            .borrow::<UniqueViewMut<Time>>()
            .unwrap()
            .elapsed = std::time::Duration::from_millis(251);

        restored_world.run(run_tweq);
        let effects = restored_world
            .borrow::<UniqueViewMut<EffectQueue>>()
            .unwrap()
            .flush();
        assert_eq!(effects.len(), 1);
        assert!(matches!(
            effects.first(),
            Some(Effect::CreateEntityByTemplateName { .. })
        ));
        assert_eq!(
            restored_world
                .borrow::<View<PropTweqEmitterState>>()
                .unwrap()
                .get(restored_emitter)
                .unwrap()
                .num_iterations,
            2
        );
    }
}
