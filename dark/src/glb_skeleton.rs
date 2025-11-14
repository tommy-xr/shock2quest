// glb_skeleton.rs
// Standalone GLB skeleton system independent of SS2-specific abstractions

use cgmath::{Matrix4, SquareMatrix};
use std::collections::HashMap;

/// A single node in a GLB skeleton hierarchy
#[derive(Debug, Clone)]
pub struct GlbNode {
    pub index: usize,
    pub name: Option<String>,
    pub local_transform: Matrix4<f32>, // T*R*S from glTF
    pub parent_index: Option<usize>,
}

/// GLB-specific skeleton with direct node index mapping
#[derive(Debug, Clone)]
pub struct GlbSkeleton {
    pub(crate) nodes: Vec<GlbNode>,
    pub(crate) inverse_bind_matrices: Vec<Matrix4<f32>>, // Per-joint inverse bind matrices
    pub(crate) joint_count: usize,
    // Mapping from node index to position in joints array
    pub(crate) node_to_joint_index: HashMap<usize, usize>,
    // Mapping from joint index to node index
    pub(crate) joint_index_to_node: Vec<usize>,
}

/// Current animation/pose state for a GLB skeleton
#[derive(Debug, Clone)]
pub struct GlbAnimationState {
    skeleton: GlbSkeleton,
    current_node_transforms: Vec<Matrix4<f32>>, // Per-node local transforms (modified by animation/poses)
    global_transforms: Vec<Matrix4<f32>>,       // Computed world transforms for all nodes
    skinning_matrices: [Matrix4<f32>; 40],      // Final matrices for GPU (limited to 40 for now)
    dirty: bool,                                // Whether transforms need recomputation
}

impl GlbSkeleton {
    /// Create a new GLB skeleton from joint nodes and inverse bind matrices
    pub fn new(
        joint_nodes: Vec<gltf::Node>,
        inverse_bind_matrices: Vec<Matrix4<f32>>,
        all_nodes: Vec<gltf::Node>, // All nodes in the scene to build hierarchy
    ) -> Self {
        let joint_count = joint_nodes.len();

        // Create mapping from node index to joint index
        let mut node_to_joint_index = HashMap::new();
        let mut joint_index_to_node = Vec::new();

        for (joint_index, joint_node) in joint_nodes.iter().enumerate() {
            let node_index = joint_node.index();
            node_to_joint_index.insert(node_index, joint_index);
            joint_index_to_node.push(node_index);
        }

        // Build node hierarchy including all nodes (not just joints)
        let mut nodes = Vec::new();
        for node in &all_nodes {
            let node_index = node.index();
            let parent_index = Self::find_parent_index(&node, &all_nodes);

            let glb_node = GlbNode {
                index: node_index,
                name: node.name().map(|s| s.to_string()),
                local_transform: Matrix4::from(node.transform().matrix()),
                parent_index,
            };
            nodes.push(glb_node);
        }

        // Sort nodes by index for predictable ordering
        nodes.sort_by_key(|n| n.index);

        Self {
            nodes,
            inverse_bind_matrices,
            joint_count,
            node_to_joint_index,
            joint_index_to_node,
        }
    }

    fn find_parent_index(target_node: &gltf::Node, all_nodes: &[gltf::Node]) -> Option<usize> {
        for node in all_nodes {
            for child in node.children() {
                if child.index() == target_node.index() {
                    return Some(node.index());
                }
            }
        }
        None
    }

    pub fn joint_count(&self) -> usize {
        self.joint_count
    }

    pub fn nodes(&self) -> &[GlbNode] {
        &self.nodes
    }

    pub fn is_joint(&self, node_index: usize) -> bool {
        self.node_to_joint_index.contains_key(&node_index)
    }

    pub fn joint_index_for_node(&self, node_index: usize) -> Option<usize> {
        self.node_to_joint_index.get(&node_index).copied()
    }

    pub fn node_index_for_joint(&self, joint_index: usize) -> Option<usize> {
        self.joint_index_to_node.get(joint_index).copied()
    }

    pub fn inverse_bind_matrix(&self, joint_index: usize) -> Matrix4<f32> {
        self.inverse_bind_matrices
            .get(joint_index)
            .copied()
            .unwrap_or_else(Matrix4::identity)
    }

    pub fn get_node(&self, node_index: usize) -> Option<&GlbNode> {
        self.nodes.iter().find(|n| n.index == node_index)
    }
}

impl GlbAnimationState {
    /// Create a new animation state from a GLB skeleton
    pub fn new(skeleton: GlbSkeleton) -> Self {
        let node_count = skeleton.nodes.len();

        // Initialize with rest pose transforms
        let current_node_transforms: Vec<Matrix4<f32>> = skeleton
            .nodes
            .iter()
            .map(|node| node.local_transform)
            .collect();

        let global_transforms = vec![Matrix4::identity(); node_count];
        let skinning_matrices = [Matrix4::identity(); 40];

        let mut state = Self {
            skeleton,
            current_node_transforms,
            global_transforms,
            skinning_matrices,
            dirty: true,
        };

        state.update_transforms();
        state
    }

