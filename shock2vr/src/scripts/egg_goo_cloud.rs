//! EggGooCloud's engine-owned one-shot radius stimulus and particle lifetime.
//! Its arSrcDesc authors one immediate pulse, radius4, intensity2, raycast,
//! no dispersion. Period5000 is irrelevant with max_firings1. See
//! projects/egg-goo-cloud.md for the raw record and original lifecycle source.
use dark::properties::{Link, PropParticleLaunchInfo, PropPosition, StimPropagator};
use serde::{Deserialize, Serialize};
use shipyard::{EntityId, Get, View, World};

use super::{
    Effect, Script, ScriptRestoreContext, ScriptState, ScriptStateError,
    script_util::get_all_links_with_template,
};
use crate::{physics::PhysicsWorld, time::Time};

const KEY: &str = "shock2vr.egg_goo_cloud";

#[derive(Default, Serialize, Deserialize)]
pub struct EggGooCloud {
    fired: bool,
    remaining: Option<f32>,
}

impl Script for EggGooCloud {
    fn update(
        &mut self,
        entity_id: EntityId,
        world: &World,
        _physics: &PhysicsWorld,
        time: &Time,
    ) -> Effect {
        if self.remaining.is_none() {
            let launch = world.borrow::<View<PropParticleLaunchInfo>>().unwrap();
            let Ok(launch) = launch.get(entity_id) else {
                return Effect::NoEffect;
            };
            // All particles launch together. Keep the entity through the
            // longest authored particle, then discard the spent effect.
            self.remaining = Some(launch.min_time.max(launch.max_time).max(0.0));
        }
        if !self.fired {
            let positions = world.borrow::<View<PropPosition>>().unwrap();
            let Ok(position) = positions.get(entity_id) else {
                return Effect::NoEffect;
            };
            self.fired = true;
            return Effect::combine(
                get_all_links_with_template(world, entity_id, |link| match link {
                    Link::StimSource(options) => Some(*options),
                    _ => None,
                })
                .into_iter()
                .filter_map(|(stim_template_id, options)| {
                    let StimPropagator::Radius { radius } = options.propagator else {
                        return None;
                    };
                    Some(Effect::RadiusStim {
                        linear_falloff: false,
                        source_entity_id: Some(entity_id),
                        center: position.position,
                        radius,
                        intensity: options.intensity,
                        stim_template_id,
                    })
                })
                .collect(),
            );
        }
        let remaining = self.remaining.as_mut().unwrap();
        *remaining -= time.elapsed.as_secs_f32();
        if *remaining <= 0.0 {
            Effect::DestroyEntity { entity_id }
        } else {
            Effect::NoEffect
        }
    }
    fn script_state_key(&self) -> Option<&'static str> {
        Some(KEY)
    }
    fn save_state(&self) -> Result<ScriptState, ScriptStateError> {
        ScriptState::encode(1, self, KEY)
    }
    fn restore_state(
        &mut self,
        state: &ScriptState,
        _context: &ScriptRestoreContext<'_>,
    ) -> Result<(), ScriptStateError> {
        *self = state.decode(1, KEY)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cgmath::{Quaternion, vec3};
    use dark::properties::{Links, StimSourceOptions, ToLink};
    #[test]
    fn cloud_pulses_once_at_full_intensity_and_restores_remaining_burst_lifetime() {
        let mut world = World::new();
        let zero = vec3(0.0, 0.0, 0.0);
        let entity = world.add_entity((
            PropPosition {
                position: zero,
                rotation: Quaternion::new(1.0, 0.0, 0.0, 0.0),
                cell: 0,
            },
            PropParticleLaunchInfo {
                launch_type: 0,
                loc_min: zero,
                loc_max: zero,
                vel_min: zero,
                vel_max: zero,
                min_radius: 0.0,
                max_radius: 0.0,
                min_time: 0.9,
                max_time: 1.3,
            },
            Links {
                to_links: vec![ToLink {
                    to_template_id: -387,
                    to_entity_id: None,
                    link: Link::StimSource(StimSourceOptions {
                        intensity: 2.0,
                        propagator: StimPropagator::Radius { radius: 4.0 },
                    }),
                }],
            },
        ));
        let physics = PhysicsWorld::new();
        let mut cloud = EggGooCloud::default();
        let advance = |cloud: &mut EggGooCloud, seconds: f32| {
            cloud.update(
                entity,
                &world,
                &physics,
                &Time {
                    elapsed: std::time::Duration::from_secs_f32(seconds),
                    ..Time::default()
                },
            )
        };
        let effects = Effect::flatten(vec![advance(&mut cloud, 0.01)]);
        assert!(matches!(
            effects.as_slice(),
            [Effect::RadiusStim {
                linear_falloff: false,
                intensity: 2.0,
                radius: 4.0,
                stim_template_id: -387,
                ..
            }]
        ));
        assert!(matches!(advance(&mut cloud, 0.5), Effect::NoEffect));
        let mut restored = EggGooCloud::default();
        restored
            .restore_state(
                &cloud.save_state().unwrap(),
                &ScriptRestoreContext::new(&std::collections::HashMap::new()),
            )
            .unwrap();
        assert!(matches!(advance(&mut restored, 0.5), Effect::NoEffect));
        assert!(matches!(
            advance(&mut restored, 0.31),
            Effect::DestroyEntity { .. }
        ));
    }
}
