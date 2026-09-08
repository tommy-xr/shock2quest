use std::collections::{HashMap, HashSet};
use std::hash::Hash;
use std::io::Cursor;
use std::rc::Rc;
use std::time::Instant;

use cgmath::{Vector3, vec3};
use rodio::buffer::SamplesBuffer;
use rodio::source::{Buffered, ChannelVolume, Source};
use rodio::{Decoder, OutputStream, OutputStreamHandle, Sink, SpatialSink};

/// Re-exported so a caller can ask a [`StreamingAudioSource`] about its format
/// without taking its own rodio dependency.
pub use rodio::source::Source as AudioSource;

use crate::audio_log;
use tracing::trace;

use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_HANDLE_ID: AtomicU64 = AtomicU64::new(0);

const SOUND_SCALE_FACTOR: f32 = 5.0;
// `play_audio` historically used a SpatialSink placed midway between the ears;
// rodio's centered spatial mix applies 0.75 to both channels. Retain that
// listener-relative baseline when using a regular Sink for authored pan.
const STATIC_CENTER_GAIN: f32 = 0.75;

#[derive(Clone, Debug)]
pub struct AudioHandle {
    id: u64,
}

impl Default for AudioHandle {
    fn default() -> Self {
        Self::new()
    }
}

impl AudioHandle {
    pub fn new() -> AudioHandle {
        let id = NEXT_HANDLE_ID.fetch_add(1, Ordering::SeqCst);
        AudioHandle { id }
    }

    /// The opaque handle id - exposed so diagnostics (the audio log) can
    /// correlate a play with the `StopSound` that ends it.
    pub fn id(&self) -> u64 {
        self.id
    }
}

pub struct AudioChannel {
    name: String,
}

/// A live, sink-owned loop. Elapsed time follows the audio wall clock.
pub struct ActiveLoop<T> {
    pub handle: u64,
    pub sample: String,
    pub owner: &'static str,
    pub source: Option<T>,
    pub elapsed_secs: f64,
}

struct LoopPlayback {
    handle: u64,
    sample: String,
    started: Instant,
}

impl LoopPlayback {
    fn new(handle: u64, clip: &AudioClip) -> Self {
        Self {
            handle,
            sample: clip.sample.clone(),
            started: Instant::now(),
        }
    }

    fn snapshot<T>(&self, owner: &'static str, source: Option<T>) -> ActiveLoop<T> {
        ActiveLoop {
            handle: self.handle,
            sample: self.sample.clone(),
            owner,
            source,
            elapsed_secs: self.started.elapsed().as_secs_f64(),
        }
    }
}

/// Gain, stereo channel attenuation and repeat behavior for
/// listener-relative playback.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AudioPlaybackSettings {
    pub gain: f32,
    pub channel_gains: [f32; 2],
    /// Repeat forever instead of playing once. `stop_audio` ends it.
    pub looping: bool,
}

impl Default for AudioPlaybackSettings {
    fn default() -> Self {
        Self {
            gain: 1.0,
            channel_gains: [STATIC_CENTER_GAIN, STATIC_CENTER_GAIN],
            looping: false,
        }
    }
}

impl AudioPlaybackSettings {
    pub fn listener_relative(gain: f32, pan_gains: [f32; 2]) -> Self {
        Self {
            gain,
            channel_gains: [
                pan_gains[0] * STATIC_CENTER_GAIN,
                pan_gains[1] * STATIC_CENTER_GAIN,
            ],
            ..Default::default()
        }
    }
}

impl AudioChannel {
    pub fn new(name: String) -> AudioChannel {
        AudioChannel { name }
    }
}

pub trait BackgroundMusic<TCue> {
    fn next_clip(&mut self, cue: Option<TCue>) -> Option<Rc<AudioClip>>;
}

#[derive(Clone, Copy, Debug)]
struct TrackedEmitter<TSourceKey> {
    position: Vector3<f32>,
    source: Option<TSourceKey>,
}

