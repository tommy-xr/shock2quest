extern crate ffmpeg_next as ffmpeg;

use engine::audio::{self, AudioClip, AudioContext, AudioHandle};
use engine::texture_format::{PixelFormat, RawTextureData};
use ffmpeg::format::{input, Pixel};
use ffmpeg::media::Type;
use ffmpeg::software::scaling::{context::Context, flag::Flags};
use ffmpeg::util::frame::video::Video;
use ffmpeg::ChannelLayout;
use std::env;
use std::fs::File;
use std::io::{prelude::*, BufReader};
use std::rc::Rc;
use std::time::Duration;

struct AudioPlayer;

impl AudioPlayer {
    pub fn from_filename(
        filename: &str,
        audio_context: &AudioContext,
    ) -> Result<AudioClip, ffmpeg::Error> {
        // 2. Open the media file
        let mut ictx = ffmpeg_next::format::input(&filename).unwrap();

        // 3. Find the audio stream
        let input = ictx
            .streams()
            .best(Type::Audio)
            .ok_or(ffmpeg::Error::StreamNotFound)?;
        let audio_stream_index = input.index();

        let context_decoder = ffmpeg::codec::context::Context::from_parameters(input.parameters())?;
        let mut audio_decoder = context_decoder.decoder().audio()?;

        // panic!(
        //     "decoder id: {:?} decoder rate: {:?} decoder channels: {:?} channel layout: {:?}",
        //     audio_decoder.id(),
        //     audio_decoder.rate(),
        //     audio_decoder.channels(),
        //     audio_decoder.channel_layout(),
        // );

        let source_channel_layout = ChannelLayout::STEREO;
        // let source_sample_rate = audio_decoder.rate();
        let source_sample_rate = audio_decoder.rate();
        let source_sample_fmt = audio_decoder.format();

        // Target audio parameters
        let target_channel_layout = ffmpeg::util::channel_layout::ChannelLayout::MONO;
        let target_channel_count = 1;
        let target_sample_rate = 44100; // For example, 44.1 kHz
        let target_sample_fmt =
            ffmpeg_next::format::Sample::I16(ffmpeg::format::sample::Type::Packed);

        // Set up the resampler
        let mut swr = ffmpeg::software::resampler(
            (source_sample_fmt, source_channel_layout, source_sample_rate),
            (target_sample_fmt, target_channel_layout, target_sample_rate),
            //(target_sample_fmt, target_channel_layout, target_sample_rate),
        )
        .unwrap();

        // 5. Decode audio packets
        let mut decoded_audio_samples: Vec<i16> = Vec::new();

        for (stream, packet) in ictx.packets() {
            if stream.index() == audio_stream_index {
                audio_decoder.send_packet(&packet).unwrap();
                let mut audio_frame = ffmpeg_next::util::frame::audio::Audio::empty();

                while audio_decoder.receive_frame(&mut audio_frame).is_ok() {
                    let mut decoded_audio_frame = ffmpeg_next::util::frame::audio::Audio::empty();
                    audio_frame.set_channel_layout(source_channel_layout);
                    let _option_delay = swr.run(&audio_frame, &mut decoded_audio_frame).unwrap();

                    let plane_count = decoded_audio_frame.planes();
                    let data: &[i16] = decoded_audio_frame.plane(0);
                    decoded_audio_samples.extend_from_slice(&data);
                }
            }
        }
        // let remapped_samples: Vec<i16> = decoded_audio_samples
        //     .chunks(2)
        //     .filter_map(|chunk| {
        //         if chunk.len() == 2 {
        //             Some((chunk[0] as i16) | ((chunk[1] as i16) << 8))
        //         } else {
        //             // Handle the case where there's an odd number of bytes, if necessary
        //             None
        //         }
        //     })
        //     .collect();

        // Alternate for u8
        // let remapped_samples = decoded_audio_samples
        //     .iter()
        //     .map(|&x| (x as i16 - 128) * 256)
        //     .collect::<Vec<_>>();

        let remapped_samples = decoded_audio_samples;

        let extracted_wav_file = File::open(resource_path("cutscenes/test.wav")).unwrap();
        let mut extracted_wav_reader = BufReader::new(extracted_wav_file);
        let mut extracted_wav_bytes = vec![];
        let _ = extracted_wav_reader
            .read_to_end(&mut extracted_wav_bytes)
            .unwrap();

        let clip = AudioClip::from_raw(target_channel_count, target_sample_rate, remapped_samples);
    }
}
