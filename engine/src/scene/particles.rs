use std::{rc::Rc, sync::Arc, time::Duration};

use cgmath::{InnerSpace, Matrix4, SquareMatrix, Vector3, vec3};
use rand::Rng;

use crate::{
    texture::{self, Texture, TextureTrait},
    texture_format::RawTextureData,
};

use super::{BillboardMaterial, SceneObject, quad};

use lazy_static::lazy_static;

lazy_static! {
    static ref PARTICLE_TEXTURE: Arc<Texture> = create_particle_texture();
}

fn create_particle_texture() -> Arc<Texture> {
    // build and compile our shader program
    // ------------------------------------
    // vertex shader
    const SIZE: u32 = 128;
    const ISIZE: i32 = SIZE as i32;
    const CENTER_X: i32 = ISIZE / 2;
    const CENTER_Y: i32 = ISIZE / 2;
    const RADIUS: f32 = (ISIZE / 2) as f32;

    let mut texture_data = RawTextureData {
        bytes: vec![0; (SIZE * SIZE * 4) as usize], // Initialize with zeros
        width: SIZE,
        height: SIZE,
        format: crate::texture_format::PixelFormat::RGBA,
    };

    for x in 0..ISIZE {
        for y in 0..ISIZE {
            let index = (x + y * ISIZE) as usize;
            let distance =
                (((x - CENTER_X) as f32).powf(2.0) + ((y - CENTER_Y) as f32).powf(2.0)).sqrt();

            let mut ratio = 1.0 - (distance / RADIUS);
            if ratio < 0.0 {
                ratio = 0.0;
            }

            texture_data.bytes[index * 4] = 255; // Red component
            texture_data.bytes[index * 4 + 1] = 255; // Green component
            texture_data.bytes[index * 4 + 2] = 255; // Blue component
            texture_data.bytes[index * 4 + 3] = (255.0 * ratio * ratio) as u8; // Alpha component
        }
    }
    let texture = texture::init_from_memory(texture_data);
    Arc::new(texture)
}

/// Where a particle spawns, in the group's local (y-up) space.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LaunchShape {
    /// Uniformly inside the launch bounding box.
    Box,
    /// A random direction at a distance in `radius` (min, max).
    Sphere { radius: (f32, f32) },
    /// A random horizontal (x/z) direction at a distance in `radius`; the
    /// height (y) comes from the launch bounding box.
    Cylinder { radius: (f32, f32) },
}

#[derive(Clone)]
pub struct ParticleSystem {
    particles: Vec<Particle>,
    acceleration: Vector3<f32>,
    max_particles: usize,
    particle_alpha: f32,
    particle_fade_time: f32, // Time in seconds for the particle to fade out
    launch_time: f32,        // Time in seconds to wait between particle launches
    launch_time_remaining: f32,
    launch_bounding_box: (Vector3<f32>, Vector3<f32>),
    launch_velocity: (Vector3<f32>, Vector3<f32>),
    launch_shape: LaunchShape,
    launch_lifetime: (f32, f32),
    root_transform: Matrix4<f32>,
    /// The emitter transform from the previous update, to place world-space
    /// particles launched between frames along the emitter's path.
    prev_transform: Option<Matrix4<f32>>,
    particle_size: (f32, f32),
    /// Tint applied to the sprite (resolved from the group's palette color
    /// index upstream). Defaults to white = untinted full texture.
    color: Vector3<f32>,
    /// Tint over a particle's remaining life as `(remaining_seconds, color)`,
    /// longest first, held past either end. Empty = constant `color`.
    color_keys: Vec<(f32, Vector3<f32>)>,
    /// One-shot burst (impact spangs): all particles launch on the first
    /// update and are never relaunched; `is_done()` reports when they have all
    /// expired. `false` = continuous emitter (steam vents etc.).
    one_shot: bool,
    has_launched: bool,
    /// Sprite for bitmap particles (PRT_SCALED_BITMAP); `None` renders the
    /// default radial glow disk.
    sprite_texture: Option<Rc<dyn TextureTrait>>,
    sprite_frames: Vec<Rc<dyn TextureTrait>>,
    sprite_frame_time: f32,
    sprite_looping: bool,
    size_velocity: f32,
    fade_in_time: f32,
    world_space: bool,
    /// Whether a continuous emitter launches new particles; existing ones
    /// live out their lifetimes either way.
    emitting: bool,
}

