use std::collections::{HashMap, HashSet};
use std::hash::Hash;
use std::io::Cursor;
use std::rc::Rc;

use cgmath::{vec3, Vector3};
use rodio::buffer::SamplesBuffer;
use rodio::source::{Buffered, Source};
use rodio::{Decoder, OutputStream, OutputStreamHandle, Sink, SpatialSink};

use tracing::trace;
use crate::audio_log;

use std::sync::atomic::{AtomicU64, Ordering};

#[cfg(target_os = "android")]
const BASE_PATH: &str = "/mnt/sdcard/shock2quest";

#[cfg(not(target_os = "android"))]
#[allow(dead_code)]
const BASE_PATH: &str = "../../Data";

static NEXT_HANDLE_ID: AtomicU64 = AtomicU64::new(0);

const SOUND_SCALE_FACTOR: f32 = 5.0;

#[derive(Clone, Debug)]
pub struct AudioHandle {
    id: u64,
}

impl AudioHandle {
    pub fn new() -> AudioHandle {
        let id = NEXT_HANDLE_ID.fetch_add(1, Ordering::SeqCst);
        AudioHandle { id }
    }
}

pub struct AudioChannel {
    name: String,
}

impl AudioChannel {
    pub fn new(name: String) -> AudioChannel {
        AudioChannel { name }
    }
}

pub trait BackgroundMusic<TCue> {
    fn next_clip(&mut self, cue: Option<TCue>) -> Option<Rc<AudioClip>>;
}

pub enum SinkAdapter {
    StaticSink(SpatialSink),
    PositionalSink(SpatialSink),
}

impl SinkAdapter {
    pub fn inner(&self) -> &SpatialSink {
        match self {
            SinkAdapter::StaticSink(sink) => sink,
            SinkAdapter::PositionalSink(sink) => sink,
        }
    }

    pub fn fixed(sink: SpatialSink) -> SinkAdapter {
        SinkAdapter::StaticSink(sink)
    }

    pub fn positional(sink: SpatialSink) -> SinkAdapter {
        SinkAdapter::PositionalSink(sink)
    }

    pub fn update_listener_position(
        &mut self,
        left_ear_position: [f32; 3],
        right_ear_position: [f32; 3],
    ) {
        match self {
            SinkAdapter::StaticSink(_) => (),
            SinkAdapter::PositionalSink(sink) => {
                sink.set_left_ear_position(left_ear_position);
                sink.set_right_ear_position(right_ear_position);
            }
        }
    }

    pub fn empty(&self) -> bool {
        self.inner().empty()
    }

    pub fn stop(&self) {
        self.inner().stop();
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
    handle_to_sink: HashMap<u64, SinkAdapter>,
    // Background music
    background_music: Option<Sink>,
    background_music_player: Option<Box<dyn BackgroundMusic<TCue>>>,
    next_music_cue: Option<TCue>,

    // Environmental sounds
    environmental_sink: Option<(Sink, Rc<AudioClip>)>,

    // Position audio context
    last_left_ear_position: Vector3<f32>,
    last_right_ear_position: Vector3<f32>,

    // Ambient, positional sounds
    ambient_sounds: HashMap<TAmbientKey, (SpatialSink, Rc<AudioClip>)>,
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
    ) -> () {
        self.background_music_player = Some(background_music_player);
        self.next_music_cue = None;
    }

    pub fn stop_background_music(&mut self) -> () {
        self.background_music_player = None;
        self.next_music_cue = None;
    }

    pub fn set_background_music_cue(&mut self, cue: TCue) -> () {
        self.next_music_cue = Some(cue)
    }

    pub fn set_environmental_sound(&mut self, clip: Rc<AudioClip>) -> () {
        let sink = rodio::Sink::try_new(&self.handle).unwrap();
        clip.add_to_sink(&sink);
        sink.set_volume(0.2);
        sink.play();
        self.environmental_sink = Some((sink, clip.clone()));
    }

