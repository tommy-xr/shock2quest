//! Regeneration feedback at the amp's actual muzzle, shared by flat and VR.
use cgmath::{Matrix4, Transform, vec3};
use engine::scene::{ParticleSystem, SceneObject};
use shipyard::{EntityId, Get, View, World};
use std::time::Duration;
pub(crate) fn render(world: &World, amp: EntityId, age: f32) -> Vec<SceneObject> {
    let Ok(transforms) = world.borrow::<View<crate::runtime_props::RuntimePropTransform>>() else {
        return vec![];
    };
    let Ok(transform) = transforms.get(amp) else {
        return vec![];
    };
    let Ok(muzzles) = world.borrow::<View<crate::weapon_muzzle::MuzzleFallback>>() else {
        return vec![];
    };
    let Ok(muzzle) = muzzles.get(amp) else {
        return vec![];
    };
    let center = transform.0.transform_point(muzzle.point);
    let mut objects = Vec::new();
    for i in 0..24 {
        let angle = i as f32 * std::f32::consts::TAU / 24.0 + age * 2.0;
        let radius = 0.12 + age * 0.32;
        let offset = vec3(angle.cos() * radius, age * 0.22, angle.sin() * radius);
        let mut mote = ParticleSystem::new()
            .with_num_particles(1)
            .with_one_shot(true)
            .with_color(vec3(0.12, 1.0, 0.45))
            .with_alpha((1.0 - age).max(0.0) * 0.85)
            .with_particle_size(0.13, 0.13)
            .with_lifetime(1.0, 1.0)
            .with_launch_bounding_box(vec3(0.0, 0.0, 0.0), vec3(0.0, 0.0, 0.0))
            .with_velocity(vec3(0.0, 0.0, 0.0), vec3(0.0, 0.0, 0.0));
        mote.update(
            Duration::ZERO,
            Matrix4::from_translation(vec3(center.x, center.y, center.z) + offset),
        );
        objects.extend(mote.render());
    }
    objects
}
