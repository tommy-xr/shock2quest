//! Brief spell feedback, drawn in world space for both presentations.
use cgmath::{Matrix4, Vector3, vec3};
use engine::scene::{ParticleSystem, SceneObject};
use std::time::Duration;
pub(crate) struct DrainTrail {
    from: Vector3<f32>,
    to: Vector3<f32>,
    elapsed: f32,
}
impl DrainTrail {
    pub fn new(from: Vector3<f32>, to: Vector3<f32>) -> Self {
        Self {
            from,
            to,
            elapsed: 0.0,
        }
    }
    pub fn advance(&mut self, dt: Duration) -> bool {
        self.elapsed += dt.as_secs_f32();
        self.elapsed < 0.9
    }
    pub fn render(&self) -> Vec<SceneObject> {
        let mut objects = Vec::new();
        for i in 0..18 {
            let progress = (self.elapsed - i as f32 * 0.018) / 0.55;
            if !(0.0..=1.0).contains(&progress) {
                continue;
            }
            let angle = progress * 12.0 + i as f32 * 2.4;
            let radius = 0.15 * (std::f32::consts::PI * progress).sin();
            let point = self.from
                + (self.to - self.from) * progress
                + vec3(angle.cos() * radius, angle.sin() * radius, 0.0);
            let mut mote = ParticleSystem::new()
                .with_num_particles(1)
                .with_one_shot(true)
                .with_color(vec3(0.15, 1.0, 0.7))
                .with_alpha(0.85)
                .with_particle_size(0.18, 0.18)
                .with_lifetime(1.0, 1.0)
                .with_velocity(vec3(0.0, 0.0, 0.0), vec3(0.0, 0.0, 0.0))
                .with_launch_bounding_box(vec3(0.0, 0.0, 0.0), vec3(0.0, 0.0, 0.0));
            mote.update(Duration::ZERO, Matrix4::from_translation(point));
            objects.extend(mote.render());
        }
        objects
    }
}