fn randf(a: f32, b: f32) -> f32 {
    if a == b {
        return a;
    }

    if a > b {
        rand::thread_rng().gen_range(b..=a)
    } else {
        rand::thread_rng().gen_range(a..=b)
    }
}
fn randv3(a: Vector3<f32>, b: Vector3<f32>) -> Vector3<f32> {
    vec3(randf(a.x, b.x), randf(a.y, b.y), randf(a.z, b.z))
}

/// A random unit vector over the axes `mask` keeps, e.g. `vec3(1, 0, 1)` for
/// the horizontal plane. Not uniform over the sphere, like the original's.
fn random_direction(mask: Vector3<f32>) -> Vector3<f32> {
    let v = randv3(-mask, mask);
    if v.magnitude2() > 0.0 {
        v.normalize()
    } else {
        v
    }
}

fn launch_position(system: &ParticleSystem) -> Vector3<f32> {
    let (min, max) = system.launch_bounding_box;
    match system.launch_shape {
        LaunchShape::Box => randv3(min, max),
        LaunchShape::Sphere { radius } => {
            random_direction(vec3(1.0, 1.0, 1.0)) * randf(radius.0, radius.1)
        }
        LaunchShape::Cylinder { radius } => {
            let flat = random_direction(vec3(1.0, 0.0, 1.0)) * randf(radius.0, radius.1);
            vec3(flat.x, randf(min.y, max.y), flat.z)
        }
    }
}

fn create_random_particle(system: &ParticleSystem, transform: Matrix4<f32>) -> Particle {
    let lifetime = randf(system.launch_lifetime.0, system.launch_lifetime.1);
    let mut position = launch_position(system);
    let mut velocity = randv3(system.launch_velocity.0, system.launch_velocity.1);
    if system.world_space {
        position = (transform * position.extend(1.0)).truncate();
        velocity = (transform * velocity.extend(0.0)).truncate();
    }
    let scale = randf(system.particle_size.0, system.particle_size.1);
    Particle {
        age: 0.0,
        remaining_life_in_seconds: lifetime,
        position,
        velocity,
        scale,
    }
}

impl Default for ParticleSystem {
    fn default() -> Self {
        Self::new()
    }
}

impl ParticleSystem {
    pub fn new() -> ParticleSystem {
        ParticleSystem {
            particle_alpha: 0.5,
            max_particles: 60,
            launch_time: 0.1,
            particle_fade_time: 0.5,
            launch_time_remaining: 0.1,
            acceleration: vec3(0.0, 0.0, 0.0),
            launch_bounding_box: (vec3(-0.2, -0.1, -0.2), vec3(0.2, 0.1, 0.2)),
            launch_velocity: (vec3(0.0, 3.0, 0.0), vec3(0.0, 4.0, 0.0)),
            launch_shape: LaunchShape::Box,
            launch_lifetime: (1.0, 10.0),
            particles: vec![],
            particle_size: (0.08, 0.08),
            root_transform: Matrix4::identity(),
            prev_transform: None,
            color: vec3(1.0, 1.0, 1.0),
            color_keys: Vec::new(),
            one_shot: false,
            has_launched: false,
            sprite_texture: None,
            sprite_frames: Vec::new(),
            sprite_frame_time: 0.1,
            sprite_looping: false,
            size_velocity: 0.0,
            fade_in_time: 0.0,
            world_space: false,
            emitting: true,
        }
    }

    pub fn with_color(self, color: Vector3<f32>) -> ParticleSystem {
        ParticleSystem { color, ..self }
    }

    pub fn with_one_shot(self, one_shot: bool) -> ParticleSystem {
        ParticleSystem { one_shot, ..self }
    }

    pub fn with_sprite_texture(self, texture: Rc<dyn TextureTrait>) -> ParticleSystem {
        ParticleSystem {
            sprite_texture: Some(texture),
            sprite_frames: Vec::new(),
            ..self
        }
    }

