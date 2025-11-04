use engine::assets::asset_cache::AssetCache;
use engine::audio::AudioContext;
use engine::scene::Scene;

pub trait ToolScene {
    fn init(&mut self, audio_context: &mut AudioContext<(), String>) {
        // Default implementation does nothing
    }
    fn update(&mut self, delta_time: f32);
    fn render(&self, asset_cache: &mut AssetCache) -> Scene;
}

pub mod bin_ai_viewer;
pub mod bin_obj_viewer;
pub mod font_viewer;
pub mod glb_viewer;
pub mod video_player;

pub use bin_ai_viewer::BinAiViewerScene;
pub use bin_obj_viewer::BinObjViewerScene;
pub use font_viewer::FontViewerScene;
pub use glb_viewer::GlbViewerScene;
pub use video_player::VideoPlayerScene;
