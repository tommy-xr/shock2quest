extern crate ffmpeg_the_third as ffmpeg;

use engine::texture_format::{PixelFormat, RawTextureData};
use ffmpeg::format::{Pixel, input};
use ffmpeg::media::Type;
use ffmpeg::software::scaling::{context::Context, flag::Flags};
use ffmpeg::util::frame::video::Video;
use std::time::Duration;

pub struct VideoPlayer {
    #[allow(dead_code)]
    width: u32,
    #[allow(dead_code)]
    height: u32,

    current_time: Duration,

    duration: Duration,
    #[allow(dead_code)]
    total_frame_count: i64,

    frames: Vec<RawTextureData>,
    #[allow(dead_code)]
    decoder: ffmpeg::decoder::Video,
    #[allow(dead_code)]
    scaler: ffmpeg::software::scaling::Context,
}

impl VideoPlayer {
    pub fn from_filename(filename: &str) -> Result<VideoPlayer, ffmpeg::Error> {
        let maybe_ictx = input(&filename);

        if maybe_ictx.is_err() {
            let err = maybe_ictx.err().unwrap();
            return Err(err);
        }

        let mut ictx = maybe_ictx.unwrap();
        let input = ictx
            .streams()
            .best(Type::Video)
            .ok_or(ffmpeg::Error::StreamNotFound)?;
        let video_stream_index = input.index();

        let context_decoder =
            ffmpeg::codec::context::Context::from_parameters(input.parameters()).unwrap();
        let mut decoder = context_decoder.decoder().video().unwrap();

        let mut scaler = Context::get(
            decoder.format(),
            decoder.width(),
            decoder.height(),
            Pixel::RGB24,
            decoder.width(),
            decoder.height(),
            Flags::BILINEAR,
        )?;

        let duration =
            Duration::from_secs_f64(input.duration() as f64 * f64::from(input.time_base()));
        let total_frame_count = input.frames();

        let mut frame_index = 0;

        let mut frames = Vec::new();

        let mut receive_and_process_decoded_frames =
            |decoder: &mut ffmpeg::decoder::Video| -> Result<(), ffmpeg::Error> {
                let mut decoded = Video::empty();
                while decoder.receive_frame(&mut decoded).is_ok() {
                    let mut rgb_frame = Video::empty();
                    scaler.run(&decoded, &mut rgb_frame).unwrap();
                    frames.push(RawTextureData {
                        bytes: rgb_frame.data(0).to_vec(),
                        width: rgb_frame.width(),
                        height: rgb_frame.height(),
                        format: PixelFormat::RGB,
                    });
                    frame_index += 1;
                }
                Ok(())
            };

        for (stream, packet) in ictx.packets().filter_map(Result::ok) {
            if stream.index() == video_stream_index {
                match decoder.send_packet(&packet) {
                    Ok(()) => receive_and_process_decoded_frames(&mut decoder).unwrap(),
                    Err(err) => println!("Video decoder send_packet error: {:?}", err),
                }
            }
        }
        decoder.send_eof()?;
        receive_and_process_decoded_frames(&mut decoder)?;

        Ok(VideoPlayer {
            width: decoder.width(),
            height: decoder.height(),
            decoder,
            scaler,
            current_time: Duration::from_secs_f64(0.0),
            frames,
            total_frame_count,
            duration,
        })
    }

    pub fn advance_by_time(&mut self, time: Duration) {
        self.current_time += time;
    }

    pub fn get_current_frame(&self) -> RawTextureData {
        let ratio = self.current_time.as_secs_f64() / self.duration.as_secs_f64();

        let current_frame = (ratio * self.frames.len() as f64) as usize;

        let idx = current_frame.max(0).min(self.frames.len() - 1);

        return self.frames[idx].clone();
    }
}
