use std::{sync::Arc, time::Duration};

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

fn create_random_particle(system: &ParticleSystem) -> Particle {
    let lifetime = randf(system.launch_lifetime.0, system.launch_lifetime.1);
    let position = randv3(system.launch_bounding_box.0, system.launch_bounding_box.1);
    let velocity = randv3(system.launch_velocity.0, system.launch_velocity.1);
    let scale = randf(system.particle_size.0, system.particle_size.1);
    Particle {
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
        }
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
            p.remaining_life_in_seconds -= delta_time;
            p.position += p.velocity * delta_time;
            p.velocity += self.acceleration * delta_time;
        });

        self.particles.retain(|p| p.remaining_life_in_seconds > 0.0);

        self.launch_time_remaining -= delta_time;

        // Check if we should create a new particle
        if self.particles.len() < self.max_particles && self.launch_time_remaining < 0.0 {
            self.launch_time_remaining = self.launch_time;
            self.particles.push(create_random_particle(self));
        }

        self.root_transform = transform
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
                // TODO: Switch to billboard material
                //    let mat = BillboardMaterial::create(some_texture, 1.0, 0.0);
                let mat = BillboardMaterial::create(
                    particle_texture.clone(),
                    1.0,
                    1.0 - (self.particle_alpha * alpha),
                    p.scale,
                );
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

struct Particle {
    remaining_life_in_seconds: f32,
    position: Vector3<f32>,
    velocity: Vector3<f32>,
    scale: f32,
}