    /// Each particle starts its own animation at birth. An empty sequence keeps
    /// the existing single sprite (or glow disk); a non-looping sequence holds
    /// its final frame until the particle expires.
    pub fn with_sprite_animation(
        self,
        frames: Vec<Rc<dyn TextureTrait>>,
        frame_time: Duration,
        looping: bool,
    ) -> ParticleSystem {
        ParticleSystem {
            sprite_frames: frames,
            sprite_frame_time: frame_time.as_secs_f32(),
            sprite_looping: looping,
            ..self
        }
    }

    /// Change sprite diameter in world units per second, clamped at zero.
    pub fn with_size_velocity(self, size_velocity: f32) -> ParticleSystem {
        ParticleSystem {
            size_velocity,
            ..self
        }
    }

    pub fn with_fade_in_time(self, fade_in_time: f32) -> ParticleSystem {
        ParticleSystem {
            fade_in_time,
            ..self
        }
    }

    /// Bake the emitter pose into each particle at birth. Existing particles
    /// then stay in the world when the emitter moves, and acceleration is in
    /// world coordinates (so gravity does not rotate with an impact normal).
    pub fn with_world_space(self, world_space: bool) -> ParticleSystem {
        ParticleSystem {
            world_space,
            ..self
        }
    }

    /// A one-shot system whose burst has fully expired (never true for
    /// continuous emitters). The owner can drop the system - and, for impact
    /// spangs, the entity carrying it.
    pub fn is_done(&self) -> bool {
        self.one_shot && self.has_launched && self.particles.is_empty()
    }

    pub fn with_lifetime(self, min: f32, max: f32) -> ParticleSystem {
        ParticleSystem {
            launch_lifetime: (min, max),
            ..self
        }
    }

    pub fn with_num_particles(self, max_particles: usize) -> ParticleSystem {
        ParticleSystem {
            max_particles,
            ..self
        }
    }

    pub fn with_velocity(self, min: Vector3<f32>, max: Vector3<f32>) -> ParticleSystem {
        ParticleSystem {
            launch_velocity: (min, max),
            ..self
        }
    }

    pub fn with_acceleration(self, acceleration: Vector3<f32>) -> ParticleSystem {
        ParticleSystem {
            acceleration,
            ..self
        }
    }

    pub fn with_particle_size(self, min: f32, max: f32) -> ParticleSystem {
        ParticleSystem {
            particle_size: (min, max),
            ..self
        }
    }

    pub fn with_launch_bounding_box(self, min: Vector3<f32>, max: Vector3<f32>) -> ParticleSystem {
        ParticleSystem {
            launch_bounding_box: (min, max),
            ..self
        }
    }

    pub fn with_launch_shape(self, launch_shape: LaunchShape) -> ParticleSystem {
        ParticleSystem {
            launch_shape,
            ..self
        }
    }

    pub fn with_launch_time(self, launch_time: Duration) -> ParticleSystem {
        ParticleSystem {
            launch_time: launch_time.as_secs_f32(),
            launch_time_remaining: launch_time.as_secs_f32(),
            ..self
        }
    }

    /// Where the emitter was `overdue` seconds before this update's
    /// `transform`: world-space launches interpolate the translation back
    /// toward the previous update; object-space ones ride the emitter anyway.
    fn emitter_transform_at(
        &self,
        transform: Matrix4<f32>,
        overdue: f32,
        delta_time: f32,
    ) -> Matrix4<f32> {
        match self.prev_transform {
            Some(prev) if self.world_space && delta_time > 0.0 => {
                let mut at = transform;
                at.w = transform.w + (prev.w - transform.w) * (overdue / delta_time);
                at
            }
            _ => transform,
        }
    }

    pub fn is_world_space(&self) -> bool {
        self.world_space
    }

    pub fn with_color_keys(self, color_keys: Vec<(f32, Vector3<f32>)>) -> ParticleSystem {
        ParticleSystem { color_keys, ..self }
    }

    /// The tint for a particle with `remaining` seconds of life.
    fn color_at(&self, remaining: f32) -> Vector3<f32> {
        let Some(&(first_time, first_color)) = self.color_keys.first() else {
            return self.color;
        };
        if remaining >= first_time {
            return first_color;
        }
        for pair in self.color_keys.windows(2) {
            let ((t_hi, c_hi), (t_lo, c_lo)) = (pair[0], pair[1]);
            if remaining >= t_lo {
                let f = if t_hi > t_lo {
                    (remaining - t_lo) / (t_hi - t_lo)
                } else {
                    1.0
                };
                return c_lo + (c_hi - c_lo) * f;
            }
        }
        self.color_keys.last().unwrap().1
    }

