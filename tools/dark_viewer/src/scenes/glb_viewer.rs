#![allow(unused_imports)]

use super::ToolScene;
use cgmath::{vec3, Deg, Matrix4, Quaternion, Rad};
use dark::importers::GLB_MODELS_IMPORTER;
use dark::motion::AnimationPlayer;
use engine::assets::asset_cache::AssetCache;
use engine::scene::{color_material, Scene, SceneObject};
use std::time::Duration;

pub struct GlbViewerScene {
    model_name: String,
    total_time: Duration,
    animation_player: AnimationPlayer,
}

impl GlbViewerScene {
    pub fn from_model(
        model_name: String,
        _asset_cache: &AssetCache,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        // We don't load the model here, we'll load it during render using the asset cache
        let animation_player = AnimationPlayer::empty();

        Ok(GlbViewerScene {
            model_name,
            total_time: Duration::ZERO,
            animation_player,
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
        let scene_objects = model.to_animated_scene_objects(&self.animation_player);

        Scene::from_objects(scene_objects)
    }
}
