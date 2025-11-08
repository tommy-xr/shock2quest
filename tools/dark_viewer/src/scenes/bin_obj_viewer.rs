#![allow(unused_imports)]

use super::ToolScene;
use cgmath::{Deg, Matrix4, Quaternion, Rad, vec3};
use dark::importers::MODELS_IMPORTER;
use dark::motion::AnimationPlayer;
use engine::assets::asset_cache::AssetCache;
use engine::scene::{Scene, SceneObject, color_material};
use std::time::Duration;

pub struct BinObjViewerScene {
    model_name: String,
    total_time: Duration,
    animation_player: AnimationPlayer,
}

impl BinObjViewerScene {
    pub fn from_model(
        model_name: String,
        _asset_cache: &AssetCache,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        // We don't load the model here, we'll load it during render using the asset cache
        let mut animation_player = AnimationPlayer::empty();
        // animation_player = AnimationPlayer::set_additional_joint_transform(
        //     &animation_player,
        //     2,
        //     Matrix4::from_translation(vec3(-0.5, 0.0, 0.0)) * Matrix4::from_angle_x(Deg(45.0)),
        // );

        Ok(BinObjViewerScene {
            model_name,
            total_time: Duration::ZERO,
            animation_player,
        })
    }
}

impl ToolScene for BinObjViewerScene {
    fn update(&mut self, delta_time: f32) {
        let elapsed = Duration::from_secs_f32(delta_time);
        self.total_time += elapsed;

        // Update animation transforms
        // self.animation_player = AnimationPlayer::set_additional_joint_transform(
        //     &self.animation_player,
        //     2,
        //     Matrix4::from_translation(vec3(-0.8, 0.0, 0.0)),
        // );
        // self.animation_player = AnimationPlayer::set_additional_joint_transform(
        //     &self.animation_player,
        //     1,
        //     Matrix4::from_angle_x(Deg(90.0 + 45.0 * self.total_time.as_secs_f32().sin())),
        // );
    }

    fn render(&self, asset_cache: &mut AssetCache) -> Scene {
        let turret = asset_cache.get(&MODELS_IMPORTER, &self.model_name);
        let turret_scene_objects = turret.to_animated_scene_objects(&self.animation_player);

        Scene::from_objects(turret_scene_objects)
    }
}
