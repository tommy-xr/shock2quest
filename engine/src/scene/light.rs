use cgmath::{Vector3, Vector4, InnerSpace};

/// Spotlight-specific parameters for shader uniforms
#[derive(Debug, Clone, Copy)]
pub struct SpotlightParams {
    pub direction: Vector3<f32>,
    pub inner_cone_angle: f32,
    pub outer_cone_angle: f32,
    pub range: f32,
}

/// Base trait for all light types in the multi-pass lighting system
pub trait Light: std::fmt::Debug {
    /// Get the light's position in world space
    fn position(&self) -> Vector3<f32>;

    /// Get the light's color (RGB) and intensity (A)
    fn color_intensity(&self) -> Vector4<f32>;

    /// Get the light's type identifier for shader selection
    fn light_type(&self) -> LightType;

    /// Check if this light affects a given world position
    /// Used for optimization to skip lights that don't affect geometry
    fn affects_position(&self, world_pos: Vector3<f32>) -> bool;

    /// Get spotlight parameters if this is a spotlight
    /// Returns None for other light types
    fn spotlight_params(&self) -> Option<SpotlightParams> {
        None
    }
}

/// Light type enumeration for shader dispatch
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LightType {
    Spotlight,
    // Future extensions:
    // PointLight,
    // DirectionalLight,
}

/// Spotlight implementation with direction and cone angles
#[derive(Debug, Clone)]
pub struct SpotLight {
    /// World space position
    pub position: Vector3<f32>,

    /// Light direction (normalized)
    pub direction: Vector3<f32>,

    /// RGB color and intensity in the alpha channel
    pub color_intensity: Vector4<f32>,

    /// Inner cone angle in radians (full brightness)
    pub inner_cone_angle: f32,

    /// Outer cone angle in radians (falloff to zero)
    pub outer_cone_angle: f32,

    /// Maximum range of the light (for optimization)
    pub range: f32,
}

impl Light for SpotLight {
    fn position(&self) -> Vector3<f32> {
        self.position
    }

    fn color_intensity(&self) -> Vector4<f32> {
        self.color_intensity
    }

    fn light_type(&self) -> LightType {
        LightType::Spotlight
    }

    fn affects_position(&self, world_pos: Vector3<f32>) -> bool {
        // Quick range check
        let distance = (world_pos - self.position).magnitude();
        if distance > self.range {
            return false;
        }

        // Cone check - ensure position is within the outer cone
        if distance > 0.0 {
            let to_position = (world_pos - self.position).normalize();
            let dot = to_position.dot(self.direction);
            let cos_outer = self.outer_cone_angle.cos();
            dot >= cos_outer
        } else {
            // Position is exactly at light source
            true
        }
    }

    fn spotlight_params(&self) -> Option<SpotlightParams> {
        Some(SpotlightParams {
            direction: self.direction,
            inner_cone_angle: self.inner_cone_angle,
            outer_cone_angle: self.outer_cone_angle,
            range: self.range,
        })
    }
}

impl SpotLight {
    /// Create a new spotlight with default parameters
    pub fn new(
        position: Vector3<f32>,
        direction: Vector3<f32>,
        color: Vector3<f32>,
        intensity: f32
    ) -> Self {
        Self {
            position,
            direction: direction.normalize(),
            color_intensity: Vector4::new(color.x, color.y, color.z, intensity),
            inner_cone_angle: std::f32::consts::FRAC_PI_8, // 22.5 degrees
            outer_cone_angle: std::f32::consts::FRAC_PI_4, // 45 degrees
            range: 10.0,
        }
    }

    /// Create a flashlight-style spotlight with narrow cone
    pub fn flashlight(position: Vector3<f32>, direction: Vector3<f32>, intensity: f32) -> Self {
        Self {
            position,
            direction: direction.normalize(),
            color_intensity: Vector4::new(1.0, 1.0, 0.9, intensity), // Warm white
            inner_cone_angle: std::f32::consts::FRAC_PI_8, // 22.5 degrees
            outer_cone_angle: std::f32::consts::FRAC_PI_6, // 30 degrees
            range: 15.0,
        }
    }

    /// Get the spotlight's attenuation factor at a given world position
    /// Returns 0.0 if outside the light's influence, 1.0 at full brightness
    pub fn attenuation_at(&self, world_pos: Vector3<f32>) -> f32 {
        let distance = (world_pos - self.position).magnitude();

        // Range falloff
        if distance > self.range {
            return 0.0;
        }

        // Distance attenuation (quadratic falloff)
        let distance_attenuation = if distance > 0.0 {
            1.0 / (1.0 + 0.1 * distance + 0.01 * distance * distance)
        } else {
            1.0
        };

        // Cone attenuation
        let cone_attenuation = if distance > 0.0 {
            let to_position = (world_pos - self.position).normalize();
            let dot = to_position.dot(self.direction);
            let cos_outer = self.outer_cone_angle.cos();
            let cos_inner = self.inner_cone_angle.cos();

            if dot < cos_outer {
                0.0
            } else if dot > cos_inner {
                1.0
            } else {
                // Smooth falloff between inner and outer cone
                (dot - cos_outer) / (cos_inner - cos_outer)
            }
        } else {
            1.0
        };

        distance_attenuation * cone_attenuation
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_spotlight_creation() {
        let light = SpotLight::new(
            Vector3::new(0.0, 1.0, 0.0),
            Vector3::new(0.0, -1.0, 0.0),
            Vector3::new(1.0, 1.0, 1.0),
            1.0
        );

        assert_eq!(light.position(), Vector3::new(0.0, 1.0, 0.0));
        assert_eq!(light.color_intensity(), Vector4::new(1.0, 1.0, 1.0, 1.0));
        assert_eq!(light.light_type(), LightType::Spotlight);
    }

    #[test]
    fn test_spotlight_affects_position() {
        let light = SpotLight::new(
            Vector3::new(0.0, 2.0, 0.0),
            Vector3::new(0.0, -1.0, 0.0), // Pointing down
            Vector3::new(1.0, 1.0, 1.0),
            1.0
        );

        // Position directly below should be affected
        assert!(light.affects_position(Vector3::new(0.0, 0.0, 0.0)));

        // Position to the side within cone should be affected
        assert!(light.affects_position(Vector3::new(0.5, 1.0, 0.0)));

        // Position outside range should not be affected
        assert!(!light.affects_position(Vector3::new(0.0, -20.0, 0.0)));

        // Position outside cone should not be affected
        assert!(!light.affects_position(Vector3::new(10.0, 1.0, 0.0)));
    }

    #[test]
    fn test_spotlight_attenuation() {
        let light = SpotLight::new(
            Vector3::new(0.0, 1.0, 0.0),
            Vector3::new(0.0, -1.0, 0.0),
            Vector3::new(1.0, 1.0, 1.0),
            1.0
        );

        // Position at light source should have high attenuation
        assert!(light.attenuation_at(Vector3::new(0.0, 1.0, 0.0)) > 0.9);

        // Position directly below in center of cone should have good attenuation
        assert!(light.attenuation_at(Vector3::new(0.0, 0.0, 0.0)) > 0.1);

        // Position outside range should have zero attenuation
        assert_eq!(light.attenuation_at(Vector3::new(0.0, -20.0, 0.0)), 0.0);
    }
}