#![allow(unused_imports)]

use super::ToolScene;
use cgmath::{Deg, Matrix4, Quaternion, Rad, SquareMatrix, vec3};
use dark::importers::GLB_MODELS_IMPORTER;
use engine::assets::asset_cache::AssetCache;
use engine::scene::{Scene, SceneObject, color_material};
use std::rc::Rc;
use std::time::Duration;

pub struct GlbViewerScene {
    model_name: String,
    scale: f32,
    total_time: Duration,
    debug_skeletons: bool,
}

impl GlbViewerScene {
    pub fn from_model(
        model_name: String,
        scale: f32,
        _asset_cache: &AssetCache,
        debug_skeletons: bool,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        // We don't load the model here, we'll load it during render using the asset cache
        Ok(GlbViewerScene {
            model_name,
            scale,
            total_time: Duration::ZERO,
            debug_skeletons,
        })
    }
}

impl ToolScene for GlbViewerScene {
    fn update(&mut self, delta_time: f32) {
        let elapsed = Duration::from_secs_f32(delta_time);
        self.total_time += elapsed;
    }

    fn render(&self, asset_cache: &mut AssetCache) -> Scene {
        let model = asset_cache.get(&GLB_MODELS_IMPORTER, &self.model_name);
        let mut scene_objects = model.clone_scene_objects();

        // Apply scale transformation to all scene objects
        let scale_matrix = Matrix4::from_scale(self.scale);
        for scene_object in &mut scene_objects {
            scene_object.set_transform(scale_matrix * scene_object.get_transform());
        }

        // Add debug skeleton visualization if requested
        if self.debug_skeletons && model.has_skeleton() {
            // Create debug cubes for each bone
            let skeleton = model.skeleton();
            let mut animation_state = dark::glb_skeleton::GlbAnimationState::new(skeleton.clone());
            let bone_transforms = animation_state.get_skinning_matrices();

            for (bone_index, bone_transform) in bone_transforms.iter().enumerate() {
                // Skip identity transforms (unused bones)
                if bone_transform != &Matrix4::identity() {
                    let bone_position = bone_transform.w.truncate();

                    // Create cube at bone position
                    let cube_color = match bone_index {
                        0 => vec3(1.0, 0.0, 0.0), // Red for first bone
                        1 => vec3(0.0, 1.0, 0.0), // Green for second bone
                        2 => vec3(0.0, 0.0, 1.0), // Blue for third bone
                        _ => vec3(0.7, 0.7, 0.7), // Gray for others
                    };
                    let cube_material = color_material::create(cube_color);
                    let mut bone_cube =
                        SceneObject::new(cube_material, Box::new(engine::scene::cube::create()));

                    // Scale cube small and position it at the bone location
                    let cube_size = 0.02; // Small cube
                    let bone_cube_transform = scale_matrix
                        * Matrix4::from_translation(bone_position)
                        * Matrix4::from_scale(cube_size);

                    bone_cube.set_transform(bone_cube_transform);
                    scene_objects.push(bone_cube);
                }
            }
        }

        Scene::from_objects(scene_objects)
    }
}
