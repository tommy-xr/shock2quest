extern crate ffmpeg_next as ffmpeg;

use engine::texture_format::{PixelFormat, RawTextureData};
use ffmpeg::format::{Pixel, input};
use ffmpeg::media::Type;
use ffmpeg::software::scaling::{context::Context, flag::Flags};
use ffmpeg::util::frame::video::Video;
use std::time::Duration;

/// Incremental video decoder used by cutscene scenes.
///
/// Frames are decoded only as playback reaches them. A retail ending can be
/// several minutes of 1080p video, so retaining every RGB frame would consume
/// tens of gigabytes and terminate the runtime before the first frame appeared.
pub struct VideoPlayer {
    current_time: Duration,
    duration: Duration,
    frames_per_second: f64,
    current_frame_index: usize,
    current_frame: RawTextureData,
    input: ffmpeg::format::context::Input,
    video_stream_index: usize,
    decoder: ffmpeg::decoder::Video,
    scaler: ffmpeg::software::scaling::Context,
    sent_eof: bool,
    frames_exhausted: bool,
}

impl VideoPlayer {
    pub fn from_filename(filename: &str) -> Result<VideoPlayer, ffmpeg::Error> {
        let input_context = input(filename)?;
        let input_stream = input_context
            .streams()
            .best(Type::Video)
            .ok_or(ffmpeg::Error::StreamNotFound)?;
        let video_stream_index = input_stream.index();

        let context_decoder =
            ffmpeg::codec::context::Context::from_parameters(input_stream.parameters())?;
        let decoder = context_decoder.decoder().video()?;
        let scaler = Context::get(
            decoder.format(),
            decoder.width(),
            decoder.height(),
            Pixel::RGB24,
            decoder.width(),
            decoder.height(),
            Flags::BILINEAR,
        )?;

        let stream_duration_seconds =
            input_stream.duration() as f64 * f64::from(input_stream.time_base());
        // Some AVI/OGV streams omit their own duration even though the
        // container reports one. FFmpeg's container duration is in
        // microseconds (AV_TIME_BASE).
        let container_duration_seconds = input_context.duration() as f64 / 1_000_000.0;
        let duration_seconds =
            if stream_duration_seconds.is_finite() && stream_duration_seconds > 0.0 {
                stream_duration_seconds
            } else {
                container_duration_seconds.max(0.0)
            };
        let duration = Duration::from_secs_f64(duration_seconds.max(0.0));
        let reported_frame_rate = f64::from(input_stream.avg_frame_rate());
        let frames_per_second = if reported_frame_rate.is_finite() && reported_frame_rate > 0.0 {
            reported_frame_rate
        } else {
            30.0
        };

        // End the stream borrow before moving the input context into the player.
        let _ = input_stream;

        let mut player = VideoPlayer {
            current_time: Duration::ZERO,
            duration,
            frames_per_second,
            current_frame_index: 0,
            current_frame: RawTextureData {
                bytes: Vec::new(),
                width: 0,
                height: 0,
                format: PixelFormat::RGB,
            },
            input: input_context,
            video_stream_index,
            decoder,
            scaler,
            sent_eof: false,
            frames_exhausted: false,
        };
        if !player.decode_next_frame()? {
            return Err(ffmpeg::Error::InvalidData);
        }
        Ok(player)
    }

    /// Decode one more video frame, consuming packets from the demuxer only as
    /// needed. Audio packets are skipped because the audio player owns its own
    /// input context.
    fn decode_next_frame(&mut self) -> Result<bool, ffmpeg::Error> {
        loop {
            let mut decoded = Video::empty();
            if self.decoder.receive_frame(&mut decoded).is_ok() {
                let mut rgb_frame = Video::empty();
                self.scaler.run(&decoded, &mut rgb_frame)?;
                self.current_frame = RawTextureData {
                    bytes: rgb_frame.data(0).to_vec(),
                    width: rgb_frame.width(),
                    height: rgb_frame.height(),
                    format: PixelFormat::RGB,
                };
                return Ok(true);
            }

            if self.sent_eof {
                return Ok(false);
            }

            let next_packet = self.input.packets().find_map(|(stream, packet)| {
                (stream.index() == self.video_stream_index).then_some(packet)
            });
            match next_packet {
                Some(packet) => self.decoder.send_packet(&packet)?,
                None => {
                    self.decoder.send_eof()?;
                    self.sent_eof = true;
                }
            }
        }
    }