impl<TSourceKey> TrackedEmitter<TSourceKey>
where
    TSourceKey: Copy,
{
    fn new(position: Vector3<f32>, source: Option<TSourceKey>) -> Self {
        Self { position, source }
    }

    fn refresh<F>(&mut self, mut source_position: F)
    where
        F: FnMut(TSourceKey) -> Option<Vector3<f32>>,
    {
        if let Some(position) = self.source.and_then(&mut source_position) {
            self.position = position;
        }
    }

    fn position(&self) -> Vector3<f32> {
        self.position
    }
}

enum SinkAdapter<TSourceKey> {
    StaticSink(Sink),
    PositionalSink {
        sink: SpatialSink,
        emitter: TrackedEmitter<TSourceKey>,
    },
}

impl<TSourceKey> SinkAdapter<TSourceKey>
where
    TSourceKey: Copy,
{
    fn fixed(sink: Sink) -> SinkAdapter<TSourceKey> {
        SinkAdapter::StaticSink(sink)
    }

    fn positional(
        sink: SpatialSink,
        position: Vector3<f32>,
        source: Option<TSourceKey>,
    ) -> SinkAdapter<TSourceKey> {
        SinkAdapter::PositionalSink {
            sink,
            emitter: TrackedEmitter::new(position, source),
        }
    }

    fn update_spatial_position<F>(
        &mut self,
        left_ear_position: [f32; 3],
        right_ear_position: [f32; 3],
        source_position: &mut F,
    ) where
        F: FnMut(TSourceKey) -> Option<Vector3<f32>>,
    {
        match self {
            SinkAdapter::StaticSink(_) => (),
            SinkAdapter::PositionalSink { sink, emitter } => {
                emitter.refresh(source_position);
                sink.set_emitter_position(to_audio_position(emitter.position()));
                sink.set_left_ear_position(left_ear_position);
                sink.set_right_ear_position(right_ear_position);
            }
        }
    }

    fn empty(&self) -> bool {
        match self {
            SinkAdapter::StaticSink(sink) => sink.empty(),
            SinkAdapter::PositionalSink { sink, .. } => sink.empty(),
        }
    }

    fn stop(&self) {
        match self {
            SinkAdapter::StaticSink(sink) => sink.stop(),
            SinkAdapter::PositionalSink { sink, .. } => sink.stop(),
        }
    }
}

pub struct AudioContext<TAmbientKey, TCue>
where
    TCue: Clone,
    TAmbientKey: Hash + Eq + Copy,
{
    #[allow(dead_code)]
    stream: OutputStream,
    handle: OutputStreamHandle,
    #[allow(dead_code)]
    sinks: Vec<Sink>,
    channel_to_last_handle: HashMap<String, u64>,
    handle_to_sink: HashMap<u64, SinkAdapter<TAmbientKey>>,
    handle_loops: HashMap<u64, LoopPlayback>,
    // Background music
    background_music: Option<Sink>,
    background_music_player: Option<Box<dyn BackgroundMusic<TCue>>>,
    next_music_cue: Option<TCue>,

    // Environmental sounds
    environmental_sink: Option<(Sink, LoopPlayback)>,

    // Position audio context
    last_left_ear_position: Vector3<f32>,
    last_right_ear_position: Vector3<f32>,

    // Ambient, positional sounds
    ambient_sounds: HashMap<TAmbientKey, (SpatialSink, LoopPlayback)>,
}

impl<TAmbientKey, TCue> Default for AudioContext<TAmbientKey, TCue>
where
    TAmbientKey: Hash + Eq + Copy,
    TCue: Clone,
{
    fn default() -> Self {
        Self::new()
    }
}

