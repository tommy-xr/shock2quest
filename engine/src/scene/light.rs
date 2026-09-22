use cgmath::{InnerSpace, Vector3, Vector4};

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
    /// Casts in every direction. In the shader these share the spotlight slots
    /// and are told apart by a negative inner cone angle - see
    /// [`SceneLight::inner_cone_angle`].
    PointLight,
}

/// An omni light: a position, a colour and a reach, with no cone.
#[derive(Debug, Clone)]
pub struct PointLight {
    /// World space position
    pub position: Vector3<f32>,

    /// RGB color and intensity in the alpha channel
    pub color_intensity: Vector4<f32>,

    /// Maximum range of the light (for optimization)
    pub range: f32,
}

impl Light for PointLight {
    fn position(&self) -> Vector3<f32> {
        self.position
    }

    fn color_intensity(&self) -> Vector4<f32> {
        self.color_intensity
    }

    fn light_type(&self) -> LightType {
        LightType::PointLight
    }

    fn affects_position(&self, world_pos: Vector3<f32>) -> bool {
        (world_pos - self.position).magnitude() <= self.range
    }
}

/// A light occupying one of the renderer's light slots. Both kinds upload
/// through the same uniforms; a point light is a spotlight slot whose cone
/// covers everything.
#[derive(Debug, Clone)]
pub enum SceneLight {
    Spot(SpotLight),
    Point(PointLight),
}

/// Marks a slot as having no cone. Angles are otherwise non-negative, so the
/// shader can branch on the sign rather than carry another uniform array
/// through every material.
pub const NO_CONE: f32 = -1.0;

impl SceneLight {
    pub fn position(&self) -> Vector3<f32> {
        match self {
            SceneLight::Spot(light) => light.position,
            SceneLight::Point(light) => light.position,
        }
    }

    pub fn color_intensity(&self) -> Vector4<f32> {
        match self {
            SceneLight::Spot(light) => light.color_intensity,
            SceneLight::Point(light) => light.color_intensity,
        }
    }

    pub fn direction(&self) -> Vector3<f32> {
        match self {
            SceneLight::Spot(light) => light.direction,
            SceneLight::Point(_) => Vector3::new(0.0, 0.0, 0.0),
        }
    }

    /// Negative for a point light - that is how the shader tells the two apart.
    pub fn inner_cone_angle(&self) -> f32 {
        match self {
            SceneLight::Spot(light) => light.inner_cone_angle,
            SceneLight::Point(_) => NO_CONE,
        }
    }

    pub fn outer_cone_angle(&self) -> f32 {
        match self {
            SceneLight::Spot(light) => light.outer_cone_angle,
            SceneLight::Point(_) => NO_CONE,
        }
    }

    pub fn range(&self) -> f32 {
        match self {
            SceneLight::Spot(light) => light.range,
            SceneLight::Point(light) => light.range,
        }
    }

    pub fn affects_position(&self, world_pos: Vector3<f32>) -> bool {
        match self {
            SceneLight::Spot(light) => light.affects_position(world_pos),
            SceneLight::Point(light) => light.affects_position(world_pos),
        }
    }
}

impl From<SpotLight> for SceneLight {
    fn from(light: SpotLight) -> Self {
        SceneLight::Spot(light)
    }
}

impl From<PointLight> for SceneLight {
    fn from(light: PointLight) -> Self {
        SceneLight::Point(light)
    }
}

/// Container for managing up to 6 lights for single-pass lighting
#[derive(Debug, Clone)]
pub struct LightArray {
    /// Array of up to 6 lights (None = disabled slot)
    pub lights: [Option<SceneLight>; 6],
}

impl LightArray {
    /// Create a new empty light array
    pub fn new() -> Self {
        Self {
            lights: [None, None, None, None, None, None],
        }
    }

    /// Add a light to the first available slot
    /// Returns the slot index if successful, None if array is full
    pub fn add_light(&mut self, light: impl Into<SceneLight>) -> Option<usize> {
        let light = light.into();
        for (i, slot) in self.lights.iter_mut().enumerate() {
            if slot.is_none() {
                *slot = Some(light);
                return Some(i);
            }
        }
        None
    }

    /// Get a reference to the light in the specified slot
    pub fn get_light(&self, index: usize) -> Option<&SceneLight> {
        self.lights.get(index).and_then(|slot| slot.as_ref())
    }

    /// Clear all lights
    pub fn clear(&mut self) {
        self.lights = [None, None, None, None, None, None];
    }

    /// Get the number of active lights
    pub fn active_count(&self) -> usize {
        self.lights.iter().filter(|s| s.is_some()).count()
    }

    /// Check if the array is empty
    pub fn is_empty(&self) -> bool {
        self.lights.iter().all(|s| s.is_none())
    }

    /// Check if the array is full
    pub fn is_full(&self) -> bool {
        self.lights.iter().all(|s| s.is_some())
    }

    /// Iterator over active lights with their indices
    pub fn iter_active(&self) -> impl Iterator<Item = (usize, &SceneLight)> {
        self.lights
            .iter()
            .enumerate()
            .filter_map(|(i, light)| light.as_ref().map(|l| (i, l)))
    }
}

