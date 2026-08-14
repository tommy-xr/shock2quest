//! Frontend (menu) sound effects, shared by every 2D screen.
//!
//! The original frontend is not silent: `res/snd/sfx/` ships the whole set the
//! menus use - a looping bed (`mloop1`/`mloop2`), a rollover blip played when
//! the cursor moves onto a widget (`MROLLOV1`/`MROLLOV2`), and a select click
//! (`MSELECT1`/`MSELECT2`). Only the first of each pair is used here; the
//! second is the same sound rerecorded and nothing in the shipped data says
//! which screen takes which.
//!
//! A screen drives this from its own update/effect pass:
//!
//! - `hover(id)` once per update with whatever widget the pointer is over,
//! - `click()` when a press activates one,
//! - `pump(..)` from `handle_effects`, which is where a scene is handed the
//!   [`AudioContext`],
//! - `stop(..)` from `GameScene::on_exit`, so the bed ends with the screen
//!   rather than playing on under the mission behind it. It has to be the exit
//!   hook and not "the frame we emitted a scene-changing effect": scenes are
//!   also replaced from paths that never reach `handle_effects`, and not every
//!   global effect actually replaces the scene (a failed load leaves it up).
//!
//! Plays are mirrored into [`crate::audio_log`] so a headless test can assert
//! the menu made noise (`GET /v1/audio/recent`) - there is no other way to
//! observe audio without speakers.

use engine::{
    assets::asset_cache::AssetCache,
    audio::{AudioContext, AudioHandle, PlayOptions, play_audio_with, stop_audio},
};
use shipyard::EntityId;
use tracing::warn;

use dark::importers::AUDIO_IMPORTER;

/// Looping bed under every frontend screen.
const HUM_SOUND: &str = "sfx/mloop1.wav";
/// Blip when the pointer moves onto a widget.
const ROLLOVER_SOUND: &str = "sfx/mrollov1.wav";
/// Click when a widget is activated.
const SELECT_SOUND: &str = "sfx/mselect1.wav";

/// The bed sits under the interaction sounds rather than competing with them.
const HUM_VOLUME: f32 = 0.35;
/// Rollover fires on every widget the cursor crosses, so it is the quietest of
/// the three.
const ROLLOVER_VOLUME: f32 = 0.5;
const SELECT_VOLUME: f32 = 0.8;

/// Identifies the widget under the pointer. Only equality matters - a screen
/// numbers its own widgets however it likes, as long as two different widgets
/// never share an id.
pub type WidgetId = usize;

#[derive(Default)]
pub struct FrontendSfx {
    /// Widget the pointer was over on the previous update, so the rollover
    /// fires once on entry rather than every frame it stays there.
    hovered: Option<WidgetId>,
    /// One-shots queued by `update`, played by the next `pump`.
    pending: Vec<&'static str>,
    /// The looping bed.
    hum: Hum,
}

/// The bed's lifecycle. `Unavailable` is a latch: without it a missing
/// `snd.crf` would re-hit the asset cache and warn once per frame, forever.
#[derive(Default)]
enum Hum {
    #[default]
    NotStarted,
    Playing(AudioHandle),
    Unavailable,
}

impl FrontendSfx {
    pub fn new() -> Self {
        Self::default()
    }

    /// The widget the pointer is over this update, or `None` when it is over
    /// nothing actionable.
    pub fn hover(&mut self, target: Option<WidgetId>) {
        if target == self.hovered {
            return;
        }
        self.hovered = target;
        if target.is_some() {
            self.pending.push(ROLLOVER_SOUND);
        }
    }

    /// A widget was activated.
    pub fn click(&mut self) {
        self.pending.push(SELECT_SOUND);
    }