impl<TAmbientKey, TCue> AudioContext<TAmbientKey, TCue>
where
    TAmbientKey: Hash + Eq + Copy,
    TCue: Clone,
{
    pub fn new() -> AudioContext<TAmbientKey, TCue> {
        let (stream, handle) = rodio::OutputStream::try_default().unwrap();
        AudioContext {
            stream,
            handle,
            sinks: vec![],
            //spatial_sinks: vec![],
            handle_to_sink: HashMap::new(),
            handle_loops: HashMap::new(),
            channel_to_last_handle: HashMap::new(),
            background_music: None,
            background_music_player: None,
            next_music_cue: None,

            environmental_sink: None,

            last_left_ear_position: vec3(-0.125, 0.0, 0.0),
            last_right_ear_position: vec3(0.125, 0.0, 0.0),

            ambient_sounds: HashMap::new(),
        }
    }

    pub fn set_background_music(
        &mut self,
        background_music_player: Box<dyn BackgroundMusic<TCue>>,
    ) {
        self.background_music_player = Some(background_music_player);
        self.next_music_cue = None;
    }

    pub fn stop_background_music(&mut self) {
        self.background_music_player = None;
        self.next_music_cue = None;
    }

    pub fn set_background_music_cue(&mut self, cue: TCue) {
        self.next_music_cue = Some(cue)
    }

    pub fn set_environmental_sound(&mut self, clip: Rc<AudioClip>) {
        self.stop_environmental_sound();
        let sink = rodio::Sink::try_new(&self.handle).unwrap();
        clip.add_to_sink_looping(&sink);
        sink.set_volume(0.2);
        sink.play();
        self.environmental_sink = Some((sink, LoopPlayback::new(AudioHandle::new().id(), &clip)));
    }

    pub fn stop_environmental_sound(&mut self) {
        if let Some((sink, _)) = self.environmental_sink.take() {
            sink.stop();
        }
    }

    /// Mission beds belong to the outgoing scene, including silent loading/menu scenes.
    pub fn stop_ambient_sounds(&mut self) {
        self.stop_environmental_sound();
        for (_, (sink, _)) in self.ambient_sounds.drain() {
            sink.stop();
        }
    }

    pub fn active_loops(&self) -> Vec<ActiveLoop<TAmbientKey>> {
        let mut loops = Vec::new();
        for (handle, playback) in &self.handle_loops {
            if self
                .handle_to_sink
                .get(handle)
                .is_some_and(|sink| !sink.empty())
            {
                loops.push(playback.snapshot("scene", None));
            }
        }
        if let Some((sink, playback)) = &self.environmental_sink {
            if !sink.empty() {
                loops.push(playback.snapshot("environmental", None));
            }
        }
        for (entity, (sink, playback)) in &self.ambient_sounds {
            if !sink.empty() {
                loops.push(playback.snapshot("ambient_emitter", Some(*entity)));
            }
        }
        loops.sort_by_key(|entry| entry.handle);
        loops
    }

    pub fn update<F>(
        &mut self,
        left_ear_position: Vector3<f32>,
        right_ear_position: Vector3<f32>,
        current_ambient_sounds: Vec<(TAmbientKey, Vector3<f32>, Rc<AudioClip>)>,
        mut source_position: F,
    ) where
        F: FnMut(TAmbientKey) -> Option<Vector3<f32>>,
    {
        audio_log!(DEBUG, "Audio system update started");
        self.update_background_music();

        trace!(
            "updating {} ambient sounds...",
            current_ambient_sounds.len()
        );

        let left_ear_position = to_audio_position(left_ear_position);
        let right_ear_position = to_audio_position(right_ear_position);

        self.last_left_ear_position = vec3(
            left_ear_position[0],
            left_ear_position[1],
            left_ear_position[2],
        );
        self.last_right_ear_position = vec3(
            right_ear_position[0],
            right_ear_position[1],
            right_ear_position[2],
        );

        self.handle_to_sink.retain(|_, sink| !sink.empty());
        self.handle_loops
            .retain(|handle, _| self.handle_to_sink.contains_key(handle));
        // Update positional sounds
        for sink in self.handle_to_sink.values_mut() {
            sink.update_spatial_position(
                left_ear_position,
                right_ear_position,
                &mut source_position,
            );
        }

        // Build hash map for new ambient sounds
        let mut current_sound_hash = HashMap::new();
        for (key, pos, clip) in &current_ambient_sounds {
            current_sound_hash.insert(key, (pos, clip));
        }

        let mut sounds_to_remove = HashSet::new();
        // Refresh nearby emitters; explicitly stop those that left the active set.
        for (key, (sink, _)) in &self.ambient_sounds {
            if let Some(current_sound) = current_sound_hash.get(key) {
                sink.set_emitter_position(to_audio_position(*current_sound.0));

                // TODO
                sink.set_left_ear_position(left_ear_position);
                sink.set_right_ear_position(right_ear_position);

                sink.set_volume(0.5);
            } else {
                sink.stop();
                sounds_to_remove.insert(*key);
            }
        }

        // Second pass - remove any sounds that are no longer playing
        for key in sounds_to_remove {
            self.ambient_sounds.remove(&key);
        }

        // Third pass - add any new sounds
        for (key, pos, clip) in &current_ambient_sounds {
            if !self.ambient_sounds.contains_key(key) {
                let sink = rodio::SpatialSink::try_new(
                    &self.handle,
                    to_audio_position(*pos),
                    left_ear_position,
                    right_ear_position,
                )
                .unwrap();

                clip.add_to_spatial_sink_looping(&sink);
                sink.set_volume(0.5);
                self.ambient_sounds.insert(
                    *key,
                    (sink, LoopPlayback::new(AudioHandle::new().id(), clip)),
                );
            }
        }
    }

    fn update_background_music(&mut self) {
        if let Some(background_music) = &self.background_music {
            if background_music.len() == 0 {
                self.background_music = None;
            }
        }

        if self.background_music.is_none() && self.background_music_player.is_some() {
            let maybe_next = self
                .background_music_player
                .as_mut()
                .unwrap()
                .next_clip(self.next_music_cue.clone());
            if let Some(next_song) = maybe_next {
                let sink = rodio::Sink::try_new(&self.handle).unwrap();
                next_song.add_to_sink(&sink);
                sink.play();
                self.next_music_cue = None;
                self.background_music = Some(sink);
            }
        }
    }
}

