use cgmath::{Transform, point3};
use dark::properties::{Link, StimPropagator, StimSourceOptions};
use shipyard::{EntityId, Get, View, World};

use crate::physics::PhysicsWorld;
use crate::runtime_props::RuntimePropTransform;
use crate::time::Time;
use crate::util::point3_to_vec3;

use super::{Effect, Script, script_util::get_all_links_with_template};

// Detonates an entity carrying radius stim sources (L$arSrcDesc): explosion
// SFX templates like "HE Explosion" and "Incendiary Explosion". One frame
// after the entity spawns, everything within the blast radius takes
// falloff-scaled damage and dynamic bodies are shoved outward. The visual
// (bitmap animation) and sound are handled by the entity's other properties;
// this script is only the blast.
/// The ShakeStim archetype (-3558): every explosion links it for camera shake.
/// It is not damage, and its numbers are unrelated to the blast's.
const SHAKE_STIM_TEMPLATE_ID: i32 = -3558;

pub struct InternalExplosion {
    has_fired: bool,
}

impl InternalExplosion {
    pub fn new() -> InternalExplosion {
        InternalExplosion { has_fired: false }
    }
}

impl Script for InternalExplosion {
    fn update(
        &mut self,
        entity_id: EntityId,
        world: &World,
        _physics: &PhysicsWorld,
        _time: &Time,
    ) -> Effect {
        if self.has_fired {
            return Effect::NoEffect;
        }
        self.has_fired = true;

        let radius_sources = get_all_links_with_template(world, entity_id, |link| match link {
            Link::StimSource(opts) => Some(*opts),
            _ => None,
        });

        // Explosions carry an impact stim (the blast) plus a ShakeStim (camera
        // shake - unimplemented, and with unrelated numbers: Droid Fusion's
        // shake is 15 @ r0.4 next to its 12 @ r4 damage stim). Skip the shake
        // and take the strongest remaining source as the blast - one authored
        // stim, never a mix.
        let mut blast: Option<(i32, f32, f32)> = None;
        for (
            stim_template_id,
            StimSourceOptions {
                intensity,
                propagator,
            },
        ) in radius_sources
        {
            if stim_template_id == SHAKE_STIM_TEMPLATE_ID {
                continue;
            }
            if let StimPropagator::Radius { radius } = propagator {
                if blast.is_none_or(|(_, max_intensity, _)| intensity > max_intensity) {
                    blast = Some((stim_template_id, intensity, radius));
                }
            }
        }
        let Some((stim_template_id, intensity, radius)) = blast else {
            return Effect::NoEffect;
        };

        let v_transform = world.borrow::<View<RuntimePropTransform>>().unwrap();
        let Ok(transform) = v_transform.get(entity_id) else {
            return Effect::NoEffect;
        };
        let center = point3_to_vec3(transform.0.transform_point(point3(0.0, 0.0, 0.0)));

        Effect::RadiusBlast {
            center,
            radius,
            intensity,
            stim_template_id,
        }
    }
}
