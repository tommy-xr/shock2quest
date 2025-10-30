use cgmath::{Matrix4, SquareMatrix};

use super::SceneObject;

/// Trait for objects that can produce SceneObjects for rendering
/// This allows for hierarchical scene composition while maintaining
/// compatibility with the existing rendering pipeline
pub trait Renderable {
    /// Convert this renderable into a flat list of SceneObjects ready for rendering
    fn render_objects(&self) -> Vec<SceneObject>;
}

/// Existing SceneObject implements Renderable for backward compatibility
impl Renderable for SceneObject {
    fn render_objects(&self) -> Vec<SceneObject> {
        vec![self.clone()]
    }
}

/// Container that applies a transform to a collection of child renderables
/// This enables hierarchical scene composition - perfect for 2D UI elements
/// that need to be positioned and rotated as a group in world space
pub struct TransformSceneObject {
    pub transform: Matrix4<f32>,
    pub children: Vec<Box<dyn Renderable>>,
}

impl TransformSceneObject {
    /// Create a new TransformSceneObject with identity transform
    pub fn new() -> Self {
        Self {
            transform: Matrix4::identity(),
            children: Vec::new(),
        }
    }

    /// Create a new TransformSceneObject with the specified transform
    pub fn with_transform(transform: Matrix4<f32>) -> Self {
        Self {
            transform,
            children: Vec::new(),
        }
    }

    /// Add a child renderable object
    pub fn add_child(&mut self, child: Box<dyn Renderable>) {
        self.children.push(child);
    }

    /// Add a SceneObject as a child (convenience method)
    pub fn add_scene_object(&mut self, scene_object: SceneObject) {
        self.children.push(Box::new(scene_object));
    }

    /// Set the transform for this group
    pub fn set_transform(&mut self, transform: Matrix4<f32>) {
        self.transform = transform;
    }

    /// Get the current transform
    pub fn get_transform(&self) -> Matrix4<f32> {
        self.transform
    }

    /// Clear all children
    pub fn clear(&mut self) {
        self.children.clear();
    }

    /// Get the number of direct children
    pub fn child_count(&self) -> usize {
        self.children.len()
    }
}

impl Default for TransformSceneObject {
    fn default() -> Self {
        Self::new()
    }
}

impl Renderable for TransformSceneObject {
    fn render_objects(&self) -> Vec<SceneObject> {
        let mut objects = Vec::new();

        for child in &self.children {
            for mut obj in child.render_objects() {
                // Apply this transform to the child's transform
                // Note: We need to account for both transform and local_transform
                let child_final_transform = obj.get_transform(); // This is just obj.transform
                let new_transform = self.transform * child_final_transform;
                obj.set_transform(new_transform);
                // local_transform remains unchanged - this preserves any existing local transforms
                objects.push(obj);
            }
        }

        objects
    }
}

/// Helper functions for working with renderables

/// Flatten a collection of renderables into a Vec<SceneObject>
/// This provides the bridge between the new trait system and existing rendering code
pub fn flatten_renderables(renderables: Vec<Box<dyn Renderable>>) -> Vec<SceneObject> {
    renderables
        .into_iter()
        .flat_map(|renderable| renderable.render_objects())
        .collect()
}

/// Convert a Vec<SceneObject> to Vec<Box<dyn Renderable>> for unified handling
pub fn scene_objects_to_renderables(objects: Vec<SceneObject>) -> Vec<Box<dyn Renderable>> {
    objects
        .into_iter()
        .map(|obj| Box::new(obj) as Box<dyn Renderable>)
        .collect()
}

/// Convenience function to create a transform group from SceneObjects
pub fn create_transform_group(
    transform: Matrix4<f32>,
    objects: Vec<SceneObject>,
) -> TransformSceneObject {
    let mut group = TransformSceneObject::with_transform(transform);
    for obj in objects {
        group.add_scene_object(obj);
    }
    group
}