#[derive(Clone)]
enum SourceType {
    Bytes(Buffered<Decoder<Cursor<Vec<u8>>>>),
    Raw(Buffered<SamplesBuffer<i16>>),
}

#[derive(Clone)]
pub struct AudioClip {
    source: SourceType,
    sample: String,
    /// Total playback length, when the decoder can report it. Used by the
    /// audio log to tell how long a played clip occupies its channel.
    total_duration: Option<std::time::Duration>,
}

impl AudioClip {
    pub fn with_sample_name(mut self, sample: String) -> Self {
        self.sample = sample;
        self
    }

    fn add_to_sink_looping(&self, sink: &Sink) {
        match &self.source {
            SourceType::Bytes(source) => sink.append(source.clone().repeat_infinite()),
            SourceType::Raw(source) => sink.append(source.clone().repeat_infinite()),
        }
    }

    fn add_to_spatial_sink_looping(&self, sink: &SpatialSink) {
        match &self.source {
            SourceType::Bytes(source) => sink.append(source.clone().repeat_infinite()),
            SourceType::Raw(source) => sink.append(source.clone().repeat_infinite()),
        }
    }
    /// Playback length of the clip, if the underlying source knows it.
    pub fn total_duration(&self) -> Option<std::time::Duration> {
        self.total_duration
    }

    pub fn add_to_spatial_sink(&self, sink: &SpatialSink) {
        match &self.source {
            SourceType::Bytes(source) => sink.append(source.clone()),
            SourceType::Raw(source) => sink.append(source.clone()),
        }
    }
    pub fn add_to_sink(&self, sink: &Sink) {
        match &self.source {
            SourceType::Bytes(source) => sink.append(source.clone()),
            SourceType::Raw(source) => sink.append(source.clone()),
        }
    }

