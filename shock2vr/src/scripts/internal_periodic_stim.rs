use std::time::Duration;

use cgmath::{Transform, point3};
use dark::properties::{Link, StimPropagator, StimSourceLifecycle, StimSourceOptions};
use shipyard::{EntityId, Get, View, World};

use crate::physics::PhysicsWorld;
use crate::runtime_props::RuntimePropTransform;
use crate::time::Time;
use crate::util::point3_to_vec3;

use super::{Effect, Script, script_util::get_all_links_with_template};

const MINIMUM_PERIOD: Duration = Duration::from_millis(1);

#[derive(Clone, Copy, Debug)]
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
        let period = self.lifecycle.period.max(MINIMUM_PERIOD);
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