impl Default for LightArray {
    fn default() -> Self {
        Self::new()
    }
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
        intensity: f32,
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
            inner_cone_angle: std::f32::consts::FRAC_PI_8,           // 22.5 degrees
            outer_cone_angle: std::f32::consts::FRAC_PI_6,           // 30 degrees
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
            1.0,
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
            1.0,
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
            1.0,
        );

        // Position at light source should have high attenuation
        assert!(light.attenuation_at(Vector3::new(0.0, 1.0, 0.0)) > 0.9);

        // Position directly below in center of cone should have good attenuation
        assert!(light.attenuation_at(Vector3::new(0.0, 0.0, 0.0)) > 0.1);

        // Position outside range should have zero attenuation
        assert_eq!(light.attenuation_at(Vector3::new(0.0, -20.0, 0.0)), 0.0);
    }

    #[test]
    fn test_light_array_creation() {
        let light_array = LightArray::new();
        assert!(light_array.is_empty());
        assert!(!light_array.is_full());
        assert_eq!(light_array.active_count(), 0);
    }

    #[test]
    fn test_light_array_add_remove() {
        let mut light_array = LightArray::new();
        let light = SpotLight::new(
            Vector3::new(0.0, 1.0, 0.0),
            Vector3::new(0.0, -1.0, 0.0),
            Vector3::new(1.0, 1.0, 1.0),
            1.0,
        );

        // Add light
        let index = light_array.add_light(light.clone()).unwrap();
        assert_eq!(index, 0);
        assert_eq!(light_array.active_count(), 1);
        assert!(!light_array.is_empty());

        // Get light
        let retrieved = light_array.get_light(0).unwrap();
        assert_eq!(retrieved.position(), light.position());

        // Clearing empties every slot - the array is rebuilt each frame
        light_array.clear();
        assert!(light_array.is_empty());
        assert_eq!(light_array.active_count(), 0);
    }

    /// The shader tells a point light from a spotlight by the sign of the inner
    /// cone angle, so a point light must report a negative one and a spotlight
    /// must not.
    #[test]
    fn point_lights_are_marked_by_a_negative_inner_cone() {
        let point: SceneLight = PointLight {
            position: Vector3::new(1.0, 2.0, 3.0),
            color_intensity: Vector4::new(1.0, 1.0, 1.0, 1.0),
            range: 10.0,
        }
        .into();
        assert!(point.inner_cone_angle() < 0.0);
        assert!(point.outer_cone_angle() < 0.0);
        assert_eq!(point.range(), 10.0);

        let spot: SceneLight = SpotLight::new(
            Vector3::new(0.0, 1.0, 0.0),
            Vector3::new(0.0, -1.0, 0.0),
            Vector3::new(1.0, 1.0, 1.0),
            1.0,
        )
        .into();
        assert!(
            spot.inner_cone_angle() >= 0.0,
            "a real cone must not look like a point light"
        );
    }

    #[test]
    fn a_point_light_reaches_in_every_direction_within_range() {
        let light = PointLight {
            position: Vector3::new(0.0, 0.0, 0.0),
            color_intensity: Vector4::new(1.0, 1.0, 1.0, 1.0),
            range: 5.0,
        };

        for direction in [
            Vector3::new(5.0, 0.0, 0.0),
            Vector3::new(-5.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, -4.9),
        ] {
            assert!(light.affects_position(direction), "{direction:?}");
        }
        assert!(!light.affects_position(Vector3::new(0.0, 5.1, 0.0)));
    }

    #[test]
    fn a_light_array_holds_both_kinds() {
        let mut light_array = LightArray::new();
        light_array.add_light(SpotLight::new(
            Vector3::new(0.0, 1.0, 0.0),
            Vector3::new(0.0, -1.0, 0.0),
            Vector3::new(1.0, 1.0, 1.0),
            1.0,
        ));
        light_array.add_light(PointLight {
            position: Vector3::new(3.0, 0.0, 0.0),
            color_intensity: Vector4::new(1.0, 0.0, 0.0, 2.0),
            range: 8.0,
        });

        assert_eq!(light_array.active_count(), 2);
        assert!(light_array.get_light(0).unwrap().inner_cone_angle() >= 0.0);
        assert!(light_array.get_light(1).unwrap().inner_cone_angle() < 0.0);
    }

    #[test]
    fn test_light_array_fill_capacity() {
        let mut light_array = LightArray::new();
        let light = SpotLight::new(
            Vector3::new(0.0, 1.0, 0.0),
            Vector3::new(0.0, -1.0, 0.0),
            Vector3::new(1.0, 1.0, 1.0),
            1.0,
        );

        // Fill array to capacity
        for i in 0..6 {
            let index = light_array.add_light(light.clone()).unwrap();
            assert_eq!(index, i);
        }

        assert!(light_array.is_full());
        assert_eq!(light_array.active_count(), 6);

        // Try to add one more (should fail)
        assert!(light_array.add_light(light).is_none());
    }

    #[test]
    fn test_light_array_iter_active() {
        let mut light_array = LightArray::new();
        let light1 = SpotLight::new(
            Vector3::new(1.0, 0.0, 0.0),
            Vector3::new(0.0, -1.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0),
            1.0,
        );
        let light2 = SpotLight::new(
            Vector3::new(2.0, 0.0, 0.0),
            Vector3::new(0.0, -1.0, 0.0),
            Vector3::new(0.0, 1.0, 0.0),
            1.0,
        );

        light_array.add_light(light1);
        light_array.add_light(light2);

        let active_lights: Vec<_> = light_array.iter_active().collect();
        assert_eq!(active_lights.len(), 2);
        assert_eq!(active_lights[0].0, 0); // First light at index 0
        assert_eq!(active_lights[1].0, 1); // Second light at index 1
    }
}
