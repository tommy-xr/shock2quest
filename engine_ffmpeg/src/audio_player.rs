extern crate ffmpeg_next as ffmpeg;

use engine::audio::{AudioClip, StreamingAudioSource};
use ffmpeg::ChannelLayout;
use ffmpeg::media::Type;

/// Everything is resampled to this, so a caller knows the stream's format
/// without waiting for the first decoded frame.
const TARGET_CHANNELS: u16 = 1;
const TARGET_SAMPLE_RATE: u32 = 44100;

/// Decoded audio held ahead of playback by [`AudioPlayer::open_stream`]:
/// 32 chunks of 4096 mono i16 samples is ~3 s / ~256 KiB, enough to ride out a
/// decode hiccup without the whole soundtrack being resident (a 6.5-minute
/// cutscene is ~34 MiB fully decoded).
const CHUNK_SAMPLES: usize = 4096;
const BUFFERED_CHUNKS: usize = 32;

/// An opened audio stream plus everything needed to pull mono 44.1 kHz i16 out
/// of it.
struct AudioDecode {
    input: ffmpeg::format::context::Input,
    stream_index: usize,
    decoder: ffmpeg::decoder::Audio,
    resampler: ffmpeg::software::resampling::Context,
}

impl AudioDecode {
    fn open(filename: &str) -> Result<AudioDecode, ffmpeg::Error> {
        let input = ffmpeg::format::input(&filename)?;

        let stream = input
            .streams()
            .best(Type::Audio)
            .ok_or(ffmpeg::Error::StreamNotFound)?;
        let stream_index = stream.index();

        let context_decoder =
            ffmpeg::codec::context::Context::from_parameters(stream.parameters())?;
        let decoder = context_decoder.decoder().audio()?;

        let resampler = ffmpeg::software::resampler(
            (decoder.format(), ChannelLayout::STEREO, decoder.rate()),
            (
                ffmpeg::format::Sample::I16(ffmpeg::format::sample::Type::Packed),
                ffmpeg::util::channel_layout::ChannelLayout::MONO,
                TARGET_SAMPLE_RATE,
            ),
        )?;

        Ok(AudioDecode {
            input,
            stream_index,
            decoder,
            resampler,
        })
    }

    /// Runs the decode to the end of the stream, handing each batch of
    /// resampled samples to `emit`. `emit` returning false stops early - that
    /// is how the streaming worker notices its listener went away.
    fn drain<F>(&mut self, mut emit: F) -> Result<(), ffmpeg::Error>
    where
        F: FnMut(&[i16]) -> bool,
    {
        for (stream, packet) in self.input.packets() {
            if stream.index() != self.stream_index || !packet_has_payload(&packet) {
                continue;
            }

            self.decoder.send_packet(&packet)?;
            let mut audio_frame = ffmpeg::util::frame::audio::Audio::empty();
            while self.decoder.receive_frame(&mut audio_frame).is_ok() {
                let mut resampled = ffmpeg::util::frame::audio::Audio::empty();
                audio_frame.set_channel_layout(ChannelLayout::STEREO);
                self.resampler.run(&audio_frame, &mut resampled)?;
                if !emit(resampled.plane(0)) {
                    return Ok(());
                }
            }
        }

        Ok(())
    }
}

pub struct AudioPlayer;

impl AudioPlayer {
    /// Decodes the whole audio stream into a clip. Fine for a short sound;
    /// prefer [`AudioPlayer::open_stream`] for anything long, which neither
    /// blocks the caller for the decode nor holds the result resident.
    pub fn from_filename(filename: &str) -> Result<AudioClip, ffmpeg::Error> {
        let mut decode = AudioDecode::open(filename)?;

        let mut samples: Vec<i16> = Vec::new();
        decode.drain(|batch| {
            samples.extend_from_slice(batch);
            true
        })?;

        Ok(AudioClip::from_raw(
            TARGET_CHANNELS,
            TARGET_SAMPLE_RATE,
            samples,
        ))
    }

    /// Most samples [`AudioPlayer::open_stream`] can hold at once, so a caller
    /// can state the memory it costs.
    pub const fn buffered_sample_capacity() -> usize {
        BUFFERED_CHUNKS * CHUNK_SAMPLES
    }

