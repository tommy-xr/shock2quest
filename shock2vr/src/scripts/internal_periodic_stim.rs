use std::time::Duration;

use cgmath::{Transform, point3};
use dark::properties::{Link, StimPropagator, StimSourceLifecycle, StimSourceOptions};
use serde::{Deserialize, Serialize};
use shipyard::{EntityId, Get, View, World};

use crate::physics::PhysicsWorld;
use crate::runtime_props::RuntimePropTransform;
use crate::time::Time;
use crate::util::point3_to_vec3;

use super::{
    Effect, Script, ScriptRestoreContext, ScriptState, ScriptStateError,
    script_util::get_all_links_with_template,
};

const SCRIPT_STATE_KEY: &str = "shock2vr.periodic_stim";

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
struct PeriodicSourceState {
    stim_template_id: i32,
    intensity: f32,
    radius: f32,
    lifecycle: StimSourceLifecycle,
    age: Duration,
    next_firing: Duration,
    firings: u32,
}

impl PeriodicSourceState {
    fn new(stim_template_id: i32, options: StimSourceOptions) -> Option<Self> {
        let StimPropagator::Radius { radius } = options.propagator else {
            return None;
        };
        // Zero is valid for a one-shot source (e.g. Droid Fusion). It cannot
        // schedule repeated firings: inventing a 1 ms cadence produces a
        // damage burst and an unbounded catch-up loop after a long frame.
        if options.lifecycle.period.is_zero()
            && (options.lifecycle.no_max_firings || options.lifecycle.max_firings > 1)
        {
            tracing::warn!(stim_template_id, "Ignoring zero-period repeating stimulus");
            return None;
        }
        Some(Self {
            stim_template_id,
            intensity: options.intensity,
            radius,
            lifecycle: options.lifecycle,
            age: Duration::ZERO,
            next_firing: Duration::ZERO,
            firings: 0,
        })
    }

    fn can_fire(&self) -> bool {
        self.lifecycle.no_max_firings
            || u32::try_from(self.lifecycle.max_firings).is_ok_and(|maximum| self.firings < maximum)
    }

    /// Advance the source and return the intensity of every firing whose time
    /// was crossed, plus whether this firing completed a destroy-on-finish
    /// lifecycle. Sources fire at birth, then once per authored period.
    fn advance(&mut self, elapsed: Duration) -> (Vec<f32>, bool) {
        self.age += elapsed;
        let period = self.lifecycle.period;
        let mut intensities = Vec::new();

        while self.next_firing <= self.age && self.can_fire() {
            intensities.push(self.intensity + self.firings as f32 * self.lifecycle.intensity_slope);
            self.firings += 1;
            self.next_firing += period;
        }

        let completed_now = !intensities.is_empty()
            && self.lifecycle.destroy_on_completion
            && !self.lifecycle.no_max_firings
            && u32::try_from(self.lifecycle.max_firings)
                .is_ok_and(|maximum| self.firings >= maximum);
        (intensities, completed_now)
    }
}

/// Runs inherited non-explosion radius stimulus sources according to their
/// authored periodic lifecycle (electrical sparks, swarms, Rad Burst, etc.).
pub struct InternalPeriodicStim {
    sources: Vec<PeriodicSourceState>,
}

impl InternalPeriodicStim {
    pub fn new() -> Self {
        Self {
            sources: Vec::new(),
        }
    }
}

