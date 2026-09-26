use std::{rc::Rc, sync::Arc, time::Duration};

use cgmath::{Matrix4, SquareMatrix, Vector3, vec3};
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
    //launch_radius: Vector2<f32>,
    launch_lifetime: (f32, f32),
    root_transform: Matrix4<f32>,
    particle_size: (f32, f32),
    /// Tint applied to the sprite (resolved from the group's palette color
    /// index upstream). Defaults to white = untinted full texture.
    color: Vector3<f32>,
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

fn create_random_particle(system: &ParticleSystem, transform: Matrix4<f32>) -> Particle {
    let lifetime = randf(system.launch_lifetime.0, system.launch_lifetime.1);
    let mut position = randv3(system.launch_bounding_box.0, system.launch_bounding_box.1);
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
            launch_lifetime: (1.0, 10.0),
            particles: vec![],
            particle_size: (0.08, 0.08),
            root_transform: Matrix4::identity(),
            color: vec3(1.0, 1.0, 1.0),
            one_shot: false,
            has_launched: false,
            sprite_texture: None,
            sprite_frames: Vec::new(),
            sprite_frame_time: 0.1,
            sprite_looping: false,
            size_velocity: 0.0,
            fade_in_time: 0.0,
            world_space: false,
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

    pub fn with_launch_time(self, launch_time: Duration) -> ParticleSystem {
        ParticleSystem {
            launch_time: launch_time.as_secs_f32(),
            launch_time_remaining: launch_time.as_secs_f32(),
            ..self
        }
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

    pub fn update(&mut self, dt: Duration, transform: Matrix4<f32>) {
        let delta_time = dt.as_secs_f32();
        self.particles.iter_mut().for_each(|p| {
            p.age += delta_time;
            p.remaining_life_in_seconds -= delta_time;
            p.position += p.velocity * delta_time;
            p.velocity += self.acceleration * delta_time;
            p.scale = (p.scale + self.size_velocity * delta_time).max(0.0);
        });

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
        } else if self.particles.len() < self.max_particles && self.launch_time_remaining < 0.0 {
            // Continuous emitter: create a new particle per launch interval.
            self.launch_time_remaining = self.launch_time;
            self.particles.push(create_random_particle(self, transform));
        }

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
                        self.color,
                        0.0,
                        1.0 - (self.particle_alpha * alpha),
                        p.scale,
                    ),
                    None => BillboardMaterial::create(
                        particle_texture.clone(),
                        self.color,
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