    pub fn with_alpha(self, alpha: f32) -> ParticleSystem {
        ParticleSystem {
            particle_alpha: alpha,
            ..self
        }
    }

    pub fn with_fade_time(self, fade_time: f32) -> ParticleSystem {
        ParticleSystem {
            particle_fade_time: fade_time,
            ..self
        }
    }

    /// Start or stop launching particles (continuous emitters).
    pub fn set_emitting(&mut self, emitting: bool) {
        self.emitting = emitting;
    }

    pub fn particle_count(&self) -> usize {
        self.particles.len()
    }

    pub fn update(&mut self, dt: Duration, transform: Matrix4<f32>) {
        let delta_time = dt.as_secs_f32();
        let (acceleration, size_velocity) = (self.acceleration, self.size_velocity);
        self.particles
            .iter_mut()
            .for_each(|p| p.advance(delta_time, acceleration, size_velocity));

        self.particles.retain(|p| p.remaining_life_in_seconds > 0.0);

        self.launch_time_remaining -= delta_time;

        if self.one_shot {
            // Launch the whole burst once; expired particles are never
            // replaced (see `is_done`).
            if !self.has_launched {
                self.has_launched = true;
                for _ in 0..self.max_particles {
                    self.particles.push(create_random_particle(self, transform));
                }
            }
        } else if !self.emitting {
            // Paused: don't bank time, or resuming would launch a backlog.
            self.launch_time_remaining = self.launch_time_remaining.max(0.0);
        } else if self.launch_time <= 0.0 {
            // No period: one launch per frame while there is room.
            if self.particles.len() < self.max_particles {
                self.particles.push(create_random_particle(self, transform));
            }
        } else {
            // Launch every particle due this frame, keeping the remainder so a
            // period that isn't a frame multiple keeps its rate. Each launch is
            // backdated to its due time - aged, and in world space placed
            // along the emitter's path - so a sub-frame period streaks rather
            // than stacking at the frame's position.
            while self.launch_time_remaining <= 0.0 && self.particles.len() < self.max_particles {
                let overdue = (-self.launch_time_remaining).min(delta_time);
                let emitter = self.emitter_transform_at(transform, overdue, delta_time);
                let mut particle = create_random_particle(self, emitter);
                particle.advance(overdue, acceleration, size_velocity);
                self.particles.push(particle);
                self.launch_time_remaining += self.launch_time;
            }
            // Full: don't bank time while waiting, or the next launches
            // would be backdated as if overdue.
            self.launch_time_remaining = self.launch_time_remaining.max(0.0);
        }
        self.prev_transform = Some(transform);

        self.root_transform = if self.world_space {
            Matrix4::identity()
        } else {
            transform
        };
    }

    pub fn render(&self) -> Vec<SceneObject> {
        let particle_texture: Arc<dyn TextureTrait> = (*PARTICLE_TEXTURE).clone();
        self.particles
            .iter()
            .map(|p| {
                let mut alpha = 1.0;
                let adj_time = self.particle_fade_time - p.remaining_life_in_seconds;

                if adj_time > 0.0 {
                    alpha = 1.0 - (adj_time / self.particle_fade_time);
                }
                if self.fade_in_time > 0.0 {
                    alpha *= (p.age / self.fade_in_time).clamp(0.0, 1.0);
                }
                // Emissive so particles self-glow (visible in dark scenes); the
                // glow is tinted by `color` in the shader, so it stays the
                // palette color rather than washing to white.
                let sprite = sprite_frame_index(
                    p.age,
                    self.sprite_frames.len(),
                    self.sprite_frame_time,
                    self.sprite_looping,
                )
                .and_then(|index| self.sprite_frames.get(index))
                .or(self.sprite_texture.as_ref());
                let mat = match sprite {
                    // Sprites render unlit at their authored texel colors -
                    // adding emissive on top would double the brightness and
                    // clip the sprite's gradients to white.
                    Some(sprite) => BillboardMaterial::create(
                        sprite.clone(),
                        self.color_at(p.remaining_life_in_seconds),
                        0.0,
                        1.0 - (self.particle_alpha * alpha),
                        p.scale,
                    ),
                    None => BillboardMaterial::create(
                        particle_texture.clone(),
                        self.color_at(p.remaining_life_in_seconds),
                        1.0,
                        1.0 - (self.particle_alpha * alpha),
                        p.scale,
                    ),
                };
                let mut scene_obj = SceneObject::new(mat, Box::new(quad::create()));
                scene_obj.set_local_transform(
                    Matrix4::from_translation(p.position) * Matrix4::from_scale(p.scale),
                );
                scene_obj.set_transform(self.root_transform);
                scene_obj
            })
            .collect::<Vec<SceneObject>>()
    }
}

