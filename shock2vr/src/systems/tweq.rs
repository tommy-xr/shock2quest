use std::time::Duration;

use cgmath::{Deg, Quaternion, Rotation3, Vector3, vec3};
use dark::{
    SCALE_FACTOR,
    properties::{
        PropPosition, PropScale, PropTweqDeleteConfig, PropTweqDeleteState, PropTweqEmitterConfig,
        PropTweqEmitterState, PropTweqRotateConfig, PropTweqRotateState, PropTweqScaleConfig,
        PropTweqScaleState, TweqAnimationConfig, TweqAnimationState, TweqAxisLimits, TweqHalt,
    },
};

/// The rotate tweq axis the port spins: the original's third axis is heading,
/// which its up axis maps onto our Y.
const HEADING_AXIS: usize = 2;

/// Spin rate for a rotating object with no authored rotate config, preserving
/// the rate the port used before the config was parsed.
const DEFAULT_SPIN_DEGREES_PER_SECOND: f32 = 20.0;
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
    v_tweq_rotate_config: View<PropTweqRotateConfig>,
    mut v_tweq_emit_state: ViewMut<PropTweqEmitterState>,
    mut v_tweq_emit_config: ViewMut<PropTweqEmitterConfig>,
    mut v_tweq_delete_state: ViewMut<PropTweqDeleteState>,
    mut v_tweq_delete_config: ViewMut<PropTweqDeleteConfig>,
    mut effects: UniqueViewMut<EffectQueue>,
) {
    for (id, (tweq, position)) in (&v_tweq_rotate_state, &v_prop_position).iter().with_id() {
        if tweq.animation_state.contains(TweqAnimationState::ON) {
            // Authored degrees per second for the heading axis when the object
            // carries a rotate config. The rate matters wherever the spin has
            // a consequence beyond looks: a Tweq emitter's facing aims what it
            // launches, so the wrong rate collapses an emitted volley's fan.
            // Objects with no authored config keep the previous fixed rate.
            let degrees_per_second = v_tweq_rotate_config
                .get(id)
                .map(|config| config.axes[HEADING_AXIS].rate)
                .unwrap_or(DEFAULT_SPIN_DEGREES_PER_SECOND);
            effects.push(Effect::SetRotation {
                entity_id: id,
                // Advance from the current pose; absolute world-time yaw erased
                // authored pitch/roll (including a sideways ejected casing).
                rotation: Quaternion::from_angle_y(Deg(
                    u_time.elapsed.as_secs_f32() * degrees_per_second
                )) * position.rotation,
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

/// What a tweq step asks of its object once an axis reaches a limit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TweqOutcome {
    /// `TWEQ_STATUS_QUO`: keep animating (also a `Continue` halt).
    Continue,
    Stop,
    Destroy,
    Slay,
}

fn halt_outcome(halt: &TweqHalt) -> TweqOutcome {
    match halt {
        TweqHalt::DestroyObject => TweqOutcome::Destroy,
        TweqHalt::SlayObj => TweqOutcome::Slay,
        // Removing the tweq leaves the object as stopping it does.
        TweqHalt::StopTweq | TweqHalt::RemoveProp => TweqOutcome::Stop,
        TweqHalt::Continue => TweqOutcome::Continue,
    }
}

/// One axis of a vector tweq, ported from Dark's `processTweqAxis`
/// (`tweqctrl.cpp`): the rate is per 100 ms, added (or multiplied with
/// `TWEQ_CC_MUL`); past a limit the value wraps or bounces (reversing the
/// axis), and the halt applies at the end of a lap - immediately, before the
/// value clips, for a destroying halt. Curve jitter is not ported.
fn advance_tweq_axis(
    value: f32,
    limits: &TweqAxisLimits,
    animation: TweqAnimationConfig,
    multiply: bool,
    halt: &TweqHalt,
    axis_state: &mut TweqAnimationState,
    ms: f32,
) -> (f32, TweqOutcome) {
    let rate = if axis_state.contains(TweqAnimationState::REVERSE) {
        -limits.rate
    } else {
        limits.rate
    };
    let next = if multiply {
        value * rate
    } else {
        value + rate * ms / 100.0
    };
    if animation.contains(TweqAnimationConfig::NOLIMT) {
        return (next, TweqOutcome::Continue);
    }
    let clipped = if next < limits.low {
        Some((limits.low, limits.high))
    } else if next > limits.high {
        Some((limits.high, limits.low))
    } else {
        None
    };
    let Some((edge, opposite)) = clipped else {
        return (next, TweqOutcome::Continue);
    };
    // A one-bounce tweq only ends once it is on its way back.
    let is_end = !animation.contains(TweqAnimationConfig::ONEBOUNCE)
        || axis_state.contains(TweqAnimationState::REVERSE);
    let outcome = halt_outcome(halt);
    if is_end && matches!(outcome, TweqOutcome::Destroy | TweqOutcome::Slay) {
        return (value, outcome);
    }
    let value = if animation.contains(TweqAnimationConfig::WRAP) {
        opposite
    } else {
        axis_state.toggle(TweqAnimationState::REVERSE);
        edge
    };
    (
        value,
        if is_end {
            outcome
        } else {
            TweqOutcome::Continue
        },
    )
}

/// Dark's `processTweqVector`: advances every axis with a non-zero rate. The
/// primary axis (or, with none, any axis) decides the tweq's outcome.
fn advance_scale_tweq(
    scale: Vector3<f32>,
    config: &PropTweqScaleConfig,
    state: &mut PropTweqScaleState,
    ms: f32,
) -> (Vector3<f32>, TweqOutcome) {
    let primary = usize::from(config.primary_axis);
    if primary > 0 {
        state.axis_states[primary - 1] = state.animation_state;
    }
    let mut values = [scale.x, scale.y, scale.z];
    let mut outcome = TweqOutcome::Continue;
    for axis in 0..3 {
        let limits = &config.axes[axis];
        if limits.rate == 0.0 {
            continue;
        }
        let (value, axis_outcome) = advance_tweq_axis(
            values[axis],
            limits,
            config.animation_config,
            config.multiply,
            &config.halt,
            &mut state.axis_states[axis],
            ms,
        );
        values[axis] = value;
        if primary == 0 || axis == primary - 1 {
            outcome = axis_outcome;
        }
        if outcome != TweqOutcome::Continue {
            break;
        }
    }
    // ...and back, so the primary axis's bounce direction persists.
    if primary > 0 {
        state.animation_state = state.axis_states[primary - 1];
    }
    (vec3(values[0], values[1], values[2]), outcome)
}

/// `PropScale` holds the runtime basis (`read_vec3`: Dark x, y, z become
/// -x, z, y), so an authored scale carries a negative x while a default one
/// does not. Tweq axes are in Dark's order and work on the x magnitude; the
/// write-back keeps the entity's existing x sign, so the transform matches the
/// one it was created with.
fn dark_scale_from_runtime(scale: Vector3<f32>) -> Vector3<f32> {
    vec3(scale.x.abs(), scale.z, scale.y)
}

fn runtime_scale_from_dark(scale: Vector3<f32>, current: Vector3<f32>) -> Vector3<f32> {
    vec3(scale.x.copysign(current.x), scale.z, scale.y)
}

/// Runs scale tweqs (`P$CfgTweqScale`/`P$StTweqScale`): explosion sprites at
/// their authored 2x/3x, shockwave rings growing from nothing, worm goo
/// breathing. Dark only advances a non-`SIM` tweq while its object is near or
/// on screen; this advances every running one.
pub fn run_scale_tweq(
    u_time: UniqueView<Time>,
    v_scale: View<PropScale>,
    v_config: View<PropTweqScaleConfig>,
    mut v_state: ViewMut<PropTweqScaleState>,
    mut effects: UniqueViewMut<EffectQueue>,
) {
    let ms = u_time.elapsed.as_secs_f32() * 1000.0;
    if ms <= 0.0 {
        return;
    }
    for (id, (state, config)) in (&mut v_state, &v_config).iter().with_id() {
        if !state.animation_state.contains(TweqAnimationState::ON) {
            continue;
        }
        let current = v_scale
            .get(id)
            .map(|scale| scale.0)
            .unwrap_or(vec3(1.0, 1.0, 1.0));
        let (dark_scale, outcome) =
            advance_scale_tweq(dark_scale_from_runtime(current), config, state, ms);
        let scale = runtime_scale_from_dark(dark_scale, current);
        match outcome {
            TweqOutcome::Destroy => effects.push(Effect::DestroyEntity { entity_id: id }),
            TweqOutcome::Slay => effects.push(Effect::SlayEntity { entity_id: id }),
            TweqOutcome::Stop | TweqOutcome::Continue => {
                if outcome == TweqOutcome::Stop {
                    state.animation_state.remove(TweqAnimationState::ON);
                }
                // A tweq settled on its limit (the explosions' fixed 2x)
                // has nothing to apply each frame.
                if scale != current || v_scale.get(id).is_err() {
                    effects.push(Effect::SetScale {
                        entity_id: id,
                        scale,
                    });
                }
            }
        }
    }
}

pub fn turn_on_tweqs(
    entity_id: EntityId,
    mut v_tweq_emit_state: ViewMut<PropTweqEmitterState>,
    mut v_tweq_delete_state: ViewMut<PropTweqDeleteState>,
    mut v_tweq_scale_state: ViewMut<PropTweqScaleState>,
) {
    if let Ok(tweq_state) = (&mut v_tweq_scale_state).get(entity_id) {
        tweq_state.animation_state.insert(TweqAnimationState::ON);
    }

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
    mut v_tweq_scale_state: ViewMut<PropTweqScaleState>,
) {
    if let Ok(tweq_state) = (&mut v_tweq_scale_state).get(entity_id) {
        tweq_state.animation_state.remove(TweqAnimationState::ON);
    }

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

    mod scale {
        use cgmath::vec3;
        use dark::properties::{
            PropTweqScaleConfig, PropTweqScaleState, TweqAnimationConfig, TweqAnimationState,
            TweqAxisLimits, TweqHalt,
        };

        use super::super::{TweqOutcome, advance_scale_tweq};

        fn config(rate: f32, low: f32, high: f32, halt: TweqHalt) -> PropTweqScaleConfig {
            let axis = TweqAxisLimits { rate, low, high };
            PropTweqScaleConfig {
                animation_config: TweqAnimationConfig::SIM,
                halt,
                multiply: false,
                axes: [axis; 3],
                primary_axis: 0,
            }
        }

        fn running() -> PropTweqScaleState {
            PropTweqScaleState {
                animation_state: TweqAnimationState::ON,
                axis_states: [TweqAnimationState::ON; 3],
            }
        }

        #[test]
        fn a_fixed_scale_tweq_settles_on_its_limit() {
            // The Standard Explosion: rate 1000, low = high = 2.
            let config = config(1000.0, 2.0, 2.0, TweqHalt::Continue);
            let mut state = running();
            let mut scale = vec3(1.0, 1.0, 1.0);
            for _ in 0..3 {
                let (next, outcome) = advance_scale_tweq(scale, &config, &mut state, 16.0);
                assert_eq!(outcome, TweqOutcome::Continue);
                scale = next;
                assert_eq!(scale, vec3(2.0, 2.0, 2.0));
            }
        }

        #[test]
        fn a_shockwave_grows_at_its_rate_and_bounces_off_the_top() {
            // Rate is per 100 ms: 1.0 grows by 10 per second.
            let config = config(1.0, 0.0, 3.0, TweqHalt::Continue);
            let mut state = running();
            let (scale, _) = advance_scale_tweq(vec3(0.0, 0.0, 0.0), &config, &mut state, 100.0);
            assert_eq!(scale, vec3(1.0, 1.0, 1.0));
            let (scale, _) = advance_scale_tweq(scale, &config, &mut state, 300.0);
            assert_eq!(scale, vec3(3.0, 3.0, 3.0), "clipped to the top");
            assert!(state.axis_states[0].contains(TweqAnimationState::REVERSE));
            let (scale, _) = advance_scale_tweq(scale, &config, &mut state, 100.0);
            assert_eq!(scale, vec3(2.0, 2.0, 2.0), "and shrinks back");
        }

        #[test]
        fn a_slaying_halt_fires_when_the_limit_is_reached() {
            // many.mis' brain balloon inflates to 5 and is slain.
            let config = config(0.5, 0.0, 5.0, TweqHalt::SlayObj);
            let mut state = running();
            let (scale, outcome) =
                advance_scale_tweq(vec3(4.9, 4.9, 4.9), &config, &mut state, 100.0);
            assert_eq!(outcome, TweqOutcome::Slay);
            assert_eq!(
                scale,
                vec3(4.9, 4.9, 4.9),
                "a destroying halt does not clip"
            );
        }

        #[test]
        fn a_stopping_halt_clips_and_stops() {
            let config = config(1.0, 0.0, 2.0, TweqHalt::StopTweq);
            let mut state = running();
            let (scale, outcome) =
                advance_scale_tweq(vec3(1.5, 1.5, 1.5), &config, &mut state, 100.0);
            assert_eq!(outcome, TweqOutcome::Stop);
            assert_eq!(scale.x, 2.0);
        }

        #[test]
        fn dark_axes_map_onto_the_runtime_basis() {
            use super::super::{dark_scale_from_runtime, runtime_scale_from_dark};
            // Dark's up axis (z) is the runtime y.
            let runtime = runtime_scale_from_dark(vec3(1.0, 2.0, 3.0), vec3(1.0, 1.0, 1.0));
            assert_eq!(runtime, vec3(1.0, 3.0, 2.0));
            assert_eq!(dark_scale_from_runtime(runtime), vec3(1.0, 2.0, 3.0));
            // An authored P$Scale carries read_vec3's negated x, and keeps it.
            let authored = vec3(-2.0, 1.0, 1.0);
            assert_eq!(dark_scale_from_runtime(authored).x, 2.0);
            assert_eq!(
                runtime_scale_from_dark(vec3(3.0, 1.0, 1.0), authored).x,
                -3.0
            );
        }

        #[test]
        fn a_primary_axis_keeps_its_bounce_direction() {
            let mut config = config(1.0, 0.0, 2.0, TweqHalt::Continue);
            config.primary_axis = 1;
            let mut state = running();
            let (scale, _) = advance_scale_tweq(vec3(1.5, 1.5, 1.5), &config, &mut state, 100.0);
            assert_eq!(scale.x, 2.0);
            let (scale, _) = advance_scale_tweq(scale, &config, &mut state, 100.0);
            assert_eq!(scale.x, 1.0, "the primary axis heads back down");
        }

        #[test]
        fn a_wrapping_tweq_jumps_to_the_other_limit() {
            let mut config = config(1.0, 0.0, 2.0, TweqHalt::Continue);
            config.animation_config |= TweqAnimationConfig::WRAP;
            let mut state = running();
            let (scale, _) = advance_scale_tweq(vec3(1.5, 1.5, 1.5), &config, &mut state, 100.0);
            assert_eq!(scale.x, 0.0);
            assert!(!state.axis_states[0].contains(TweqAnimationState::REVERSE));
        }

        #[test]
        fn a_one_bounce_tweq_halts_only_on_the_way_back() {
            let mut config = config(1.0, 0.0, 2.0, TweqHalt::StopTweq);
            config.animation_config |= TweqAnimationConfig::ONEBOUNCE;
            let mut state = running();
            let (scale, outcome) =
                advance_scale_tweq(vec3(1.5, 1.5, 1.5), &config, &mut state, 100.0);
            assert_eq!(outcome, TweqOutcome::Continue, "bounces off the top");
            let (_, outcome) = advance_scale_tweq(scale, &config, &mut state, 2500.0);
            assert_eq!(outcome, TweqOutcome::Stop, "stops at the bottom");
        }

        #[test]
        fn unused_axes_are_left_alone() {
            // Worm goo breathes on X/Y only (Z rate 0).
            let mut config = config(0.002, 1.0, 1.05, TweqHalt::Continue);
            config.axes[2] = TweqAxisLimits {
                rate: 0.0,
                low: 0.0,
                high: 0.0,
            };
            let mut state = running();
            let (scale, _) = advance_scale_tweq(vec3(1.0, 1.0, 1.0), &config, &mut state, 1000.0);
            assert!((scale.x - 1.02).abs() < 1e-6);
            assert_eq!(scale.z, 1.0);
        }
    }
}
