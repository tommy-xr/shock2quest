use std::{
    cell::RefCell,
    collections::{HashMap, VecDeque},
    rc::Rc,
    time::{Duration, Instant},
};

use super::{Song, SongPlayContext, SongTransition};
use crate::importers::AUDIO_IMPORTER;
use engine::{
    assets::asset_cache::AssetCache,
    audio::{AudioClip, BackgroundMusic},
};

/// Bounded playback evidence, shared with tools without duplicating decisions.
#[derive(Default)]
pub struct SongPlaybackStatus {
    pub history: VecDeque<SongTransition>,
    pub clips_played: u64,
    pub started_at: Option<Instant>,
    pub duration: Option<Duration>,
    pub error: Option<String>,
}

pub struct SongPlayer {
    song: Song,
    name_to_clip: HashMap<String, Rc<AudioClip>>,
    play_state: SongPlayContext,
    default_cue: Option<String>,
    status: Rc<RefCell<SongPlaybackStatus>>,
}

impl SongPlayer {
    pub fn new(song: &Song, asset_cache: &mut AssetCache) -> SongPlayer {
        let mut name_to_clip = HashMap::new();
        for file in song.all_wav_files() {
            let key = file.to_ascii_lowercase();
            if !name_to_clip.contains_key(&key) {
                if let Some(audio) = asset_cache.get_opt(&AUDIO_IMPORTER, &file) {
                    name_to_clip.insert(key, audio);
                }
            }
        }
        SongPlayer {
            song: song.clone(),
            name_to_clip,
            play_state: song.start_playing(),
            default_cue: None,
            status: Rc::new(RefCell::new(SongPlaybackStatus::default())),
        }
    }

    /// Used by standalone modes without spatial music-event emitters.
    pub fn with_default_cue(mut self, cue: Option<String>) -> Self {
        self.default_cue = cue;
        self
    }

    pub fn status(&self) -> Rc<RefCell<SongPlaybackStatus>> {
        self.status.clone()
    }
}

impl BackgroundMusic<String> for SongPlayer {
    fn next_clip(&mut self, cue: Option<String>) -> Option<Rc<AudioClip>> {
        let mut status = self.status.borrow_mut();
        // A broken asset should report once and stay stopped, not retry/log on
        // every rendered frame. Restarting creates a fresh player.
        if status.error.is_some() {
            return None;
        }
        let cue = cue.or_else(|| self.default_cue.clone());
        let transition =
            match self
                .song
                .transition(&self.play_state, cue.as_deref(), &mut rand::thread_rng())
            {
                Ok(transition) => transition,
                Err(error) => {
                    status.error = Some(error);
                    return None;
                }
            };
        let clip_name = &self.song.sections()[transition.to].wav_file;
        let Some(clip) = self
            .name_to_clip
            .get(&clip_name.to_ascii_lowercase())
            .cloned()
        else {
            status.error = Some(format!("Missing WAV: {clip_name}"));
            return None;
        };
        tracing::info!(?transition, wav = clip_name, "song transition");
        self.play_state.current_section = transition.to;
        status.clips_played += 1;
        status.started_at = Some(Instant::now());
        status.duration = clip.total_duration();
        if status.history.len() == 32 {
            status.history.pop_front();
        }
        status.history.push_back(transition);
        Some(clip)
    }
}
