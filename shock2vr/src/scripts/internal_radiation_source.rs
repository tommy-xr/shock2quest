use cgmath::{Transform, point3};
use dark::properties::{Link, StimPropagator};
use shipyard::{EntityId, Get, View, World};

use crate::physics::PhysicsWorld;
use crate::runtime_props::RuntimePropTransform;
use crate::time::Time;
use crate::util::point3_to_vec3;

use super::{Effect, Script, script_util::get_all_links_with_template};

/// Radiation stimulus archetype authored by `The Player`'s Radiate
/// receptron and emitted by the persistent `Rad Burst` corpse effect.
pub const RADIATION_STIM_TEMPLATE_ID: i32 = -386;

/// Refresh a persistent radius radiation source every frame. `RadCheck`
/// integrates that ambient value on its own retail 100 ms cadence, so this is
/// an observation rather than repeated additive damage.
pub struct InternalRadiationSource;

impl Script for InternalRadiationSource {
    fn update(
        &mut self,
        entity_id: EntityId,
        world: &World,
        _physics: &PhysicsWorld,
        _time: &Time,
    ) -> Effect {
        let source = get_all_links_with_template(world, entity_id, |link| match link {
            Link::StimSource(options) => Some(*options),
            _ => None,
        })
        .into_iter()
        .find_map(|(stim_template_id, options)| {
            if stim_template_id != RADIATION_STIM_TEMPLATE_ID {
                return None;
            }
            let StimPropagator::Radius { radius } = options.propagator else {
                return None;
            };
            Some((options.intensity, radius))
        });
        let Some((intensity, radius)) = source else {
            return Effect::NoEffect;
        };

        let transforms = world.borrow::<View<RuntimePropTransform>>().unwrap();
        let Ok(transform) = transforms.get(entity_id) else {
            return Effect::NoEffect;
        };
        let center = point3_to_vec3(transform.0.transform_point(point3(0.0, 0.0, 0.0)));

        Effect::RadiusStim {
            linear_falloff: true,
            source_entity_id: None,
            center,
            radius,
            intensity,
            stim_template_id: RADIATION_STIM_TEMPLATE_ID,
        }
    }
}

#[cfg(test)]
mod tests {
    use cgmath::{Matrix4, vec3};
    use dark::properties::{Link, Links, StimPropagator, StimSourceOptions, ToLink};
    use shipyard::World;

    use crate::{
        physics::PhysicsWorld,
        runtime_props::RuntimePropTransform,
        scripts::{Effect, Script},
        time::Time,
    };

    use super::{InternalRadiationSource, RADIATION_STIM_TEMPLATE_ID};

    #[test]
    fn authored_rad_burst_emits_ambient_radius_stim_without_blast_force() {
        let mut world = World::new();
        let source = world.add_entity((
            RuntimePropTransform(Matrix4::from_translation(vec3(1.0, 2.0, 3.0))),
            Links {
                to_links: vec![ToLink {
                    to_template_id: RADIATION_STIM_TEMPLATE_ID,
                    to_entity_id: None,
                    link: Link::StimSource(StimSourceOptions {
                        intensity: 8.0,
                        propagator: StimPropagator::Radius { radius: 6.0 },
                    }),
                }],
            },
        ));

        let effect =
            InternalRadiationSource.update(source, &world, &PhysicsWorld::new(), &Time::default());

        assert!(matches!(
            effect,
            Effect::RadiusStim {
            linear_falloff: true,
                source_entity_id: None,
                center,
                radius: 6.0,
                intensity: 8.0,
                stim_template_id: RADIATION_STIM_TEMPLATE_ID,
            } if center == vec3(1.0, 2.0, 3.0)
        ));
    }
}