    fn add_to_sink_with_settings(&self, sink: &Sink, settings: AudioPlaybackSettings) {
        sink.set_volume(settings.gain);
        let channel_volumes = settings.channel_gains.to_vec();
        // A looping clip repeats inside the source - a seamless bed (a menu
        // hum, a machine loop) rather than the re-append-when-empty pattern,
        // which leaves an audible gap of however long the caller takes to
        // notice.
        match (&self.source, settings.looping) {
            (SourceType::Bytes(source), false) => {
                sink.append(ChannelVolume::new(source.clone(), channel_volumes))
            }
            (SourceType::Raw(source), false) => {
                sink.append(ChannelVolume::new(source.clone(), channel_volumes))
            }
            (SourceType::Bytes(source), true) => sink.append(ChannelVolume::new(
                source.clone().repeat_infinite(),
                channel_volumes,
            )),
            (SourceType::Raw(source), true) => sink.append(ChannelVolume::new(
                source.clone().repeat_infinite(),
                channel_volumes,
            )),
        }
    }
    pub fn from_bytes(bytes: Vec<u8>) -> AudioClip {
        // Most shipped SS2 samples are IMA ADPCM WAVs, which rodio decodes via
        // symphonia - and that decoder never reports a length. Read it out of
        // the RIFF header instead, falling back to the decoder for other formats.
        let total_duration = wav_duration(&bytes);
        let buf = Cursor::new(bytes);
        let decoder = rodio::Decoder::new(buf).unwrap();
        // Ask the decoder, not the `Buffered` wrapper: `Buffered` cannot know
        // the length until the whole source has been consumed.
        let total_duration = total_duration.or_else(|| decoder.total_duration());
        let source = decoder.buffered();
        AudioClip {
            source: SourceType::Bytes(source),
            sample: String::new(),
            total_duration,
        }
    }

    pub fn from_raw(channels: u16, sample_rate: u32, data: Vec<i16>) -> AudioClip {
        let samples = rodio::buffer::SamplesBuffer::new(channels, sample_rate, data);
        let total_duration = samples.total_duration();
        let source = samples.buffered();
        AudioClip {
            source: SourceType::Raw(source),
            sample: String::new(),
            total_duration,
        }
    }
}

/// Playback length of a RIFF/WAVE buffer, from `nAvgBytesPerSec` and the
/// `data` chunk size. Format-agnostic (works for the PCM and IMA ADPCM samples
/// the game ships) and does not require decoding the clip.
fn wav_duration(bytes: &[u8]) -> Option<std::time::Duration> {
    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return None;
    }

    let read_u32 = |at: usize| -> Option<usize> {
        bytes
            .get(at..at + 4)
            .and_then(|raw| raw.try_into().ok())
            .map(|raw| u32::from_le_bytes(raw) as usize)
    };

    let mut avg_bytes_per_sec = None;
    let mut data_len = None;
    let mut offset = 12;
    while offset + 8 <= bytes.len() {
        let id = &bytes[offset..offset + 4];
        let size = read_u32(offset + 4)?;
        let body = offset + 8;
        if id == b"fmt " && size >= 12 && body + 12 <= bytes.len() {
            avg_bytes_per_sec = Some(read_u32(body + 8)?);
        } else if id == b"data" {
            data_len = Some(size.min(bytes.len().saturating_sub(body)));
        }
        // Chunks are word-aligned.
        offset = body.saturating_add(size).saturating_add(size & 1);
    }

    match (avg_bytes_per_sec, data_len) {
        (Some(rate), Some(len)) if rate > 0 => {
            Some(std::time::Duration::from_secs_f64(len as f64 / rate as f64))
        }
        _ => None,
    }
}

/// A rodio source fed chunk by chunk over a channel, for audio too long to
/// decode into an [`AudioClip`] up front - a feature-length cutscene is minutes
/// of PCM that would otherwise all be resident before its first frame shows.
///
/// `next` blocks while the producer catches up, which is what rodio's own file
/// decoders do in the mixer callback anyway, and ends when the sender drops -
/// so end-of-stream and a playback cut short by [`stop_audio`] finish the same
/// way.
pub struct StreamingAudioSource {
    chunks: std::sync::mpsc::Receiver<Vec<i16>>,
    current: std::vec::IntoIter<i16>,
    channels: u16,
    sample_rate: u32,
}

impl StreamingAudioSource {
    pub fn new(
        channels: u16,
        sample_rate: u32,
        chunks: std::sync::mpsc::Receiver<Vec<i16>>,
    ) -> StreamingAudioSource {
        StreamingAudioSource {
            chunks,
            current: Vec::new().into_iter(),
            channels,
            sample_rate,
        }
    }
}

impl Iterator for StreamingAudioSource {
    type Item = i16;

