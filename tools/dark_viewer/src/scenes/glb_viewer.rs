#![allow(unused_imports)]

use super::{ToolScene, render_helpers::build_model_scene_with_debug_skeletons};
use cgmath::{Deg, Matrix4, Quaternion, Rad, vec3};
use dark::importers::GLB_MODELS_IMPORTER;
use dark::motion::AnimationPlayer;
use engine::assets::asset_cache::AssetCache;
use engine::scene::{Scene, SceneObject, color_material};
use std::time::Duration;

pub struct GlbViewerScene {
    model_name: String,
    scale: f32,
    total_time: Duration,
    animation_player: AnimationPlayer,
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
        let animation_player = AnimationPlayer::empty();

        Ok(GlbViewerScene {
            model_name,
            scale,
            total_time: Duration::ZERO,
            animation_player,
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
        let mut scene_objects = model.to_animated_scene_objects(&self.animation_player);

        // Apply scale transformation to all scene objects
        let scale_matrix = Matrix4::from_scale(self.scale);
        for scene_object in &mut scene_objects {
            scene_object.transform = scale_matrix * scene_object.transform;
        }

        build_model_scene_with_debug_skeletons(
            model.as_ref(),
            Some(&self.animation_player),
            scene_objects,
            self.debug_skeletons,
        )
    }
}