impl Script for InternalPeriodicStim {
    fn script_state_key(&self) -> Option<&'static str> {
        Some(SCRIPT_STATE_KEY)
    }

    fn save_state(&self) -> Result<ScriptState, ScriptStateError> {
        ScriptState::encode(1, &self.sources, SCRIPT_STATE_KEY)
    }

    fn restore_state(
        &mut self,
        state: &ScriptState,
        _context: &ScriptRestoreContext<'_>,
    ) -> Result<(), ScriptStateError> {
        // Sources contain stable stimulus template IDs, not runtime handles.
        // Hydrated scripts skip initialize, preserving cadence and budget.
        self.sources = state.decode(1, SCRIPT_STATE_KEY)?;
        Ok(())
    }

    fn initialize(&mut self, entity_id: EntityId, world: &World) -> Effect {
        self.sources = get_all_links_with_template(world, entity_id, |link| match link {
            Link::StimSource(options) => Some(*options),
            _ => None,
        })
        .into_iter()
        .filter_map(|(stim_template_id, options)| {
            PeriodicSourceState::new(stim_template_id, options)
        })
        .collect();
        Effect::NoEffect
    }

    fn update(
        &mut self,
        entity_id: EntityId,
        world: &World,
        _physics: &PhysicsWorld,
        time: &Time,
    ) -> Effect {
        let center = world
            .borrow::<View<RuntimePropTransform>>()
            .ok()
            .and_then(|transforms| {
                transforms.get(entity_id).ok().map(|transform| {
                    point3_to_vec3(transform.0.transform_point(point3(0.0, 0.0, 0.0)))
                })
            });

        let mut effects = Vec::new();
        let mut destroy_on_completion = false;
        for source in &mut self.sources {
            let (intensities, completed) = source.advance(time.elapsed);
            destroy_on_completion |= completed;
            if let Some(center) = center {
                effects.extend(intensities.into_iter().map(|intensity| Effect::RadiusStim {
                    linear_falloff: true,
                    source_entity_id: Some(entity_id),
                    center,
                    radius: source.radius,
                    intensity,
                    stim_template_id: source.stim_template_id,
                }));
            }
        }
        if destroy_on_completion {
            effects.push(Effect::DestroyEntity { entity_id });
        }

        if effects.is_empty() {
            Effect::NoEffect
        } else {
            Effect::combine(effects)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source(lifecycle: StimSourceLifecycle) -> PeriodicSourceState {
        PeriodicSourceState::new(
            -378,
            StimSourceOptions {
                intensity: 5.0,
                propagator: StimPropagator::Radius { radius: 2.4 },
                lifecycle,
            },
        )
        .unwrap()
    }

    #[test]
    fn zero_period_is_rejected_instead_of_firing_every_millisecond() {
        assert!(
            PeriodicSourceState::new(
                -378,
                StimSourceOptions {
                    intensity: 5.0,
                    propagator: StimPropagator::Radius { radius: 2.4 },
                    lifecycle: StimSourceLifecycle {
                        no_max_firings: true,
                        ..Default::default()
                    },
                }
            )
            .is_none()
        );
    }

    #[test]
    fn zero_period_one_shot_fires_once_without_inventing_a_cadence() {
        let mut source = source(StimSourceLifecycle {
            max_firings: 1,
            destroy_on_completion: true,
            ..Default::default()
        });
        assert_eq!(source.advance(Duration::ZERO), (vec![5.0], true));
        assert_eq!(source.advance(Duration::from_secs(30)), (vec![], false));
    }

    #[test]
    fn save_load_keeps_the_remaining_period_and_finite_firing_budget() {
        let mut before = InternalPeriodicStim::new();
        before.sources.push(source(StimSourceLifecycle {
            period: Duration::from_secs(5),
            max_firings: 3,
            no_max_firings: false,
            destroy_on_completion: true,
            intensity_slope: 0.25,
        }));
        assert_eq!(
            before.sources[0].advance(Duration::from_secs(2)),
            (vec![5.0], false)
        );
        let saved = before
            .save_state()
            .expect("periodic source must persist its lifecycle");
        let mut restored = InternalPeriodicStim::new();
        restored
            .restore_state(
                &saved,
                &super::super::ScriptRestoreContext::new(&std::collections::HashMap::new()),
            )
            .unwrap();
        assert_eq!(
            restored.sources[0].advance(Duration::from_millis(2999)),
            (vec![], false)
        );
        assert_eq!(
            restored.sources[0].advance(Duration::from_millis(1)),
            (vec![5.25], false)
        );
        assert_eq!(
            restored.sources[0].advance(Duration::from_secs(5)),
            (vec![5.5], true)
        );
        assert_eq!(
            restored.sources[0].advance(Duration::from_secs(50)),
            (vec![], false)
        );
    }

    #[test]
    fn infinite_source_fires_at_birth_and_each_period() {
        let mut source = source(StimSourceLifecycle {
            period: Duration::from_secs(5),
            max_firings: 0,
            no_max_firings: true,
            destroy_on_completion: false,
            intensity_slope: 0.0,
        });

        assert_eq!(source.advance(Duration::ZERO), (vec![5.0], false));
        assert_eq!(
            source.advance(Duration::from_millis(4_999)),
            (vec![], false)
        );
        assert_eq!(source.advance(Duration::from_millis(1)), (vec![5.0], false));
    }

    #[test]
    fn finite_source_applies_slope_and_destroys_after_its_last_firing() {
        let mut source = source(StimSourceLifecycle {
            period: Duration::from_millis(100),
            max_firings: 3,
            no_max_firings: false,
            destroy_on_completion: true,
            intensity_slope: 0.25,
        });

        assert_eq!(
            source.advance(Duration::from_millis(250)),
            (vec![5.0, 5.25, 5.5], true)
        );
        assert_eq!(source.advance(Duration::from_secs(1)), (vec![], false));
    }
}