    fn next(&mut self) -> Option<i16> {
        loop {
            if let Some(sample) = self.current.next() {
                return Some(sample);
            }
            self.current = self.chunks.recv().ok()?.into_iter();
        }
    }
}

impl Source for StreamingAudioSource {
    /// The stream never changes format, so the whole thing is one span of
    /// unknown length.
    fn current_frame_len(&self) -> Option<usize> {
        None
    }

    fn channels(&self) -> u16 {
        self.channels
    }

    fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    fn total_duration(&self) -> Option<std::time::Duration> {
        None
    }
}

/// Plays a [`StreamingAudioSource`] at the listener origin, on the same
/// listener-relative gain as [`play_audio`] so a streamed soundtrack sounds
/// like the fully-decoded clip it replaces. [`stop_audio`] ends it.
pub fn play_streaming_audio<TAmbientKey: Hash + Eq + Copy, TCue: Clone>(
    context: &mut AudioContext<TAmbientKey, TCue>,
    handle: AudioHandle,
    source: StreamingAudioSource,
) -> Vec<u64> {
    let id = handle.id;
    let preempted = prepare_audio_play(context, &handle, None);
    let sink = rodio::Sink::try_new(&context.handle).unwrap();
    let settings = AudioPlaybackSettings::default();
    sink.set_volume(settings.gain);
    sink.append(ChannelVolume::new(source, settings.channel_gains.to_vec()));
    context.handle_to_sink.insert(id, SinkAdapter::fixed(sink));
    preempted
}

pub fn stop_audio<TAmbientKey: Hash + Eq + Copy, TCue: Clone>(
    context: &mut AudioContext<TAmbientKey, TCue>,
    handle: AudioHandle,
) {
    let maybe_sink = context.handle_to_sink.remove(&handle.id);
    context.handle_loops.remove(&handle.id);

    if let Some(sink) = maybe_sink {
        sink.stop();
    }
}

/// Plays audio at the listener origin (non-spatial).
pub fn play_audio<TAmbientKey: Hash + Eq + Copy, TCue: Clone>(
    context: &mut AudioContext<TAmbientKey, TCue>,
    handle: AudioHandle,
    maybe_channel: Option<AudioChannel>,
    audio_clip: Rc<AudioClip>,
) -> Vec<u64> {
    play_audio_with_settings(
        context,
        handle,
        maybe_channel,
        audio_clip,
        AudioPlaybackSettings::default(),
    )
}

pub fn play_audio_with_settings<TAmbientKey: Hash + Eq + Copy, TCue: Clone>(
    context: &mut AudioContext<TAmbientKey, TCue>,
    handle: AudioHandle,
    maybe_channel: Option<AudioChannel>,
    audio_clip: Rc<AudioClip>,
    settings: AudioPlaybackSettings,
) -> Vec<u64> {
    let id = handle.id;
    let preempted = prepare_audio_play(context, &handle, maybe_channel);
    let sink = rodio::Sink::try_new(&context.handle).unwrap();
    audio_clip.add_to_sink_with_settings(&sink, settings);
    if settings.looping {
        context
            .handle_loops
            .insert(id, LoopPlayback::new(id, &audio_clip));
    }

    context.handle_to_sink.insert(id, SinkAdapter::fixed(sink));
    preempted
}

pub fn play_spatial_audio<TAmbientKey: Hash + Eq + Copy, TCue: Clone>(
    context: &mut AudioContext<TAmbientKey, TCue>,
    position: Vector3<f32>,
    source: Option<TAmbientKey>,
    handle: AudioHandle,
    maybe_channel: Option<AudioChannel>,
    audio_clip: Rc<AudioClip>,
) -> Vec<u64> {
    play_spatial_audio_with_gain(
        context,
        position,
        source,
        handle,
        maybe_channel,
        audio_clip,
        1.0,
    )
}