    pub fn advance_by_time(&mut self, time: Duration) {
        self.current_time += time;
        // A stream that reported no duration must not be clamped to zero, or it
        // would never decode a second frame and so never reach EOF - the only
        // completion signal such a stream has.
        if !self.duration.is_zero() {
            self.current_time = self.current_time.min(self.duration);
        }
        let target_frame_index =
            (self.current_time.as_secs_f64() * self.frames_per_second).floor() as usize;

        while self.current_frame_index < target_frame_index {
            match self.decode_next_frame() {
                Ok(true) => self.current_frame_index += 1,
                Ok(false) => {
                    self.frames_exhausted = true;
                    break;
                }
                Err(error) => {
                    eprintln!("cutscene video decode failed: {error}");
                    self.frames_exhausted = true;
                    break;
                }
            }
        }
    }

    pub fn get_current_frame(&self) -> RawTextureData {
        self.current_frame.clone()
    }

    /// Playback length of the video stream. Zero when neither the stream nor
    /// the container reported one.
    pub fn duration(&self) -> Duration {
        self.duration
    }

    /// True once playback has reached the end: either the timeline caught up
    /// with the reported duration, or the decoder drained (which is the only
    /// signal available for a stream that reports no duration).
    pub fn is_finished(&self) -> bool {
        self.frames_exhausted || (!self.duration.is_zero() && self.current_time >= self.duration)
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    fn retained_frame_bytes(player: &VideoPlayer) -> usize {
        player.current_frame.bytes.len()
    }

    #[test]
    fn avi_and_ogv_advance_without_retaining_prior_frames() {
        crate::init().unwrap();
        let testdata = Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata");

        for fixture_name in ["streaming-test.avi", "streaming-test.ogv"] {
            let fixture = testdata.join(fixture_name);
            let mut player = VideoPlayer::from_filename(fixture.to_str().unwrap()).unwrap();
            let first_frame = player.get_current_frame();
            let one_rgb_frame = 64 * 64 * 3;

            assert!(
                retained_frame_bytes(&player) <= one_rgb_frame,
                "opening {fixture_name} retained {} decoded bytes",
                retained_frame_bytes(&player)
            );

            player.advance_by_time(Duration::from_secs(1));

            assert!(
                player.current_frame_index >= 29,
                "{fixture_name} did not advance at its 30fps timeline"
            );
            assert_ne!(
                player.get_current_frame().bytes,
                first_frame.bytes,
                "{fixture_name} should decode a later frame after advancing"
            );
            assert!(
                retained_frame_bytes(&player) <= one_rgb_frame,
                "advancing {fixture_name} retained prior decoded frames"
            );

            player.advance_by_time(Duration::from_secs(10));
            let final_frame_index = player.current_frame_index;
            player.advance_by_time(Duration::from_secs(10));
            assert_eq!(
                player.current_frame_index, final_frame_index,
                "{fixture_name} should stop decoding cleanly at EOF"
            );
            assert!(
                retained_frame_bytes(&player) <= one_rgb_frame,
                "reaching EOF in {fixture_name} retained prior decoded frames"
            );
        }
    }

    #[test]
    fn playback_is_finished_only_once_the_video_ends() {
        crate::init().unwrap();
        let testdata = Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata");

        for fixture_name in ["streaming-test.avi", "streaming-test.ogv"] {
            let fixture = testdata.join(fixture_name);
            let mut player = VideoPlayer::from_filename(fixture.to_str().unwrap()).unwrap();

            assert!(
                !player.duration().is_zero(),
                "{fixture_name} should report a duration"
            );
            assert!(
                !player.is_finished(),
                "{fixture_name} should not be finished before it plays"
            );

            player.advance_by_time(player.duration() / 2);
            assert!(
                !player.is_finished(),
                "{fixture_name} should not be finished halfway through"
            );

            player.advance_by_time(player.duration());
            assert!(
                player.is_finished(),
                "{fixture_name} should be finished after its duration elapses"
            );
        }
    }

    /// A container that reports no duration has only EOF to signal completion,
    /// so playback must still advance frame by frame and finish.
    #[test]
    fn a_duration_less_stream_still_advances_to_completion() {
        crate::init().unwrap();
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("testdata")
            .join("streaming-test.avi");

        let mut player = VideoPlayer::from_filename(fixture.to_str().unwrap()).unwrap();
        player.duration = Duration::ZERO;
        let first_frame = player.get_current_frame();

        player.advance_by_time(Duration::from_millis(500));
        assert!(
            player.current_frame_index > 0,
            "a duration-less stream should still decode past its first frame"
        );
        assert_ne!(player.get_current_frame().bytes, first_frame.bytes);
        assert!(!player.is_finished());

        player.advance_by_time(Duration::from_secs(60));
        assert!(
            player.is_finished(),
            "a duration-less stream should finish once its frames run out"
        );
    }
}