    /// Set a custom transform for a specific node (for manual posing)
    pub fn set_node_transform(&mut self, node_index: usize, transform: Matrix4<f32>) {
        // Find the position in our nodes array for this node index
        if let Some(pos) = self
            .skeleton
            .nodes
            .iter()
            .position(|n| n.index == node_index)
        {
            self.current_node_transforms[pos] = transform;
            self.dirty = true;
        }
    }

    /// Get the current local transform for a node
    pub fn get_node_transform(&self, node_index: usize) -> Option<Matrix4<f32>> {
        let pos = self
            .skeleton
            .nodes
            .iter()
            .position(|n| n.index == node_index)?;
        self.current_node_transforms.get(pos).copied()
    }

    /// Update all global transforms and skinning matrices (call after setting transforms)
    pub fn update_transforms(&mut self) {
        if !self.dirty {
            return;
        }

        // Compute global transforms for all nodes
        let node_count = self.skeleton.nodes.len();
        self.global_transforms = vec![Matrix4::identity(); node_count];

        // Process nodes in dependency order (parents before children)
        // Simple approach: iterate until no more changes
        let mut changed = true;
        while changed {
            changed = false;
            for (pos, node) in self.skeleton.nodes.iter().enumerate() {
                let parent_global = if let Some(parent_index) = node.parent_index {
                    if let Some(parent_pos) = self
                        .skeleton
                        .nodes
                        .iter()
                        .position(|n| n.index == parent_index)
                    {
                        self.global_transforms[parent_pos]
                    } else {
                        Matrix4::identity()
                    }
                } else {
                    Matrix4::identity()
                };

                let new_global = parent_global * self.current_node_transforms[pos];

                if (new_global - self.global_transforms[pos]).magnitude2() > 1e-10 {
                    self.global_transforms[pos] = new_global;
                    changed = true;
                }
            }
        }

        // Compute skinning matrices for joints only
        self.skinning_matrices = [Matrix4::identity(); 40];

        for joint_index in 0..self.skeleton.joint_count.min(40) {
            if let Some(node_index) = self.skeleton.node_index_for_joint(joint_index) {
                if let Some(node_pos) = self
                    .skeleton
                    .nodes
                    .iter()
                    .position(|n| n.index == node_index)
                {
                    let global_transform = self.global_transforms[node_pos];
                    let inverse_bind = self.skeleton.inverse_bind_matrix(joint_index);
                    self.skinning_matrices[joint_index] = global_transform * inverse_bind;
                }
            }
        }

        self.dirty = false;
    }

    /// Get the final skinning matrices for GPU rendering
    pub fn get_skinning_matrices(&mut self) -> [Matrix4<f32>; 40] {
        self.update_transforms();
        self.skinning_matrices
    }

    /// Get the skeleton reference
    pub fn skeleton(&self) -> &GlbSkeleton {
        &self.skeleton
    }

    /// Get global transform for a specific node
    pub fn get_global_transform(&mut self, node_index: usize) -> Option<Matrix4<f32>> {
        self.update_transforms();
        let pos = self
            .skeleton
            .nodes
            .iter()
            .position(|n| n.index == node_index)?;
        self.global_transforms.get(pos).copied()
    }

    /// Set a transform for a specific joint (uses joint index, not node index)
    pub fn set_joint_transform(&mut self, joint_index: usize, transform: Matrix4<f32>) {
        // Convert joint index to node index
        if let Some(node_index) = self.skeleton.node_index_for_joint(joint_index) {
            self.set_node_transform(node_index, transform);
        }
    }

    /// Get the current local transform for a joint (uses joint index)
    pub fn get_joint_transform(&self, joint_index: usize) -> Option<Matrix4<f32>> {
        let node_index = self.skeleton.node_index_for_joint(joint_index)?;
        self.get_node_transform(node_index)
    }
}

// Helper trait extension for Matrix4 to calculate magnitude
trait Matrix4Ext {
    fn magnitude2(&self) -> f32;
}

impl Matrix4Ext for Matrix4<f32> {
    fn magnitude2(&self) -> f32 {
        // Simple magnitude calculation for matrix difference detection
        self.x.x * self.x.x
            + self.x.y * self.x.y
            + self.x.z * self.x.z
            + self.x.w * self.x.w
            + self.y.x * self.y.x
            + self.y.y * self.y.y
            + self.y.z * self.y.z
            + self.y.w * self.y.w
            + self.z.x * self.z.x
            + self.z.y * self.z.y
            + self.z.z * self.z.z
            + self.z.w * self.z.w
            + self.w.x * self.w.x
            + self.w.y * self.w.y
            + self.w.z * self.w.z
            + self.w.w * self.w.w
    }
}
