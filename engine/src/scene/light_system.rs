use crate::scene::light::{Light, LightType, SpotLight, SpotlightParams};
use cgmath::{Vector3, Vector4};

pub const MAX_SPOT_LIGHTS: usize = 2;

#[derive(Clone, Copy, Debug)]
pub struct SpotlightUniform {
    pub position: Vector3<f32>,
    pub range: f32,
    pub color_intensity: Vector4<f32>,
    pub direction: Vector3<f32>,
    pub cos_inner: f32,
    pub cos_outer: f32,
}

impl SpotlightUniform {
    fn from_light(light: &dyn Light, params: SpotlightParams) -> Self {
        Self {
            position: light.position(),
            range: params.range,
            color_intensity: light.color_intensity(),
            direction: params.direction,
            cos_inner: params.inner_cone_angle.cos(),
            cos_outer: params.outer_cone_angle.cos(),
        }
    }
}

impl Default for SpotlightUniform {
    fn default() -> Self {
        Self {
            position: Vector3::new(0.0, 0.0, 0.0),
            range: 0.0,
            color_intensity: Vector4::new(0.0, 0.0, 0.0, 0.0),
            direction: Vector3::new(0.0, -1.0, 0.0),
            cos_inner: 0.0,
            cos_outer: 0.0,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct LightingBatch {
    pub spot_count: usize,
    pub spots: [SpotlightUniform; MAX_SPOT_LIGHTS],
}

impl Default for LightingBatch {
    fn default() -> Self {
        Self {
            spot_count: 0,
            spots: [SpotlightUniform::default(); MAX_SPOT_LIGHTS],
        }
    }
}

impl LightingBatch {
    pub fn is_empty(&self) -> bool {
        self.spot_count == 0
    }
}

/// Container for managing multiple lights in a scene
/// This provides the foundation for multi-pass lighting while maintaining
/// flexibility for future optimizations like spatial partitioning
#[derive(Clone)]
pub struct LightSystem {
    /// All lights in the scene
    lights: Vec<Box<dyn Light>>,

    /// Cached dirty flag for optimization
    dirty: bool,
}

impl LightSystem {
    /// Create a new empty light system
    pub fn new() -> Self {
        Self {
            lights: Vec::new(),
            dirty: false,
        }
    }

    /// Add a light to the system
    pub fn add_light(&mut self, light: Box<dyn Light>) {
        self.lights.push(light);
        self.dirty = true;
    }

    /// Add a spotlight to the system (convenience method)
    pub fn add_spotlight(&mut self, spotlight: SpotLight) {
        self.add_light(Box::new(spotlight));
    }

    /// Get all lights in the system
    pub fn lights(&self) -> &[Box<dyn Light>] {
        &self.lights
    }

    /// Get lights that affect a given world position
    /// This is used for optimization in the rendering pipeline
    pub fn lights_affecting_position(&self, world_pos: Vector3<f32>) -> Vec<&dyn Light> {
        self.lights
            .iter()
            .filter_map(|light| {
                if light.affects_position(world_pos) {
                    Some(light.as_ref())
                } else {
                    None
                }
            })
            .collect()
    }

    /// Gather up to MAX_SPOT_LIGHTS spotlights influencing the provided world position.
    pub fn gather_spotlights_for_position(&self, world_pos: Vector3<f32>) -> LightingBatch {
        use std::cmp::Ordering;

        let mut candidates: Vec<(f32, &dyn Light)> = self
            .lights
            .iter()
            .filter_map(|light| {
                if light.light_type() != LightType::Spotlight {
                    return None;
                }

                let influence = light.influence_at(world_pos);
                if influence <= 0.0 {
                    return None;
                }

                let intensity = light.color_intensity().w;
                Some((influence * intensity, light.as_ref()))
            })
            .collect();

        candidates.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or_else(|| Ordering::Equal));

        let mut batch = LightingBatch::default();

        for (index, (_, light)) in candidates.into_iter().take(MAX_SPOT_LIGHTS).enumerate() {
            if let Some(params) = light.spotlight_params() {
                batch.spots[index] = SpotlightUniform::from_light(light, params);
                batch.spot_count += 1;
            }
        }

        batch
    }

    /// Get lights by type (for shader optimization)
    pub fn lights_by_type(&self, light_type: LightType) -> Vec<&dyn Light> {
        self.lights
            .iter()
            .filter_map(|light| {
                if light.light_type() == light_type {
                    Some(light.as_ref())
                } else {
                    None
                }
            })
            .collect()
    }

    /// Clear all lights from the system
    pub fn clear(&mut self) {
        self.lights.clear();
        self.dirty = true;
    }

    /// Get the number of lights in the system
    pub fn light_count(&self) -> usize {
        self.lights.len()
    }

    /// Check if the light system has been modified since last check
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// Mark the light system as clean (used after processing changes)
    pub fn mark_clean(&mut self) {
        self.dirty = false;
    }

    /// Remove lights that don't affect any geometry in a given bounding volume
    /// This is a future optimization point for portal culling integration
    pub fn cull_lights_for_bounds(
        &self,
        min_bounds: Vector3<f32>,
        max_bounds: Vector3<f32>,
    ) -> Vec<&dyn Light> {
        // For now, just check if any corner of the bounding box is affected
        // This is a conservative approach that ensures we don't miss any lights
        let corners = [
            Vector3::new(min_bounds.x, min_bounds.y, min_bounds.z),
            Vector3::new(max_bounds.x, min_bounds.y, min_bounds.z),
            Vector3::new(min_bounds.x, max_bounds.y, min_bounds.z),
            Vector3::new(min_bounds.x, min_bounds.y, max_bounds.z),
            Vector3::new(max_bounds.x, max_bounds.y, min_bounds.z),
            Vector3::new(max_bounds.x, min_bounds.y, max_bounds.z),
            Vector3::new(min_bounds.x, max_bounds.y, max_bounds.z),
            Vector3::new(max_bounds.x, max_bounds.y, max_bounds.z),
        ];

        self.lights
            .iter()
            .filter_map(|light| {
                // Check if light affects any corner of the bounding box
                let affects_bounds = corners.iter().any(|&corner| light.affects_position(corner));

                if affects_bounds {
                    Some(light.as_ref())
                } else {
                    None
                }
            })
            .collect()
    }
}

impl Default for LightSystem {
    fn default() -> Self {
        Self::new()
    }
}

// Note: We can't derive Clone for Box<dyn Light> automatically, so we implement it manually
// This is needed for the LightSystem to be cloneable, but we'll avoid cloning when possible
impl Clone for Box<dyn Light> {
    fn clone(&self) -> Self {
        // For now, we'll implement cloning for SpotLight only
        // This can be extended as we add more light types
        match self.light_type() {
            LightType::Spotlight => {
                // This is a bit of a hack, but works for our current needs
                // In a more complex system, we'd use an enum instead of trait objects
                let pos = self.position();
                let color = self.color_intensity();

                // Create a default spotlight with the basic properties
                // Note: This loses some specific spotlight properties like cone angles
                // A future improvement would be to add a clone method to the Light trait
                Box::new(SpotLight::new(
                    pos,
                    Vector3::new(0.0, -1.0, 0.0), // Default direction
                    Vector3::new(color.x, color.y, color.z),
                    color.w,
                ))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_light_system_creation() {
        let system = LightSystem::new();
        assert_eq!(system.light_count(), 0);
        assert!(!system.is_dirty());
    }

    #[test]
    fn test_add_spotlight() {
        let mut system = LightSystem::new();

        let spotlight = SpotLight::new(
            Vector3::new(0.0, 1.0, 0.0),
            Vector3::new(0.0, -1.0, 0.0),
            Vector3::new(1.0, 1.0, 1.0),
            1.0,
        );

        system.add_spotlight(spotlight);

        assert_eq!(system.light_count(), 1);
        assert!(system.is_dirty());
    }

    #[test]
    fn test_lights_affecting_position() {
        let mut system = LightSystem::new();

        // Add a spotlight pointing down
        let spotlight = SpotLight::new(
            Vector3::new(0.0, 2.0, 0.0),
            Vector3::new(0.0, -1.0, 0.0),
            Vector3::new(1.0, 1.0, 1.0),
            1.0,
        );
        system.add_spotlight(spotlight);

        // Position under the light should be affected
        let affected_lights = system.lights_affecting_position(Vector3::new(0.0, 0.0, 0.0));
        assert_eq!(affected_lights.len(), 1);

        // Position far away should not be affected
        let affected_lights = system.lights_affecting_position(Vector3::new(100.0, 0.0, 0.0));
        assert_eq!(affected_lights.len(), 0);
    }

    #[test]
    fn test_lights_by_type() {
        let mut system = LightSystem::new();

        let spotlight1 = SpotLight::new(
            Vector3::new(0.0, 1.0, 0.0),
            Vector3::new(0.0, -1.0, 0.0),
            Vector3::new(1.0, 1.0, 1.0),
            1.0,
        );
        let spotlight2 = SpotLight::new(
            Vector3::new(1.0, 1.0, 0.0),
            Vector3::new(0.0, -1.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0),
            0.5,
        );

        system.add_spotlight(spotlight1);
        system.add_spotlight(spotlight2);

        let spotlights = system.lights_by_type(LightType::Spotlight);
        assert_eq!(spotlights.len(), 2);
    }

    #[test]
    fn test_clear_lights() {
        let mut system = LightSystem::new();

        let spotlight = SpotLight::new(
            Vector3::new(0.0, 1.0, 0.0),
            Vector3::new(0.0, -1.0, 0.0),
            Vector3::new(1.0, 1.0, 1.0),
            1.0,
        );
        system.add_spotlight(spotlight);

        assert_eq!(system.light_count(), 1);

        system.clear();

        assert_eq!(system.light_count(), 0);
        assert!(system.is_dirty());
    }
}
