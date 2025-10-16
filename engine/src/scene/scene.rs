pub use crate::scene::scene_object::SceneObject;
use crate::scene::light_system::LightSystem;

/// Legacy scene type - simple vector of scene objects
pub type LegacyScene = Vec<SceneObject>;

/// Enhanced scene with lighting support for multi-pass rendering
/// This struct provides the foundation for Doom 3-style multi-pass lighting
#[derive(Clone)]
pub struct Scene {
    /// Scene objects (geometry and materials)
    pub objects: Vec<SceneObject>,

    /// Lighting system for multi-pass rendering
    pub lights: LightSystem,
}

impl Scene {
    /// Create a new empty scene
    pub fn new() -> Self {
        Self {
            objects: Vec::new(),
            lights: LightSystem::new(),
        }
    }

    /// Create a scene from a vector of scene objects (for backwards compatibility)
    pub fn from_objects(objects: Vec<SceneObject>) -> Self {
        Self {
            objects,
            lights: LightSystem::new(),
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

    /// Get a mutable reference to the scene objects
    pub fn objects_mut(&mut self) -> &mut Vec<SceneObject> {
        &mut self.objects
    }

    /// Get a reference to the light system
    pub fn lights(&self) -> &LightSystem {
        &self.lights
    }

    /// Get a mutable reference to the light system
    pub fn lights_mut(&mut self) -> &mut LightSystem {
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

    /// Get the total number of lights
    pub fn light_count(&self) -> usize {
        self.lights.light_count()
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
