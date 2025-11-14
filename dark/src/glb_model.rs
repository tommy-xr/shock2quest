// glb_model.rs
// Standalone GLB model that doesn't depend on SS2 Model abstractions

use cgmath::Matrix4;
use collision::Aabb3;
use engine::scene::SceneObject;

use crate::glb_skeleton::{GlbAnimationState, GlbSkeleton};

/// Standalone GLB model with direct skeleton control
#[derive(Clone)]
pub struct GlbModel {
    scene_objects: Vec<SceneObject>,
    bounding_box: Aabb3<f32>,
    animation_state: GlbAnimationState,
}

impl GlbModel {
    /// Create a new GLB model with skeleton
    pub fn new(
        scene_objects: Vec<SceneObject>,
        bounding_box: Aabb3<f32>,
        skeleton: GlbSkeleton,
    ) -> Self {
        let animation_state = GlbAnimationState::new(skeleton);

        Self {
            scene_objects,
            bounding_box,
            animation_state,
        }
    }

    /// Create a static GLB model without skeleton
    pub fn new_static(scene_objects: Vec<SceneObject>, bounding_box: Aabb3<f32>) -> Self {
        // Create empty skeleton for static model
        let empty_skeleton = GlbSkeleton::empty();
        let animation_state = GlbAnimationState::new(empty_skeleton);

        Self {
            scene_objects,
            bounding_box,
            animation_state,
        }
    }

    /// Set a transform for a specific node (for manual posing)
    pub fn set_node_transform(&mut self, node_index: usize, transform: Matrix4<f32>) {
        self.animation_state
            .set_node_transform(node_index, transform);
    }

    /// Get the current transform for a node
    pub fn get_node_transform(&self, node_index: usize) -> Option<Matrix4<f32>> {
        self.animation_state.get_node_transform(node_index)
    }

    /// Get the final skinning matrices for GPU rendering
    pub fn get_skinning_matrices(&mut self) -> [Matrix4<f32>; 40] {
        self.animation_state.get_skinning_matrices()
    }

    /// Get scene objects with current skinning applied
    pub fn to_scene_objects_with_skinning(&mut self) -> Vec<SceneObject> {
        let skinning_data = self.get_skinning_matrices();

        self.scene_objects
            .iter()
            .map(|obj| {
                let mut new_obj = obj.clone();
                new_obj.set_skinning_data(skinning_data);
                new_obj
            })
            .collect()
    }

    /// Get static scene objects (without skinning)
    pub fn to_scene_objects(&self) -> &Vec<SceneObject> {
        &self.scene_objects
    }

    /// Clone scene objects
    pub fn clone_scene_objects(&self) -> Vec<SceneObject> {
        self.scene_objects.clone()
    }

    /// Get the skeleton reference
    pub fn skeleton(&self) -> &GlbSkeleton {
        self.animation_state.skeleton()
    }

    /// Get bounding box
    pub fn bounding_box(&self) -> Aabb3<f32> {
        self.bounding_box
    }

    /// Check if this model has a skeleton
    pub fn has_skeleton(&self) -> bool {
        self.skeleton().joint_count() > 0
    }

    /// Apply hand pose transforms (convenience method for VR gloves)
    pub fn apply_hand_pose(
        &mut self,
        joint_transforms: &std::collections::HashMap<u32, Matrix4<f32>>,
    ) {
        for (joint_id, transform) in joint_transforms {
            // Convert joint ID to node index if needed
            let node_index = *joint_id as usize; // Assuming direct mapping for now
            self.set_node_transform(node_index, *transform);
        }
    }

    /// Get global transform for a node (useful for debugging)
    pub fn get_global_transform(&mut self, node_index: usize) -> Option<Matrix4<f32>> {
        self.animation_state.get_global_transform(node_index)
    }

    /// Set a transform for a specific joint (uses joint index, not node index)
    pub fn set_joint_transform(&mut self, joint_index: usize, transform: Matrix4<f32>) {
        self.animation_state
            .set_joint_transform(joint_index, transform);
    }

    /// Get the current local transform for a joint (uses joint index)
    pub fn get_joint_transform(&self, joint_index: usize) -> Option<Matrix4<f32>> {
        self.animation_state.get_joint_transform(joint_index)
    }
}

// Add empty skeleton support to GlbSkeleton
impl GlbSkeleton {
    /// Create an empty skeleton for static models
    pub fn empty() -> Self {
        Self {
            nodes: Vec::new(),
            inverse_bind_matrices: Vec::new(),
            joint_count: 0,
            node_to_joint_index: std::collections::HashMap::new(),
            joint_index_to_node: Vec::new(),
        }
    }
}