    /// Start the bed if it is not running, and play anything queued since the
    /// last call.
    pub fn pump(
        &mut self,
        asset_cache: &mut AssetCache,
        audio_context: &mut AudioContext<EntityId, String>,
    ) {
        if matches!(self.hum, Hum::NotStarted) {
            let handle = AudioHandle::new();
            self.hum = if play(
                HUM_SOUND,
                &handle,
                PlayOptions {
                    volume: HUM_VOLUME,
                    looping: true,
                },
                asset_cache,
                audio_context,
            ) {
                Hum::Playing(handle)
            } else {
                Hum::Unavailable
            };
        }

        for sound in std::mem::take(&mut self.pending) {
            let volume = if sound == ROLLOVER_SOUND {
                ROLLOVER_VOLUME
            } else {
                SELECT_VOLUME
            };
            play(
                sound,
                &AudioHandle::new(),
                PlayOptions {
                    volume,
                    looping: false,
                },
                asset_cache,
                audio_context,
            );
        }
    }

    /// Stop the bed. The screen's `GameScene::on_exit` calls this - a looping
    /// sink never drains on its own, so nothing else would ever end it.
    ///
    /// One-shots are deliberately left alone: the select click that caused the
    /// transition should finish over the screen that replaces this one.
    pub fn stop(&mut self, audio_context: &mut AudioContext<EntityId, String>) {
        if let Hum::Playing(handle) = std::mem::take(&mut self.hum) {
            stop_audio(audio_context, handle.clone());
            crate::audio_log::record_stop(handle.id());
        }
    }
}

/// Play one frontend sound by asset name, reporting whether it was found.
///
/// The sounds live in `snd.crf` under `sfx/`, so they are addressed by their
/// archive-relative path: the bare basenames are ambiguous across the mounted
/// archives, and a menu that silently played some other `mloop1` would be
/// worse than a silent one.
fn play(
    name: &'static str,
    handle: &AudioHandle,
    options: PlayOptions,
    asset_cache: &mut AssetCache,
    audio_context: &mut AudioContext<EntityId, String>,
) -> bool {
    let Some(clip) = asset_cache.get_opt(&AUDIO_IMPORTER, name) else {
        warn!("frontend sfx: unable to load {name}");
        return false;
    };
    let duration = clip.total_duration();
    let preempted = play_audio_with(audio_context, handle.clone(), None, clip, options);
    crate::audio_log::record_stops(&preempted);
    crate::audio_log::record(crate::audio_log::SoundRecord {
        sample: name,
        tags: vec![("kind".to_owned(), "menu".to_owned())],
        position: [0.0, 0.0, 0.0],
        duration,
        source_entity: None,
        handle: Some(handle.id()),
    });
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rollover_fires_once_per_widget_entry() {
        let mut sfx = FrontendSfx::new();

        sfx.hover(Some(2));
        assert_eq!(sfx.pending, vec![ROLLOVER_SOUND]);

        // Staying on the same widget is not a new entry.
        sfx.hover(Some(2));
        assert_eq!(sfx.pending, vec![ROLLOVER_SOUND]);

        // Moving straight from one widget to another is.
        sfx.hover(Some(3));
        assert_eq!(sfx.pending, vec![ROLLOVER_SOUND, ROLLOVER_SOUND]);
    }

    #[test]
    fn leaving_a_widget_is_silent() {
        let mut sfx = FrontendSfx::new();
        sfx.hover(Some(1));
        sfx.pending.clear();

        sfx.hover(None);
        assert!(sfx.pending.is_empty());
    }

    #[test]
    fn re_entering_the_same_widget_fires_again() {
        let mut sfx = FrontendSfx::new();
        sfx.hover(Some(1));
        sfx.hover(None);
        sfx.pending.clear();

        sfx.hover(Some(1));
        assert_eq!(sfx.pending, vec![ROLLOVER_SOUND]);
    }

    #[test]
    fn click_queues_the_select_sound() {
        let mut sfx = FrontendSfx::new();
        sfx.click();
        assert_eq!(sfx.pending, vec![SELECT_SOUND]);
    }
}
