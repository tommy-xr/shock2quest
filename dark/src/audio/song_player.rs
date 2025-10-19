use std::{collections::HashMap, rc::Rc};

use engine::{
    assets::asset_cache::AssetCache,
    audio::{AudioClip, BackgroundMusic},
};
use tracing::info;

use crate::importers::AUDIO_IMPORTER;

use super::{Song, SongPlayContext};

pub struct SongPlayer {
    song: Song,
    name_to_clip: HashMap<String, Rc<AudioClip>>,
    play_state: SongPlayContext,
}

impl SongPlayer {
    pub fn new(song: &Song, asset_cache: &mut AssetCache) -> SongPlayer {
        let mut name_to_clip = HashMap::new();
        let wav_files = song.all_wav_files();

        for file in &wav_files {
            let audio = asset_cache.get(&AUDIO_IMPORTER, file);
            name_to_clip.insert(file.to_ascii_lowercase(), audio);
        }

        let all_schemas = song.all_schemas();

        info!(
            "starting song - wav files: {} schemas: {}",
            wav_files.join(","),
            all_schemas.join(","),
        );
        //panic!("");

        let my_song = song.clone();

        let play_state = my_song.start_playing();

        SongPlayer {
            song: my_song,
            name_to_clip,
            play_state,
        }
    }
}

impl BackgroundMusic<String> for SongPlayer {
    fn next_clip(&mut self, cue: Option<String>) -> Option<Rc<engine::audio::AudioClip>> {
        let (next_state, clip_name) = self.song.play_next(self.play_state.clone(), cue.clone());
        self.play_state = next_state.clone();

        let maybe_audio_clip = self
            .name_to_clip
            .get(&clip_name.to_ascii_lowercase())
            .cloned();
        info!(
            "searching for next clip - used cue {:?}, next clip is {:?}",
            cue, next_state
        );
        maybe_audio_clip
    }
}