pub fn play_spatial_audio_with_gain<TAmbientKey: Hash + Eq + Copy, TCue: Clone>(
    context: &mut AudioContext<TAmbientKey, TCue>,
    position: Vector3<f32>,
    source: Option<TAmbientKey>,
    handle: AudioHandle,
    maybe_channel: Option<AudioChannel>,
    audio_clip: Rc<AudioClip>,
    gain: f32,
) -> Vec<u64> {
    let id = handle.id;
    let scaled_position = position / SOUND_SCALE_FACTOR;
    let (sink, preempted) =
        play_audio_core(context, scaled_position, handle, maybe_channel, audio_clip);
    sink.set_volume(gain);

    context
        .handle_to_sink
        .insert(id, SinkAdapter::positional(sink, position, source));
    preempted
}

fn to_audio_position(position: Vector3<f32>) -> [f32; 3] {
    [
        position.x / SOUND_SCALE_FACTOR,
        position.y / SOUND_SCALE_FACTOR,
        position.z / SOUND_SCALE_FACTOR,
    ]
}

pub fn play_audio_core<TAmbientKey: Hash + Eq + Copy, TCue: Clone>(
    context: &mut AudioContext<TAmbientKey, TCue>,
    position: Vector3<f32>,
    handle: AudioHandle,
    maybe_channel: Option<AudioChannel>,
    audio_clip: Rc<AudioClip>,
) -> (SpatialSink, Vec<u64>) {
    // Handles whose playback this play cuts short. Reported back so callers
    // (the audio log) can mark them stopped - these preemptions never go
    // through `stop_audio`.
    let preempted = prepare_audio_play(context, &handle, maybe_channel);

    //let reverb = source.buffered().reverb(Duration::from_millis(40), 0.7);
    // let x = rand::thread_rng().gen_range(-1.0..1.0);
    // let y = rand::thread_rng().gen_range(-1.0..1.0);
    // let z = rand::thread_rng().gen_range(-1.0..1.0);
    let scaled_x = position.x;
    let scaled_y = position.y;
    let scaled_z = position.z;
    let left_ear = context.last_left_ear_position;
    let right_ear = context.last_right_ear_position;
    let positions = (
        [scaled_x, scaled_y, scaled_z],
        [left_ear.x, left_ear.y, left_ear.z],
        [right_ear.x, right_ear.y, right_ear.z],
    );
    let sink = rodio::SpatialSink::try_new(&context.handle, positions.0, positions.1, positions.2)
        .unwrap();
    audio_clip.add_to_spatial_sink(&sink);

    //context.handle_to_sink.insert(handle.id, sink);
    (sink, preempted)

    //context.spatial_sinks.push(sink);
}

fn prepare_audio_play<TAmbientKey: Hash + Eq + Copy, TCue: Clone>(
    context: &mut AudioContext<TAmbientKey, TCue>,
    handle: &AudioHandle,
    maybe_channel: Option<AudioChannel>,
) -> Vec<u64> {
    let mut preempted = Vec::new();

    if let Some(channel) = maybe_channel {
        let maybe_previous_audio = context.channel_to_last_handle.get(&channel.name);
        if let Some(audio) = maybe_previous_audio {
            let previous_id = *audio;
            let maybe_sink = context.handle_to_sink.remove(&previous_id);
            context.handle_loops.remove(&previous_id);

            if let Some(sink) = maybe_sink {
                if !sink.empty() {
                    sink.stop();
                    preempted.push(previous_id);
                }
            }
        }

        context
            .channel_to_last_handle
            .insert(channel.name, handle.id);
    }

    context.handle_loops.remove(&handle.id);
    if let Some(current_channel) = context.handle_to_sink.get(&handle.id) {
        if !current_channel.empty() {
            current_channel.stop();
            preempted.push(handle.id);
        }
    }

    preempted
}

#[cfg(test)]
mod tests {
    use super::{
        AudioClip, AudioPlaybackSettings, AudioSource, StreamingAudioSource, TrackedEmitter,
        wav_duration,
    };
    use cgmath::vec3;

    #[test]
    fn environmental_bed_repeats_without_game_updates() {
        let clip = AudioClip::from_raw(1, 48_000, vec![8192; 64]);
        let (sink, mut output) = rodio::Sink::new_idle();
        clip.add_to_sink_looping(&sink);
        let samples: Vec<_> = output.by_ref().take(1024).collect();
        assert_eq!(samples.len(), 1024);
        assert!(samples.iter().all(|sample| *sample > 0.2));
        sink.stop();
    }

