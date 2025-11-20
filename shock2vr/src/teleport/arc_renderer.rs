use cgmath::{Deg, Matrix4, Quaternion, Rotation3, Vector3, vec3};
use engine::scene::{BillboardMaterial, SceneObject, VertexPosition, basic_material, quad};
use engine::texture_format::TextureFormat;
use once_cell::sync::OnceCell;
use std::sync::Arc;

use super::ArcTrajectory;

#[derive(Clone, Copy)]
pub struct ArcRenderConfig {
    pub landing_scale: Vector3<f32>,
    pub landing_height_offset: f32,
}

impl Default for ArcRenderConfig {
    fn default() -> Self {
        Self {
            landing_scale: vec3(0.3, 0.02, 0.3),
            landing_height_offset: 0.02,
        }
    }
}

pub struct ArcRenderer;

impl ArcRenderer {
    /// Create a line mesh matching the arc trajectory for quick visualization.
    pub fn create_arc_lines(
        trajectory: &ArcTrajectory,
        color: Vector3<f32>,
    ) -> Option<SceneObject> {
        if trajectory.points.len() < 2 {
            return None;
        }

        let mut vertices = Vec::with_capacity(trajectory.points.len().saturating_sub(1) * 2);

        for pair in trajectory.points.windows(2) {
            vertices.push(VertexPosition { position: pair[0] });
            vertices.push(VertexPosition { position: pair[1] });
        }

        if vertices.len() < 2 {
            return None;
        }

        let material = engine::scene::color_material::create(color);
        let mesh = engine::scene::lines_mesh::create(vertices);
        let mut arc = SceneObject::new(material, Box::new(mesh));
        arc.set_depth_write(false);
        Some(arc)
    }

    /// Create a particle trail along the arc trajectory for better VR visibility.
    /// Uses billboard particles that always face the camera for optimal viewing.
    /// Generates a smooth interpolated arc for consistent visual density.
    pub fn create_particle_arc(
        trajectory: &ArcTrajectory,
        color: Vector3<f32>,
    ) -> Vec<SceneObject> {
        if trajectory.points.is_empty() {
            return Vec::new();
        }

        // Generate our own smooth arc for visualization
        let visual_arc = Self::generate_smooth_arc(trajectory);
        let mut particles = Vec::new();

        // Get or create a color-tinted particle texture
        let particle_texture_arc = Self::get_tinted_particle_texture(color);

        // Create particles at sampled positions along the visual arc
        let sample_interval = 2; // Take every 2nd point for good density
        for (i, &position) in visual_arc.iter().enumerate() {
            if i % sample_interval == 0 {
                // Calculate alpha gradient from controller to target
                let alpha_ratio = 1.0 - (i as f32 / visual_arc.len() as f32);
                let alpha = 0.4 + alpha_ratio * 0.6; // Range from 0.4 to 1.0 for better visibility

                // Create billboard material with enhanced emissivity for cyberpunk glow
                let particle_texture: Arc<dyn engine::texture::TextureTrait> =
                    particle_texture_arc.clone();
                let emissivity = (color.x.max(color.y).max(color.z) * 1.5).min(1.0); // Boost emissivity for glow effect
                let material = BillboardMaterial::create(
                    particle_texture,
                    emissivity,  // Enhanced glow
                    1.0 - alpha, // transparency (1.0 = fully transparent)
                    0.06,        // Slightly larger particle size (6cm diameter)
                );

                // Create scene object for this particle
                let mut particle = SceneObject::new(material, Box::new(quad::create()));
                particle.set_transform(Matrix4::from_translation(position));
                particle.set_depth_write(false);
                particles.push(particle);
            }
        }

        particles
    }

    /// Create a textured ring landing indicator so players can see the destination.
    /// Uses the teleport-landing.png asset as a billboard quad with pulsing animation.
    pub fn create_target_indicator(
        position: Vector3<f32>,
        color: Vector3<f32>,
        config: ArcRenderConfig,
        animation_time: f32,
    ) -> SceneObject {
        // Create or get a color-tinted version of the ring texture
        let ring_texture_arc = Self::get_tinted_ring_texture(color);

        // Create ground-oriented material with enhanced cyberpunk glow
        let ring_texture: Arc<dyn engine::texture::TextureTrait> = ring_texture_arc.clone();
        let emissivity = (color.x.max(color.y).max(color.z) * 1.2).min(1.0); // Enhanced glow for cyberpunk effect
        let material = basic_material::create(
            ring_texture,
            emissivity, // Boosted emissivity for better glow
            0.15,       // Slightly less transparency for better visibility
        );

        // Create quad positioned and oriented to face upward (ground normal)
        let mut target = SceneObject::new(material, Box::new(quad::create()));

        // Scale and position the ring with pulsing animation
        let base_ring_size = config.landing_scale.x.max(config.landing_scale.z); // Use larger of x or z
        let pulse_amplitude = 0.15; // 15% size variation
        let pulse_frequency = 2.5; // Pulses per second
        let pulse_factor = 1.0 + pulse_amplitude * (animation_time * pulse_frequency).sin();
        let animated_size = base_ring_size * pulse_factor;

        // Orient the quad to face upward (rotate 90 degrees around X-axis)
        let rotation = Matrix4::from(Quaternion::from_axis_angle(vec3(1.0, 0.0, 0.0), Deg(-90.0)));
        let translation =
            Matrix4::from_translation(position + vec3(0.0, config.landing_height_offset, 0.0));
        let scale = Matrix4::from_scale(animated_size);
        target.set_transform(translation * rotation * scale);
        target.set_depth_write(false);

        target
    }