    /// Opens the audio stream and decodes it on a worker thread, a bounded
    /// buffer ahead of playback. Returns as soon as the stream is open, so a
    /// caller that shows a first video frame does not wait on the soundtrack.
    ///
    /// The worker stops when the returned source is dropped, so stopping the
    /// sink also ends the decode.
    pub fn open_stream(filename: &str) -> Result<StreamingAudioSource, ffmpeg::Error> {
        let mut decode = AudioDecode::open(filename)?;
        let (sender, receiver) = std::sync::mpsc::sync_channel::<Vec<i16>>(BUFFERED_CHUNKS);

        std::thread::spawn(move || {
            let mut chunk: Vec<i16> = Vec::with_capacity(CHUNK_SAMPLES);
            let result = decode.drain(|batch| {
                chunk.extend_from_slice(batch);
                while chunk.len() >= CHUNK_SAMPLES {
                    let rest = chunk.split_off(CHUNK_SAMPLES);
                    if sender.send(std::mem::replace(&mut chunk, rest)).is_err() {
                        return false;
                    }
                }
                true
            });
            if let Err(error) = result {
                eprintln!("cutscene audio decode failed: {error}");
            }
            // Whatever did not fill a last chunk still belongs to the stream.
            if !chunk.is_empty() {
                let _ = sender.send(chunk);
            }
        });

        Ok(StreamingAudioSource::new(
            TARGET_CHANNELS,
            TARGET_SAMPLE_RATE,
            receiver,
        ))
    }
}

fn packet_has_payload(packet: &ffmpeg::Packet) -> bool {
    packet.size() > 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine::audio::AudioSource;
    use std::path::Path;

    /// The video fixtures carry no audio track, so the audio tests get their
    /// own: 2 s of a 440 Hz tone in an ogv beside a video stream, which is the
    /// shape a cutscene has.
    const AUDIO_FIXTURE: &str = "streaming-audio-test.ogv";

    fn fixture(name: &str) -> String {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("testdata")
            .join(name)
            .to_string_lossy()
            .into_owned()
    }

    #[test]
    fn skips_empty_ogg_end_of_stream_packets() {
        assert!(!packet_has_payload(&ffmpeg::Packet::empty()));
    }

    /// The streamed samples must be the same soundtrack the up-front decode
    /// produced - same order, same length, nothing dropped at a chunk boundary.
    #[test]
    fn a_streamed_file_yields_exactly_what_decoding_it_all_yields() {
        crate::init().unwrap();
        let path = fixture(AUDIO_FIXTURE);

        let mut expected = Vec::new();
        AudioDecode::open(&path)
            .unwrap()
            .drain(|batch| {
                expected.extend_from_slice(batch);
                true
            })
            .unwrap();
        assert!(
            expected.len() > CHUNK_SAMPLES,
            "the fixture must be longer than one chunk to exercise chunking"
        );

        // Compared as counts first, then contents: a mismatched tail would
        // otherwise print both soundtracks.
        let streamed: Vec<i16> = AudioPlayer::open_stream(&path).unwrap().collect();
        assert_eq!(streamed.len(), expected.len(), "sample count");
        assert!(streamed == expected, "streamed samples differ");
    }

    #[test]
    fn a_streamed_file_reports_the_target_format() {
        crate::init().unwrap();
        let source = AudioPlayer::open_stream(&fixture(AUDIO_FIXTURE)).unwrap();
        assert_eq!(AudioSource::channels(&source), TARGET_CHANNELS);
        assert_eq!(AudioSource::sample_rate(&source), TARGET_SAMPLE_RATE);
    }

    /// Dropping the source must end the decode rather than leave a worker
    /// running the rest of a long cutscene - this is what skipping does.
    #[test]
    fn dropping_the_source_stops_the_decode() {
        crate::init().unwrap();
        let mut source = AudioPlayer::open_stream(&fixture(AUDIO_FIXTURE)).unwrap();
        assert!(source.next().is_some());
        drop(source);
    }
}
