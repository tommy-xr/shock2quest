use engine::scene::Scene;

pub trait ToolScene {
    fn update(&mut self, delta_time: f32);
    fn render(&self) -> Scene;
}

pub mod video_player;

pub use video_player::VideoPlayerScene;