mod audio_player;
mod video_player;

pub use crate::audio_player::AudioPlayer;
pub use crate::video_player::VideoPlayer;

pub fn init() -> Result<(), ffmpeg_next::Error> {
    ffmpeg_next::init()
}
