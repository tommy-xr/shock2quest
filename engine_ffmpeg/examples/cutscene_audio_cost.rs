//! What a cutscene's audio costs the scene that plays it: how long opening it
//! blocks before the first video frame can be shown, and how much decoded PCM
//! is resident while it plays. Compares decoding the whole stream up front
//! against streaming it.
//!
//! ```text
//! cargo run --release -p engine_ffmpeg --example cutscene_audio_cost -- <video>...
//! ```

use std::time::Instant;

use engine_ffmpeg::AudioPlayer;

const BYTES_PER_SAMPLE: usize = std::mem::size_of::<i16>();

fn mib(samples: usize) -> f64 {
    (samples * BYTES_PER_SAMPLE) as f64 / (1024.0 * 1024.0)
}

fn main() {
    engine_ffmpeg::init().expect("ffmpeg init");

    let files: Vec<String> = std::env::args().skip(1).collect();
    if files.is_empty() {
        eprintln!("usage: cutscene_audio_cost <video>...");
        std::process::exit(2);
    }

    for file in files {
        println!("{file}");

        let started = Instant::now();
        let clip = AudioPlayer::from_filename(&file).expect("decode");
        let full_decode = started.elapsed();
        let seconds = clip
            .total_duration()
            .expect("a decoded clip knows its length")
            .as_secs_f64();
        let total_samples = (seconds * 44100.0).round() as usize;
        drop(clip);
        println!(
            "  decode up front: blocks {:>8.3} s, holds {:>8.2} MiB ({:.1} s of audio)",
            full_decode.as_secs_f64(),
            mib(total_samples),
            seconds,
        );

        let started = Instant::now();
        let mut stream = AudioPlayer::open_stream(&file).expect("open");
        let open = started.elapsed();
        let first_sample = {
            let started = Instant::now();
            assert!(stream.next().is_some(), "the stream must yield audio");
            started.elapsed()
        };
        println!(
            "  stream:          blocks {:>8.3} s, holds {:>8.2} MiB at most, first sample after {:.3} s",
            open.as_secs_f64(),
            mib(AudioPlayer::buffered_sample_capacity()),
            (open + first_sample).as_secs_f64(),
        );
    }
}
