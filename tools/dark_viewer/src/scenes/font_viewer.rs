use super::ToolScene;
use cgmath::vec2;
use dark::font::Font;
use engine::assets::asset_cache::AssetCache;
use engine::materials::ScreenSpaceMaterial;
use engine::scene::{Scene, SceneObject};
use std::fs::File;
use std::io::BufReader;
use std::time::Duration;

pub struct FontViewerScene {
    font_file_path: String,
    font: Option<Font>,
    text_string: String,
    font_size: f32,
    position: cgmath::Vector2<f32>,
    total_time: Duration,
}

impl FontViewerScene {
    pub fn from_file(font_file_path: String, resource_path_fn: fn(&str) -> String) -> Result<Self, Box<dyn std::error::Error>> {
        // Load font
        let font_file = File::open(resource_path_fn(&font_file_path))?;
        let mut font_reader = BufReader::new(font_file);
        let font = Font::read(&mut font_reader);

        Ok(FontViewerScene {
            font_file_path,
            font: Some(font),
            text_string: "0123456789 Ramsey Recruitment".to_string(),
            font_size: 30.0,
            position: vec2(200.0, 200.0),
            total_time: Duration::ZERO,
        })
    }

    pub fn set_text(&mut self, text: String) {
        self.text_string = text;
    }

    pub fn set_font_size(&mut self, size: f32) {
        self.font_size = size;
    }

    pub fn set_position(&mut self, position: cgmath::Vector2<f32>) {
        self.position = position;
    }
}

impl ToolScene for FontViewerScene {
    fn update(&mut self, delta_time: f32) {
        let elapsed = Duration::from_secs_f32(delta_time);
        self.total_time += elapsed;
    }

    fn render(&self, _asset_cache: &mut AssetCache) -> Scene {
        let mut scene = vec![];

        if let Some(font) = &self.font {
            let text_material = ScreenSpaceMaterial::create(
                font.texture.clone(),
                cgmath::vec4(1.0, 1.0, 1.0, 1.0),
            );

            let text_mesh = font.get_mesh(&self.text_string, self.position, self.font_size);
            let text_obj = SceneObject::new(text_material, Box::new(text_mesh));
            scene.push(text_obj);
        }

        Scene::from_objects(scene)
    }
}