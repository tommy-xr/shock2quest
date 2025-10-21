use engine::assets::asset_cache::AssetCache;
use engine::scene::Scene;

pub trait ToolScene {
    fn update(&mut self, delta_time: f32);
    fn render(&self, asset_cache: &mut AssetCache) -> Scene;
}

pub mod video_player;
pub mod bin_obj_viewer;
pub mod font_viewer;

pub use video_player::VideoPlayerScene;
pub use bin_obj_viewer::BinObjViewerScene;
pub use font_viewer::FontViewerScene;