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
        // Ambient markers select a theme, not a one-shot event. The original
        // themed player resends it at every section boundary (SetTheme /
        // _DoSegmentCallback), including sections that did not handle it yet.
        let cue = cue.map(|cue| cue.to_ascii_lowercase());
        if let Some(theme) = cue.as_ref().filter(|cue| cue.starts_with("theme ")) {
            self.default_cue = Some(theme.clone());
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn player() -> SongPlayer {
        // Quiet is unhandled in the first section, handled in the second.
        let mut bytes = vec![0; 40];
        bytes.extend(2u32.to_le_bytes());
        for events in [vec!["", "theme begin"], vec!["", "theme quiet", "combat"]] {
            bytes.extend([0; 44]); // section name + metadata
            let mut wav = [0; 32];
            wav[..8].copy_from_slice(b"test.wav");
            bytes.extend(wav);
            bytes.extend((events.len() as u32).to_le_bytes());
            for event in events {
                let mut name = [0; 36];
                name[..event.len()].copy_from_slice(event.as_bytes());
                bytes.extend(name);
                bytes.extend(1u32.to_le_bytes()); // one branch
                bytes.extend(1u32.to_le_bytes()); // target section
                bytes.extend(100u32.to_le_bytes());
            }
        }
        let song = Song::read(&mut Cursor::new(bytes));
        SongPlayer {
            play_state: song.start_playing(),
            song,
            name_to_clip: HashMap::from([(
                "test.wav".into(),
                Rc::new(AudioClip::from_raw(1, 8000, vec![0; 80])),
            )]),
            default_cue: None,
            status: Rc::new(RefCell::new(SongPlaybackStatus::default())),
        }
    }

    #[test]
    fn theme_survives_unhandled_sections_and_one_shot_events() {
        let mut player = player().with_default_cue(Some("theme begin".into()));
        for (cue, expected_event, option) in [
            (Some("THEME QUIET"), "theme quiet", 0),
            (None, "theme quiet", 1),
            (Some("combat"), "combat", 2),
            (None, "theme quiet", 1),
            (Some("theme begin"), "theme begin", 0),
            (None, "theme begin", 0),
        ] {
            assert!(player.next_clip(cue.map(str::to_owned)).is_some());
            let status = player.status.borrow();
            let transition = status.history.back().unwrap();
            assert_eq!(transition.cue.as_deref(), Some(expected_event));
            assert_eq!(transition.option_index, option);
        }
    }

    #[test]
    fn replacement_player_does_not_inherit_the_previous_theme() {
        let mut old = player();
        old.next_clip(Some("theme quiet".into())).unwrap();
        let mut replacement = player();
        replacement.next_clip(None).unwrap();
        assert_eq!(
            replacement.status.borrow().history.back().unwrap().cue,
            None
        );
    }
}
