use super::ToolScene;
use cgmath::{vec3, Matrix4};
use engine::scene::{basic_material, cube, Scene, SceneObject};
use engine::texture::{init_from_memory2, TextureOptions, TextureTrait};
use engine::texture_format::{PixelFormat, RawTextureData};
use std::rc::Rc;
use std::time::Duration;

#[cfg(feature = "ffmpeg")]
use engine_ffmpeg::VideoPlayer;

pub struct VideoPlayerScene {
    file_name: String,
    #[cfg(feature = "ffmpeg")]
    video_player: VideoPlayer,
    total_time: Duration,
}

impl VideoPlayerScene {
    pub fn from_file(file_name: String) -> Result<Self, Box<dyn std::error::Error>> {
        #[cfg(feature = "ffmpeg")]
        {
            let video_player = VideoPlayer::from_filename(&file_name)?;
            Ok(VideoPlayerScene {
                file_name,
                video_player,
                total_time: Duration::ZERO,
            })
        }
        #[cfg(not(feature = "ffmpeg"))]
        {
            Ok(VideoPlayerScene {
                file_name,
                total_time: Duration::ZERO,
            })
        }
    }
}

impl ToolScene for VideoPlayerScene {

    fn update(&mut self, delta_time: f32) {
        let elapsed = Duration::from_secs_f32(delta_time);
        self.total_time += elapsed;

        #[cfg(feature = "ffmpeg")]
        {
            self.video_player.advance_by_time(elapsed);
        }
    }

    fn render(&self, _asset_cache: &mut engine::assets::asset_cache::AssetCache) -> Scene {
        let texture: Rc<dyn TextureTrait> = {
            #[cfg(feature = "ffmpeg")]
            {
                let texture_data = self.video_player.get_current_frame();
                Rc::new(init_from_memory2(
                    texture_data,
                    &TextureOptions { wrap: false },
                ))
            }
            #[cfg(not(feature = "ffmpeg"))]
            {
                // Create a simple 1x1 white texture as fallback
                let white_pixel = vec![255u8, 255u8, 255u8, 255u8];
                let texture_data = RawTextureData {
                    width: 1,
                    height: 1,
                    bytes: white_pixel,
                    format: PixelFormat::RGBA,
                };
                Rc::new(init_from_memory2(
                    texture_data,
                    &TextureOptions { wrap: false },
                ))
            }
        };

        let cube_mat = basic_material::create(texture, 1.0, 0.0);
        let mut cube_obj = SceneObject::new(cube_mat, Box::new(cube::create()));
        cube_obj.set_transform(Matrix4::from_scale(3.0));

        Scene::from_objects(vec![cube_obj])
    }
}