    /// Get or create a color-tinted version of the ring texture
    fn get_tinted_ring_texture(color: Vector3<f32>) -> Arc<engine::texture::Texture> {
        use std::collections::HashMap;

        // Cache tinted textures by color (quantized to avoid infinite cache growth)
        static TINTED_TEXTURES: OnceCell<
            std::sync::Mutex<HashMap<(u8, u8, u8), Arc<engine::texture::Texture>>>,
        > = OnceCell::new();
        let cache = TINTED_TEXTURES.get_or_init(|| std::sync::Mutex::new(HashMap::new()));

        // Quantize color to reduce cache size
        let color_key = (
            (color.x * 255.0) as u8,
            (color.y * 255.0) as u8,
            (color.z * 255.0) as u8,
        );

        if let Ok(mut cache_guard) = cache.lock() {
            if let Some(texture) = cache_guard.get(&color_key) {
                return texture.clone();
            }

            // Create new tinted texture
            let tinted_texture = Self::create_tinted_ring_texture(color);
            cache_guard.insert(color_key, tinted_texture.clone());
            tinted_texture
        } else {
            // Fallback if mutex is poisoned
            Self::create_tinted_ring_texture(color)
        }
    }

    /// Create a new color-tinted ring texture
    fn create_tinted_ring_texture(color: Vector3<f32>) -> Arc<engine::texture::Texture> {
        // Try to load the PNG asset first
        let base_texture_data = match std::fs::read("assets/teleport-landing.png") {
            Ok(buffer) => {
                let format = &engine::texture_format::PNG;
                format.load(&buffer)
            }
            Err(_) => {
                // Fallback to programmatic ring
                Self::create_fallback_ring_data()
            }
        };

        // Apply color tinting to the texture data
        let mut tinted_data = base_texture_data.bytes.clone();

        // Tint each pixel by multiplying RGB channels with the color
        for i in (0..tinted_data.len()).step_by(4) {
            if i + 3 < tinted_data.len() {
                let original_r = tinted_data[i] as f32 / 255.0;
                let original_g = tinted_data[i + 1] as f32 / 255.0;
                let original_b = tinted_data[i + 2] as f32 / 255.0;

                // Apply color tinting while preserving brightness
                tinted_data[i] = (original_r * color.x * 255.0) as u8;
                tinted_data[i + 1] = (original_g * color.y * 255.0) as u8;
                tinted_data[i + 2] = (original_b * color.z * 255.0) as u8;
                // Keep alpha unchanged
            }
        }

        let tinted_texture_data = engine::texture_format::RawTextureData {
            bytes: tinted_data,
            width: base_texture_data.width,
            height: base_texture_data.height,
            format: base_texture_data.format,
        };

        Arc::new(engine::texture::init_from_memory(tinted_texture_data))
    }

    /// Create fallback ring texture data (for color tinting)
    fn create_fallback_ring_data() -> engine::texture_format::RawTextureData {
        let size = 128u32;
        let center = size as f32 / 2.0;
        let outer_radius = center * 0.8;
        let inner_radius = center * 0.5;

        let mut texture_data = engine::texture_format::RawTextureData {
            bytes: vec![0; (size * size * 4) as usize],
            width: size,
            height: size,
            format: engine::texture_format::PixelFormat::RGBA,
        };

        for x in 0..size {
            for y in 0..size {
                let dx = x as f32 - center;
                let dy = y as f32 - center;
                let distance = (dx * dx + dy * dy).sqrt();

                let alpha = if distance <= outer_radius && distance >= inner_radius {
                    // Create a ring shape with soft edges
                    let outer_fade =
                        1.0 - ((distance - outer_radius + 10.0) / 10.0).max(0.0).min(1.0);
                    let inner_fade = ((distance - inner_radius + 10.0) / 10.0).max(0.0).min(1.0);
                    (255.0 * outer_fade * inner_fade) as u8
                } else {
                    0
                };

                let index = ((y * size + x) * 4) as usize;
                texture_data.bytes[index] = 255; // Red
                texture_data.bytes[index + 1] = 255; // Green
                texture_data.bytes[index + 2] = 255; // Blue
                texture_data.bytes[index + 3] = alpha; // Alpha
            }
        }

        texture_data
    }