    /// Chunk boundaries must be inaudible: the source is one continuous
    /// soundtrack, whatever sizes the decoder happened to hand over.
    #[test]
    fn a_streaming_source_joins_its_chunks_seamlessly() {
        let (sender, receiver) = std::sync::mpsc::sync_channel(4);
        sender.send(vec![1, 2, 3]).unwrap();
        // An empty batch is skipped rather than mistaken for the end.
        sender.send(Vec::new()).unwrap();
        sender.send(vec![4]).unwrap();
        drop(sender);

        let source = StreamingAudioSource::new(1, 44100, receiver);
        assert_eq!(AudioSource::channels(&source), 1);
        assert_eq!(AudioSource::sample_rate(&source), 44100);
        assert_eq!(source.collect::<Vec<i16>>(), vec![1, 2, 3, 4]);
    }

    /// Playback ends when the producer goes away - that is how a decode that
    /// reached end-of-stream, or one cut short, finishes the sink.
    #[test]
    fn a_streaming_source_ends_when_its_producer_drops() {
        let (sender, receiver) = std::sync::mpsc::sync_channel::<Vec<i16>>(1);
        drop(sender);
        assert_eq!(StreamingAudioSource::new(1, 44100, receiver).next(), None);
    }

    fn riff(avg_bytes_per_sec: u32, data_len: usize) -> Vec<u8> {
        let mut fmt = Vec::new();
        fmt.extend_from_slice(&1u16.to_le_bytes()); // wFormatTag
        fmt.extend_from_slice(&1u16.to_le_bytes()); // nChannels
        fmt.extend_from_slice(&22050u32.to_le_bytes()); // nSamplesPerSec
        fmt.extend_from_slice(&avg_bytes_per_sec.to_le_bytes());
        fmt.extend_from_slice(&1u16.to_le_bytes()); // nBlockAlign
        fmt.extend_from_slice(&8u16.to_le_bytes()); // wBitsPerSample

        let mut body = Vec::new();
        body.extend_from_slice(b"WAVE");
        body.extend_from_slice(b"fmt ");
        body.extend_from_slice(&(fmt.len() as u32).to_le_bytes());
        body.extend_from_slice(&fmt);
        body.extend_from_slice(b"data");
        body.extend_from_slice(&(data_len as u32).to_le_bytes());
        body.extend_from_slice(&vec![0u8; data_len]);

        let mut out = Vec::new();
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&(body.len() as u32).to_le_bytes());
        out.extend_from_slice(&body);
        out
    }

    #[test]
    fn wav_duration_reads_avg_bytes_per_sec_and_data_size() {
        let duration = wav_duration(&riff(22050, 11025)).expect("duration");
        assert_eq!(duration.as_secs_f64(), 0.5);
    }

    #[test]
    fn wav_duration_rejects_non_riff_buffers() {
        assert!(wav_duration(b"not a wave file at all").is_none());
        assert!(wav_duration(&[]).is_none());
    }

    #[test]
    fn positional_emitter_refreshes_from_its_live_source() {
        let mut emitter = TrackedEmitter::new(vec3(1.0, 2.0, 3.0), Some(7_u32));

        emitter.refresh(|source| {
            assert_eq!(source, 7);
            Some(vec3(4.0, 5.0, 6.0))
        });

        assert_eq!(emitter.position(), vec3(4.0, 5.0, 6.0));
    }

    #[test]
    fn static_sink_receives_schema_gain_and_pan() {
        let clip = AudioClip::from_raw(1, 1, vec![i16::MAX / 2]);
        let (sink, mut output) = rodio::Sink::new_idle();
        clip.add_to_sink_with_settings(
            &sink,
            AudioPlaybackSettings {
                gain: 0.5,
                channel_gains: [1.0, 0.25],
                looping: false,
            },
        );

        assert_eq!(sink.volume(), 0.5);
        assert!((output.next().unwrap() - 0.25).abs() < 0.001);
        assert!((output.next().unwrap() - 0.0625).abs() < 0.001);
    }
}
