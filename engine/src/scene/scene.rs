pub use crate::scene::scene_object::SceneObject;
use crate::scene::{RenderLayer, light::LightArray};

/// Legacy scene type - simple vector of scene objects
pub type LegacyScene = Vec<SceneObject>;

/// Enhanced scene with lighting support for single-pass rendering
/// This struct provides the foundation for efficient single-pass lighting with up to 6 spotlights
#[derive(Clone)]
pub struct Scene {
    /// Scene objects (geometry and materials)
    pub objects: Vec<SceneObject>,

    /// Light array for single-pass rendering (up to 6 spotlights)
    pub lights: LightArray,
}

impl Scene {
    /// Create a new empty scene
    pub fn new() -> Self {
        Self {
            objects: Vec::new(),
            lights: LightArray::new(),
        }
    }

    /// Create a scene from a vector of scene objects (for backwards compatibility)
    pub fn from_objects(objects: Vec<SceneObject>) -> Self {
        Self {
            objects,
            lights: LightArray::new(),
        }
    }

    /// Add a scene object to the scene
    pub fn add_object(&mut self, object: SceneObject) {
        self.objects.push(object);
    }

    /// Get a reference to the scene objects
    pub fn objects(&self) -> &[SceneObject] {
        &self.objects
    }

    /// Objects in one explicit composition layer, preserving their authored
    /// order within that layer regardless of how a host concatenated sources.
    pub fn objects_in_layer(&self, layer: RenderLayer) -> impl Iterator<Item = &SceneObject> {
        self.objects
            .iter()
            .filter(move |object| object.render_layer() == layer)
    }

    /// Get a mutable reference to the scene objects
    pub fn objects_mut(&mut self) -> &mut Vec<SceneObject> {
        &mut self.objects
    }

    /// Get a reference to the light array
    pub fn lights(&self) -> &LightArray {
        &self.lights
    }

    /// Get a mutable reference to the light array
    pub fn lights_mut(&mut self) -> &mut LightArray {
        &mut self.lights
    }

    /// Clear all objects and lights from the scene
    pub fn clear(&mut self) {
        self.objects.clear();
        self.lights.clear();
    }

    /// Get the total number of scene objects
    pub fn object_count(&self) -> usize {
        self.objects.len()
    }

    /// Get the total number of active lights
    pub fn light_count(&self) -> usize {
        self.lights.active_count()
    }
}

impl Default for Scene {
    fn default() -> Self {
        Self::new()
    }
}

// Provide backwards compatibility by allowing conversion to/from Vec<SceneObject>
impl From<Vec<SceneObject>> for Scene {
    fn from(objects: Vec<SceneObject>) -> Self {
        Self::from_objects(objects)
    }
}

impl From<Scene> for Vec<SceneObject> {
    fn from(scene: Scene) -> Self {
        scene.objects
    }
}

// Allow Scene to be used as a Vec<SceneObject> in many contexts
impl std::ops::Deref for Scene {
    type Target = Vec<SceneObject>;

    fn deref(&self) -> &Self::Target {
        &self.objects
    }
}

impl std::ops::DerefMut for Scene {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.objects
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::{color_material, cube};
    use cgmath::{Matrix4, Vector3};

    fn object(layer: RenderLayer, marker: f32) -> SceneObject {
        let mut object = SceneObject::new(
            color_material::create(Vector3::new(1.0, 1.0, 1.0)),
            Box::new(cube::create()),
        );
        object.set_render_layer(layer);
        object.set_transform(Matrix4::from_translation(Vector3::new(marker, 0.0, 0.0)));
        object
    }

    fn render_markers(scene: &Scene) -> Vec<f32> {
        RenderLayer::ORDERED
            .into_iter()
            .flat_map(|layer| scene.objects_in_layer(layer))
            .map(|object| object.get_transform().w.x)
            .collect()
    }

    #[test]
    fn explicit_layers_make_host_concatenation_order_irrelevant() {
        let flat = Scene::from_objects(vec![
            object(RenderLayer::World, 1.0),
            object(RenderLayer::SceneOverlay, 2.0),
            object(RenderLayer::SceneUi, 3.0),
            object(RenderLayer::SystemOverlay, 4.0),
        ]);
        let quest = Scene::from_objects(vec![
            object(RenderLayer::SceneUi, 3.0),
            object(RenderLayer::SystemOverlay, 4.0),
            object(RenderLayer::World, 1.0),
            object(RenderLayer::SceneOverlay, 2.0),
        ]);

        assert_eq!(render_markers(&flat), vec![1.0, 2.0, 3.0, 4.0]);
        assert_eq!(render_markers(&quest), vec![1.0, 2.0, 3.0, 4.0]);
    }
}