#[derive(Clone)]
struct Particle {
    age: f32,
    remaining_life_in_seconds: f32,
    position: Vector3<f32>,
    velocity: Vector3<f32>,
    scale: f32,
}

impl Particle {
    fn advance(&mut self, dt: f32, acceleration: Vector3<f32>, size_velocity: f32) {
        self.age += dt;
        self.remaining_life_in_seconds -= dt;
        self.position += self.velocity * dt;
        self.velocity += acceleration * dt;
        self.scale = (self.scale + size_velocity * dt).max(0.0);
    }
}

fn sprite_frame_index(age: f32, count: usize, frame_time: f32, looping: bool) -> Option<usize> {
    if count == 0 {
        return None;
    }
    if !frame_time.is_finite() || frame_time <= 0.0 {
        return Some(0);
    }
    let frame = (age / frame_time) as usize;
    Some(if looping {
        frame % count
    } else {
        frame.min(count - 1)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use cgmath::InnerSpace;

    #[test]
    fn world_particles_keep_launch_pose_and_gravity_when_emitter_moves() {
        let mut system = ParticleSystem::new()
            .with_one_shot(true)
            .with_num_particles(1)
            .with_lifetime(2.0, 2.0)
            .with_world_space(true)
            .with_launch_bounding_box(vec3(0.0, 0.0, 0.0), vec3(0.0, 0.0, 0.0))
            .with_velocity(vec3(-1.0, 0.0, 0.0), vec3(-1.0, 0.0, 0.0))
            .with_acceleration(vec3(0.0, -2.0, 0.0));
        let launch = Matrix4::from_translation(vec3(4.0, 5.0, 6.0))
            * Matrix4::from_angle_z(cgmath::Deg(90.0));
        system.update(Duration::ZERO, launch);
        system.update(
            Duration::from_millis(500),
            Matrix4::from_translation(vec3(99.0, 99.0, 99.0)),
        );
        let p = &system.particles[0];
        assert!((p.position - vec3(4.0, 4.5, 6.0)).magnitude2() < 1e-5);
        assert!((p.velocity - vec3(0.0, -2.0, 0.0)).magnitude2() < 1e-5);
        assert_eq!(system.root_transform, Matrix4::identity());
    }

    #[test]
    fn launch_shapes_place_particles_on_their_radius() {
        let launch = |shape| {
            let system = ParticleSystem::new()
                .with_launch_bounding_box(vec3(-5.0, 0.5, -5.0), vec3(5.0, 1.5, 5.0))
                .with_launch_shape(shape);
            (0..200).map(move |_| launch_position(&system))
        };
        for p in launch(LaunchShape::Sphere { radius: (2.0, 2.0) }) {
            assert!((p.magnitude() - 2.0).abs() < 1e-4, "{p:?}");
        }
        for p in launch(LaunchShape::Cylinder { radius: (2.0, 3.0) }) {
            let flat = vec3(p.x, 0.0, p.z).magnitude();
            assert!((2.0 - 1e-4..=3.0 + 1e-4).contains(&flat), "{p:?}");
            assert!((0.5..=1.5).contains(&p.y), "{p:?}");
        }
    }

    #[test]
    fn color_keys_blend_over_remaining_life() {
        let (red, green, blue) = (
            vec3(1.0, 0.0, 0.0),
            vec3(0.0, 1.0, 0.0),
            vec3(0.0, 0.0, 1.0),
        );
        let system =
            ParticleSystem::new().with_color_keys(vec![(2.0, red), (1.0, green), (0.0, blue)]);
        let close = |a: Vector3<f32>, b: Vector3<f32>| (a - b).magnitude2() < 1e-6;
        assert!(
            close(system.color_at(5.0), red),
            "held before the first key"
        );
        assert!(close(system.color_at(1.5), vec3(0.5, 0.5, 0.0)));
        assert!(close(system.color_at(0.5), vec3(0.0, 0.5, 0.5)));
        assert!(close(system.color_at(-1.0), blue), "held past the last key");
        assert!(close(
            ParticleSystem::new().with_color(red).color_at(1.0),
            red
        ));
    }

    fn continuous(launch_time: f32, max: usize) -> ParticleSystem {
        ParticleSystem::new()
            .with_num_particles(max)
            .with_lifetime(10.0, 10.0)
            .with_launch_bounding_box(vec3(0.0, 0.0, 0.0), vec3(0.0, 0.0, 0.0))
            .with_velocity(vec3(0.0, 0.0, 0.0), vec3(0.0, 0.0, 0.0))
            .with_launch_time(Duration::from_secs_f32(launch_time))
    }

    #[test]
    fn continuous_emitter_keeps_its_rate_between_frames() {
        let frame = Duration::from_secs_f64(1.0 / 60.0);
        // 20 ms is not a frame multiple; one exact frame must not slip to two.
        for (period, expected) in [(0.02, 50), (1.0 / 60.0, 60), (0.005, 200)] {
            let mut system = continuous(period, 1000);
            for _ in 0..60 {
                system.update(frame, Matrix4::identity());
            }
            let n = system.particles.len() as i32;
            assert!((n - expected).abs() <= 1, "period {period}: {n} launches");
        }
    }

    #[test]
    fn world_space_launches_spread_along_the_emitter_path() {
        let mut system = continuous(0.004, 1000).with_world_space(true);
        let frame = Duration::from_secs_f64(1.0 / 60.0);
        system.update(frame, Matrix4::from_translation(vec3(0.0, 0.0, 0.0)));
        system.particles.clear();
        system.update(frame, Matrix4::from_translation(vec3(4.0, 0.0, 0.0)));
        let mut xs: Vec<f32> = system.particles.iter().map(|p| p.position.x).collect();
        xs.sort_by(f32::total_cmp);
        assert!(xs.len() >= 4, "{xs:?}");
        assert!(
            xs.windows(2).all(|w| w[1] - w[0] > 0.5),
            "spread, not stacked: {xs:?}"
        );
        assert!(xs.iter().all(|x| (0.0..=4.0).contains(x)), "{xs:?}");
    }

    #[test]
    fn zero_period_launches_once_per_frame() {
        let mut system = continuous(0.0, 1000);
        for _ in 0..10 {
            system.update(Duration::from_millis(16), Matrix4::identity());
        }
        assert_eq!(system.particles.len(), 10);
    }

    #[test]
    fn sprite_animation_wraps_or_holds_the_last_frame() {
        assert_eq!(sprite_frame_index(0.0, 4, 0.25, false), Some(0));
        assert_eq!(sprite_frame_index(0.75, 4, 0.25, false), Some(3));
        assert_eq!(sprite_frame_index(1.25, 4, 0.25, false), Some(3));
        assert_eq!(sprite_frame_index(1.25, 4, 0.25, true), Some(1));
        assert_eq!(sprite_frame_index(1.0, 0, 0.25, true), None);
        assert_eq!(sprite_frame_index(1.0, 4, 0.0, true), Some(0));
    }

    #[test]
    fn shrinking_burst_expires_without_relaunching() {
        let mut system = ParticleSystem::new()
            .with_one_shot(true)
            .with_num_particles(2)
            .with_lifetime(0.5, 0.5)
            .with_particle_size(0.1, 0.1)
            .with_size_velocity(-1.0);
        system.update(Duration::ZERO, Matrix4::identity());
        system.update(Duration::from_millis(250), Matrix4::identity());
        assert_eq!(system.particles.len(), 2);
        assert!(
            system
                .particles
                .iter()
                .all(|p| p.scale == 0.0 && p.age == 0.25)
        );
        system.update(Duration::from_millis(250), Matrix4::identity());
        assert!(system.is_done());
        system.update(Duration::from_secs(1), Matrix4::identity());
        assert!(system.is_done());
    }
}
