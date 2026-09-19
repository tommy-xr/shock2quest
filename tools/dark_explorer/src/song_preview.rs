//! Audition authored songs through the game's player; branch highlights are
//! observations from that player, never a second random roll in the UI.
use crate::{
    explorer,
    ui::{play_wav, quiet_catch},
};
use dark::audio::{Song, SongPlaybackStatus, SongPlayer};
use eframe::egui;
use engine::{
    assets::{asset_cache::AssetCache, asset_paths::AssetPath},
    audio::{AudioContext, AudioHandle},
};
use std::{cell::RefCell, rc::Rc, time::Duration};

pub struct SongPreview {
    song: Song,
    audio: Option<AudioContext<(), String>>,
    assets: Option<AssetCache>,
    status: Rc<RefCell<SongPlaybackStatus>>,
    playing: bool,
    pending: Option<String>,
    observed_clips: u64,
    audition: Option<AudioHandle>,
    error: Option<String>,
}

impl SongPreview {
    pub fn new(song: Song) -> Self {
        Self {
            song,
            audio: None,
            assets: None,
            status: Rc::default(),
            playing: false,
            pending: None,
            observed_clips: 0,
            audition: None,
            error: None,
        }
    }

    fn stop(&mut self) {
        if let Some(audio) = &mut self.audio {
            audio.stop_background_music();
            if let Some(handle) = self.audition.take() {
                engine::audio::stop_audio(audio, handle);
            }
        }
        self.playing = false;
        self.pending = None;
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub fn play(&mut self) {
        self.stop();
        self.error = None;
        let result = quiet_catch(|| {
            let assets = self.assets.get_or_insert_with(|| {
                AssetCache::new(
                    shock2vr::paths::data_root().to_string_lossy().into_owned(),
                    AssetPath::combine(vec![
                        explorer::family_mounts("snd"),
                        explorer::family_mounts("song"),
                    ]),
                )
            });
            let player = SongPlayer::new(&self.song, assets);
            self.status = player.status();
            self.observed_clips = 0;
            let audio = self.audio.get_or_insert_with(AudioContext::new);
            audio.set_background_music(Box::new(player));
            self.pending = self.song.start_event();
            if let Some(event) = &self.pending {
                audio.set_background_music_cue(event.clone());
            }
            self.playing = true;
        });
        if let Err(error) = result {
            self.error = Some(error);
            self.stop();
        }
    }

    pub fn show(&mut self, ui: &mut egui::Ui) {
        if let Some(audio) = &mut self.audio {
            audio.update(
                cgmath::vec3(-0.125, 0.0, 0.0),
                cgmath::vec3(0.125, 0.0, 0.0),
                vec![],
                |_| None,
            );
        }
        {
            let status = self.status.borrow();
            if status.clips_played != self.observed_clips {
                self.observed_clips = status.clips_played;
                self.pending = None;
            }
            if let Some(error) = &status.error {
                self.error = Some(error.clone());
                self.playing = false;
            }
        }
        ui.horizontal(|ui| {
            if ui
                .button(if self.playing { "Restart" } else { "Play song" })
                .clicked()
            {
                self.play();
            }
            if ui.button("Stop").clicked() {
                self.stop();
            }
            ui.label(format!("{} sections", self.song.sections().len()));
        });
        ui.label("Events take effect at the next WAV boundary; the latest event replaces the pending one.");
        if let Some(event) = self.song.start_event() {
            ui.small(format!("Play starts with {event}. Send it again to restart a theme that returns to silence."));
        }
        let mut events = self.song.all_schemas();
        events.retain(|e| !e.is_empty());
        events.sort();
        events.dedup();
        ui.horizontal_wrapped(|ui| {
            for event in &events {
                if ui
                    .add_enabled(self.playing, egui::Button::new(event))
                    .clicked()
                {
                    self.audio
                        .as_mut()
                        .unwrap()
                        .set_background_music_cue(event.clone());
                    self.pending = Some(event.clone());
                }
            }
        });
        if let Some(event) = &self.pending {
            ui.colored_label(egui::Color32::YELLOW, format!("Pending: {event}"));
        }
        if let Some(error) = &self.error {
            ui.colored_label(egui::Color32::RED, error);
        }
        let status = self.status.borrow();
        let last = status.history.back();
        if let Some(last) = last {
            let section = &self.song.sections()[last.to];
            ui.heading(format!(
                "{}: {}",
                if self.playing {
                    "Playing"
                } else {
                    "Last played"
                },
                section.wav_file
            ));
            if self.playing {
                if let (Some(start), Some(duration)) = (status.started_at, status.duration) {
                    let seconds = start.elapsed().as_secs_f32().min(duration.as_secs_f32());
                    ui.add(
                        egui::ProgressBar::new(seconds / duration.as_secs_f32().max(0.001))
                            .text(format!("{seconds:.1} / {:.1} s", duration.as_secs_f32())),
                    );
                }
            }
        }
        ui.collapsing(
            format!("Recent transitions ({})", status.clips_played),
            |ui| {
                for transition in status.history.iter().rev() {
                    let from = &self.song.sections()[transition.from];
                    let to = &self.song.sections()[transition.to];
                    let option = &from.options[transition.option_index];
                    let branch = &option.sub_options[transition.branch_index];
                    let total: u64 = option
                        .sub_options
                        .iter()
                        .map(|s| u64::from(s.probability))
                        .sum();
                    ui.label(format!(
                        "{} -> {} · {} · weight {}/{} · requested {}",
                        from.name,
                        to.name,
                        if option.schema.is_empty() {
                            "default"
                        } else {
                            &option.schema
                        },
                        branch.probability,
                        total,
                        transition.cue.as_deref().unwrap_or("none")
                    ));
                }
            },
        );
        ui.separator();
        let mut audition_file = None;
        egui::ScrollArea::vertical()
            .id_salt("song_sections")
            .show(ui, |ui| {
                for (index, section) in self.song.sections().iter().enumerate() {
                    let active = self.playing && last.is_some_and(|t| t.to == index);
                    let color = if active {
                        egui::Color32::LIGHT_GREEN
                    } else {
                        ui.visuals().text_color()
                    };
                    ui.horizontal(|ui| {
                        ui.colored_label(
                            color,
                            format!(
                                "{index}: {} · {}{}",
                                section.name,
                                section.wav_file,
                                if active { "  PLAYING" } else { "" }
                            ),
                        );
                        if ui.small_button("Audition WAV").clicked() {
                            audition_file = Some(section.wav_file.clone());
                        }
                    });
                    for (option_index, option) in section.options.iter().enumerate() {
                        ui.indent((index, option_index), |ui| {
                            ui.label(if option.schema.is_empty() {
                                "Default / unmatched event"
                            } else {
                                &option.schema
                            });
                            let total: u64 = option
                                .sub_options
                                .iter()
                                .map(|s| u64::from(s.probability))
                                .sum();
                            for (branch_index, branch) in option.sub_options.iter().enumerate() {
                                let taken = last.is_some_and(|t| {
                                    t.from == index
                                        && t.option_index == option_index
                                        && t.branch_index == branch_index
                                });
                                let target = self.song.sections().get(branch.next_index as usize);
                                let label = format!(
                                    "{} -> {}: {} · {:.1}% (weight {})",
                                    if taken { "TAKEN" } else { "" },
                                    branch.next_index,
                                    target
                                        .map(|s| s.wav_file.as_str())
                                        .unwrap_or("MISSING SECTION"),
                                    100.0 * f64::from(branch.probability) / total.max(1) as f64,
                                    branch.probability
                                );
                                ui.colored_label(
                                    if taken {
                                        egui::Color32::LIGHT_BLUE
                                    } else {
                                        ui.visuals().weak_text_color()
                                    },
                                    label,
                                );
                            }
                        });
                    }
                    ui.separator();
                }
            });
        drop(status);
        if let Some(file) = audition_file {
            self.stop();
            let mounts = explorer::family_mounts("snd");
            match explorer::read_asset_bytes(&*mounts, &file) {
                Some(bytes) => {
                    play_wav(&mut self.audio, &mut self.audition, &mut self.error, bytes)
                }
                None => self.error = Some(format!("Missing WAV: {file}")),
            }
        }
        if self.playing {
            ui.ctx().request_repaint_after(Duration::from_millis(30));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine::audio::BackgroundMusic;

    /// Exercises the mounted remaster/classic data through the same importers
    /// as playback. Explicitly opt in: licensed game assets are not in CI.
    #[test]
    #[ignore = "requires a System Shock 2 data installation"]
    fn mounted_songs_resolve_their_wavs_and_report_played_branches() {
        let mounts = explorer::family_mounts("song");
        let mut assets = AssetCache::new(
            shock2vr::paths::data_root().to_string_lossy().into_owned(),
            AssetPath::combine(vec![
                explorer::family_mounts("snd"),
                explorer::family_mounts("song"),
            ]),
        );
        let mut keys = std::collections::BTreeSet::new();
        for entry in mounts.entries() {
            if entry.key.ends_with(".snc") {
                keys.insert(entry.key);
            }
        }
        assert!(!keys.is_empty(), "no installed songs");
        for key in keys {
            let song = assets.get(&dark::importers::SONG_IMPORTER, &key);
            for section in song.sections() {
                let clip = assets
                    .get_opt(&dark::importers::AUDIO_IMPORTER, &section.wav_file)
                    .unwrap_or_else(|| panic!("{key}: missing {}", section.wav_file));
                assert!(clip.total_duration().is_some(), "{}", section.wav_file);
                assert!(!section.options.is_empty(), "{key}: {}", section.name);
                for option in &section.options {
                    assert!(
                        option.sub_options.iter().any(|o| o.probability > 0),
                        "{key}"
                    );
                    assert!(
                        option
                            .sub_options
                            .iter()
                            .all(|o| (o.next_index as usize) < song.sections().len()),
                        "{key}"
                    );
                }
            }
            let mut player =
                SongPlayer::new(&song, &mut assets).with_default_cue(song.start_event());
            let status = player.status();
            for _ in 0..40 {
                let clip = player
                    .next_clip(None)
                    .unwrap_or_else(|| panic!("{key}: {:?}", status.borrow().error));
                let status = status.borrow();
                let transition = status.history.back().unwrap();
                let expected = assets.get(
                    &dark::importers::AUDIO_IMPORTER,
                    &song.sections()[transition.to].wav_file,
                );
                assert!(Rc::ptr_eq(&clip, &expected));
                assert_eq!(status.duration, clip.total_duration());
            }
            assert_eq!(status.borrow().history.len(), 32);
            assert_eq!(status.borrow().clips_played, 40);
        }
    }
}