    /// Generate a smooth visual arc for consistent particle density
    /// regardless of the original trajectory point count
    fn generate_smooth_arc(trajectory: &ArcTrajectory) -> Vec<Vector3<f32>> {
        if trajectory.points.is_empty() {
            return Vec::new();
        }

        let start_point = trajectory.points[0];
        let end_point = if let Some(landing_pos) = trajectory.landing_position {
            landing_pos
        } else {
            // If no landing position, use the last trajectory point
            trajectory.points[trajectory.points.len() - 1]
        };

        // Generate a smooth parabolic arc with consistent point density
        let num_points = 25; // Fixed number for consistent visual density
        let mut arc_points = Vec::with_capacity(num_points);

        // Calculate arc parameters
        let horizontal_distance =
            ((end_point.x - start_point.x).powi(2) + (end_point.z - start_point.z).powi(2)).sqrt();
        let height_difference = end_point.y - start_point.y;

        // Create a parabolic arc that goes through start and end points
        for i in 0..num_points {
            let t = i as f32 / (num_points - 1) as f32; // 0.0 to 1.0

            // Linear interpolation for x and z
            let x = start_point.x + t * (end_point.x - start_point.x);
            let z = start_point.z + t * (end_point.z - start_point.z);

            // Parabolic interpolation for y (creates an arc)
            let arc_height = horizontal_distance * 0.3; // Arc height as fraction of distance
            let parabolic_offset = 4.0 * arc_height * t * (1.0 - t); // Parabolic curve
            let y = start_point.y + t * height_difference + parabolic_offset;

            arc_points.push(vec3(x, y, z));
        }

        arc_points
    }

    /// Get or create a color-tinted particle texture for arc visualization
    fn get_tinted_particle_texture(color: Vector3<f32>) -> Arc<engine::texture::Texture> {
        use std::collections::HashMap;

        // Cache tinted particle textures by color (quantized to avoid infinite cache growth)
        static TINTED_PARTICLE_TEXTURES: OnceCell<
            std::sync::Mutex<HashMap<(u8, u8, u8), Arc<engine::texture::Texture>>>,
        > = OnceCell::new();
        let cache = TINTED_PARTICLE_TEXTURES.get_or_init(|| std::sync::Mutex::new(HashMap::new()));

        // Quantize color to reduce cache size
        let color_key = (
            (color.x * 255.0) as u8,
            (color.y * 255.0) as u8,
            (color.z * 255.0) as u8,
        );

        if let Ok(mut cache_guard) = cache.lock() {
            if let Some(texture) = cache_guard.get(&color_key) {
                return texture.clone();
            }

            // Create new tinted particle texture
            let tinted_texture = Self::create_tinted_particle_texture(color);
            cache_guard.insert(color_key, tinted_texture.clone());
            tinted_texture
        } else {
            // Fallback if mutex is poisoned
            Self::create_tinted_particle_texture(color)
        }
    }

    /// Create a color-tinted particle texture
    fn create_tinted_particle_texture(color: Vector3<f32>) -> Arc<engine::texture::Texture> {
        let size = 64u32;
        let center = size as f32 / 2.0;
        let radius = center * 0.8;

        let mut texture_data = engine::texture_format::RawTextureData {
            bytes: vec![0; (size * size * 4) as usize],
            width: size,
            height: size,
            format: engine::texture_format::PixelFormat::RGBA,
        };

        for x in 0..size {
            for y in 0..size {
                let dx = x as f32 - center;
                let dy = y as f32 - center;
                let distance = (dx * dx + dy * dy).sqrt();

                let alpha = if distance <= radius {
                    let ratio = 1.0 - (distance / radius);
                    (255.0 * ratio * ratio) as u8
                } else {
                    0
                };

                let index = ((y * size + x) * 4) as usize;
                // Apply color tinting to create colored particles
                texture_data.bytes[index] = (255.0 * color.x) as u8; // Red channel
                texture_data.bytes[index + 1] = (255.0 * color.y) as u8; // Green channel
                texture_data.bytes[index + 2] = (255.0 * color.z) as u8; // Blue channel
                texture_data.bytes[index + 3] = alpha; // Alpha
            }
        }

        Arc::new(engine::texture::init_from_memory(texture_data))
    }
}
