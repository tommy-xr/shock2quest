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

use crate::resource_path;

pub struct VideoPlayer {
    width: u32,
    height: u32,

    current_time: Duration,

    duration: Duration,
    total_frame_count: i64,

    frames: Vec<RawTextureData>,
    decoder: ffmpeg::decoder::Video,
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
        // let duration = Duration::from_secs_f64(
        //     (input.duration() as f64 / f64::from(ffmpeg::ffi::AV_TIME_BASE)) * 1000000.0,
        // );
        let total_frame_count = input.frames();

        let mut frame_index = 0;

        let mut frames = Vec::new();

        let mut receive_and_process_decoded_frames =
            |decoder: &mut ffmpeg::decoder::Video| -> Result<(), ffmpeg::Error> {
                let mut fail_count = 0;
                let mut decoded = Video::empty();
                // loop {
                //     match decoder.receive_frame(&mut decoded) {
                //         Ok(_) => {
                //             println!("---receiving frame...");
                //             let mut rgb_frame = Video::empty();
                //             scaler.run(&decoded, &mut rgb_frame).unwrap();
                //             save_file(&rgb_frame, frame_index).unwrap();
                //             frame_index += 1;
                //         }
                //         Err(e) => {
                //             // Handle other errors as needed
                //             println!("received error: {:?}", e);
                //             break;
                //         }
                //     }
                // }
                while decoder.receive_frame(&mut decoded).is_ok() {
                    println!("---receiving frame...");
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

        for (stream, packet) in ictx.packets() {
            println!(
                "-- receiving packet: {} | {:?}",
                stream.index(),
                packet.pts()
            );
            if stream.index() == video_stream_index {
                println!("--- got video packet...");
                match decoder.send_packet(&packet) {
                    Ok(()) => receive_and_process_decoded_frames(&mut decoder).unwrap(),
                    Err(err) => println!("received err in send_packet: {:?}", err),
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
        println!("adding time: {:?}", time);
        self.current_time += time;
        println!("new self.current_time: {:?}", self.current_time);
    }

    pub fn get_current_frame(&self) -> RawTextureData {
        let ratio = self.current_time.as_secs_f64() / self.duration.as_secs_f64();

        let current_frame = (ratio * self.frames.len() as f64) as usize;

        let idx = current_frame.max(0).min(self.frames.len() - 1);
        println!(
            "duration: {:?} ratio: {} current_frame: {} idx: {}",
            self.duration, ratio, current_frame, idx
        );

        return self.frames[idx].clone();
    }
}

pub fn dump_frames(filename: &str) -> Result<(), ffmpeg::Error> {
    ffmpeg::init().unwrap();

    if let Ok(mut ictx) = input(&filename) {
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

        let mut frame_index = 0;

        let mut receive_and_process_decoded_frames =
            |decoder: &mut ffmpeg::decoder::Video| -> Result<(), ffmpeg::Error> {
                let mut fail_count = 0;
                let mut decoded = Video::empty();
                // loop {
                //     match decoder.receive_frame(&mut decoded) {
                //         Ok(_) => {
                //             println!("---receiving frame...");
                //             let mut rgb_frame = Video::empty();
                //             scaler.run(&decoded, &mut rgb_frame).unwrap();
                //             save_file(&rgb_frame, frame_index).unwrap();
                //             frame_index += 1;
                //         }
                //         Err(e) => {
                //             // Handle other errors as needed
                //             println!("received error: {:?}", e);
                //             break;
                //         }
                //     }
                // }
                while decoder.receive_frame(&mut decoded).is_ok() {
                    println!("---receiving frame...");
                    let mut rgb_frame = Video::empty();
                    scaler.run(&decoded, &mut rgb_frame).unwrap();
                    save_file(&rgb_frame, frame_index).unwrap();
                    frame_index += 1;
                }
                Ok(())
            };

        for (stream, packet) in ictx.packets() {
            println!(
                "-- receiving packet: {} | {:?}",
                stream.index(),
                packet.pts()
            );
            if stream.index() == video_stream_index {
                println!("--- got video packet...");
                match decoder.send_packet(&packet) {
                    Ok(()) => receive_and_process_decoded_frames(&mut decoder).unwrap(),
                    Err(err) => println!("received err in send_packet: {:?}", err),
                }
            }
        }
        decoder.send_eof()?;
        receive_and_process_decoded_frames(&mut decoder)?;
    }

    Ok(())
}

pub fn play_audio(
    filename: &str,
    context: &mut AudioContext<(), String>,
) -> Result<(), std::io::Error> {
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
    let target_sample_fmt = ffmpeg_next::format::Sample::I16(ffmpeg::format::sample::Type::Packed);

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

    //let clip = AudioClip::from_bytes(extracted_wav_bytes);
    let handle = AudioHandle::new();
    audio::test_audio(context, handle, None, Rc::new(clip));

    Ok(())
}

fn save_file(frame: &Video, index: usize) -> std::result::Result<(), std::io::Error> {
    let mut file = File::create(format!("frame{}.ppm", index))?;
    file.write_all(format!("P6\n{} {}\n255\n", frame.width(), frame.height()).as_bytes())?;
    file.write_all(frame.data(0))?;
    Ok(())
}