    pub fn update(
        &mut self,
        position: Vector3<f32>,
        current_ambient_sounds: Vec<(TAmbientKey, Vector3<f32>, Rc<AudioClip>)>,
    ) {
        audio_log!(DEBUG, "Audio system update started");
        self.update_background_music();
        self.update_environmental_sounds();

        trace!(
            "updating {} ambient sounds...",
            current_ambient_sounds.len()
        );

        let left_ear_position = [
            (position.x - 1.0) / SOUND_SCALE_FACTOR,
            position.y / SOUND_SCALE_FACTOR,
            position.z / SOUND_SCALE_FACTOR,
        ];
        let right_ear_position = [
            (position.x + 1.0) / SOUND_SCALE_FACTOR,
            position.y / SOUND_SCALE_FACTOR,
            position.z / SOUND_SCALE_FACTOR,
        ];

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
        // Update positional sounds
        for (_, sink) in &mut self.handle_to_sink {
            sink.update_listener_position(left_ear_position, right_ear_position);
        }

        // Build hash map for new ambient sounds
        let mut current_sound_hash = HashMap::new();
        for (key, pos, clip) in &current_ambient_sounds {
            current_sound_hash.insert(key, (pos, clip));
        }

        let mut sounds_to_remove = HashSet::new();
        // First pass - check existing ambient sounds, update position, and see if they have completed
        for (key, (sink, clip)) in &self.ambient_sounds {
            if let Some(current_sound) = current_sound_hash.get(key) {
                if sink.len() == 0 {
                    clip.add_to_spatial_sink(&sink);
                }

                sink.set_emitter_position([
                    current_sound.0.x / SOUND_SCALE_FACTOR,
                    current_sound.0.y / SOUND_SCALE_FACTOR,
                    current_sound.0.z / SOUND_SCALE_FACTOR,
                ]);

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
                    [
                        pos.x / SOUND_SCALE_FACTOR,
                        pos.y / SOUND_SCALE_FACTOR,
                        pos.z / SOUND_SCALE_FACTOR,
                    ],
                    left_ear_position,
                    right_ear_position,
                )
                .unwrap();

                self.ambient_sounds.insert(*key, (sink, clip.clone()));
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

    fn update_environmental_sounds(&mut self) {
        if let Some((current_sink, clip)) = &self.environmental_sink {
            if current_sink.len() == 0 {
                let sink = rodio::Sink::try_new(&self.handle).unwrap();
                clip.add_to_sink(&sink);
                sink.set_volume(0.2);
                sink.play();
                self.environmental_sink = Some((sink, clip.clone()));
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
}

impl AudioClip {
    pub fn add_to_spatial_sink(&self, sink: &SpatialSink) -> () {
        match &self.source {
            SourceType::Bytes(source) => sink.append(source.clone()),
            SourceType::Raw(source) => sink.append(source.clone()),
        }
    }
    pub fn add_to_sink(&self, sink: &Sink) -> () {
        match &self.source {
            SourceType::Bytes(source) => sink.append(source.clone()),
            SourceType::Raw(source) => sink.append(source.clone()),
        }
    }
    pub fn from_bytes(bytes: Vec<u8>) -> AudioClip {
        let buf = Cursor::new(bytes);
        let source = rodio::Decoder::new(buf).unwrap().buffered();
        AudioClip {
            source: SourceType::Bytes(source),
        }
    }

    pub fn from_raw(channels: u16, sample_rate: u32, data: Vec<i16>) -> AudioClip {
        let source = rodio::buffer::SamplesBuffer::new(channels, sample_rate, data).buffered();
        AudioClip {
            source: SourceType::Raw(source),
        }
    }
}

pub fn stop_audio<TAmbientKey: Hash + Eq + Copy, TCue: Clone>(
    context: &mut AudioContext<TAmbientKey, TCue>,
    handle: AudioHandle,
) -> () {
    let maybe_sink = context.handle_to_sink.remove(&handle.id);

    if let Some(sink) = maybe_sink {
        sink.stop();
    }
}

pub fn test_audio<TAmbientKey: Hash + Eq + Copy, TCue: Clone>(
    context: &mut AudioContext<TAmbientKey, TCue>,
    handle: AudioHandle,
    maybe_channel: Option<AudioChannel>,
    audio_clip: Rc<AudioClip>,
) {
    let position = (context.last_left_ear_position + context.last_right_ear_position) / 2.0;

    let id = handle.id.clone();
    let sink = play_audio_core(context, position, handle, maybe_channel, audio_clip);

    context.handle_to_sink.insert(id, SinkAdapter::fixed(sink));
}

pub fn play_spatial_audio<TAmbientKey: Hash + Eq + Copy, TCue: Clone>(
    context: &mut AudioContext<TAmbientKey, TCue>,
    position: Vector3<f32>,
    handle: AudioHandle,
    maybe_channel: Option<AudioChannel>,
    audio_clip: Rc<AudioClip>,
) {
    let id = handle.id.clone();
    let scaled_position = position / SOUND_SCALE_FACTOR;
    let sink = play_audio_core(context, scaled_position, handle, maybe_channel, audio_clip);

    context
        .handle_to_sink
        .insert(id, SinkAdapter::positional(sink));
}

pub fn play_audio_core<TAmbientKey: Hash + Eq + Copy, TCue: Clone>(
    context: &mut AudioContext<TAmbientKey, TCue>,
    position: Vector3<f32>,
    handle: AudioHandle,
    maybe_channel: Option<AudioChannel>,
    audio_clip: Rc<AudioClip>,
) -> SpatialSink {
    if let Some(channel) = maybe_channel {
        let maybe_previous_audio = context.channel_to_last_handle.get(&channel.name);
        if let Some(audio) = maybe_previous_audio {
            let maybe_sink = context.handle_to_sink.remove(audio);

            if let Some(sink) = maybe_sink {
                if !sink.empty() {
                    sink.stop();
                }
            }
        }

        context
            .channel_to_last_handle
            .insert(channel.name, handle.id);
    }

    if let Some(current_channel) = context.handle_to_sink.get(&handle.id) {
        if !current_channel.empty() {
            current_channel.stop();
        }
    }

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
    sink

    //context.spatial_sinks.push(sink);
}
