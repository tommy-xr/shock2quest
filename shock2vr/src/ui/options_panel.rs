//! Retail Options canvas, shared by the main menu and the paused mission.
use super::{HAlign, Rect, UiCanvas, VAlign, resolve_menu_label, resolve_menu_rects};
use crate::user_settings::{self, TurnMode, UserSettings};
use cgmath::{Vector2, vec2};
use dark::importers::{STRINGS_IMPORTER, UI_LAYOUT_IMPORTER};
use engine::assets::asset_cache::AssetCache;

const TABS: [Rect; 4] = [
    Rect::new(4.0, 8.0, 152.0, 56.0),
    Rect::new(164.0, 8.0, 152.0, 56.0),
    Rect::new(324.0, 8.0, 152.0, 56.0),
    Rect::new(484.0, 8.0, 151.0, 56.0),
];
const FRAME: [Rect; 2] = [
    Rect::new(244.0, 408.0, 152.0, 56.0),
    Rect::new(20.0, 67.0, 601.0, 335.0),
];
// Retail audio's five full-width rows are also suitable for comfort controls.
const ROWS: [Rect; 5] = [
    Rect::new(93.0, 73.0, 481.0, 23.0),
    Rect::new(93.0, 102.0, 481.0, 23.0),
    Rect::new(93.0, 130.0, 481.0, 23.0),
    Rect::new(93.0, 158.0, 481.0, 23.0),
    Rect::new(93.0, 187.0, 481.0, 23.0),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OptionsEvent {
    Tab(usize),
    Row(usize),
    Done,
}

pub struct OptionsLayout {
    pub tabs: Vec<Rect>,
    pub frame: Vec<Rect>,
    pub rows: Vec<Rect>,
}

impl Default for OptionsLayout {
    fn default() -> Self {
        Self {
            tabs: TABS.to_vec(),
            frame: FRAME.to_vec(),
            rows: ROWS.to_vec(),
        }
    }
}

impl OptionsLayout {
    pub fn load(assets: &mut AssetCache) -> Self {
        let mut read = |name, fallback: &[Rect]| {
            let layout = assets.get_opt(&UI_LAYOUT_IMPORTER, name);
            resolve_menu_rects(layout.as_deref().map(|r| r.as_slice()), fallback)
        };
        Self {
            tabs: read("OPTIONTR.BIN", &TABS),
            frame: read("OPTIONSR.BIN", &FRAME),
            rows: read("OPTIONAR.BIN", &ROWS),
        }
    }
}

#[derive(Default)]
pub struct OptionsPanel {
    tab: usize,
    error: bool,
}

impl OptionsPanel {
    pub fn hit(&self, layout: &OptionsLayout, point: Vector2<f32>) -> Option<OptionsEvent> {
        if layout.frame[0].contains(point) {
            return Some(OptionsEvent::Done);
        }
        // Audio and Display retain the retail tab slots but remain disabled
        // until they have real user-facing controls.
        for (i, rect) in layout.tabs.iter().take(2).enumerate() {
            if rect.contains(point) {
                return Some(OptionsEvent::Tab(i));
            }
        }
        for (i, rect) in layout.rows.iter().take(4).enumerate() {
            if rect.contains(point) {
                return Some(OptionsEvent::Row(i));
            }
        }
        None
    }

    pub fn activate(&mut self, event: OptionsEvent) -> bool {
        match event {
            OptionsEvent::Done => return true,
            OptionsEvent::Tab(tab) if tab < 2 => {
                self.tab = tab;
                self.error = false;
            }
            OptionsEvent::Row(row) => {
                self.error = user_settings::update(|settings| self.change(settings, row)).is_err();
            }
            _ => {}
        }
        false
    }

    fn change(&self, settings: &mut UserSettings, row: usize) {
        let defaults = crate::user_settings::VrSettings::default();
        let vr = &mut settings.vr;
        match (self.tab, row) {
            (0, 0) => vr.vignette = vr.vignette.next(),
            (0, 1) => vr.vignette_movement = !vr.vignette_movement,
            (0, 2) => vr.vignette_turning = !vr.vignette_turning,
            (0, 3) => {
                vr.vignette = defaults.vignette;
                vr.vignette_movement = defaults.vignette_movement;
                vr.vignette_turning = defaults.vignette_turning;
            }
            (1, 0) => {
                vr.turning = if vr.turning == TurnMode::Snap {
                    TurnMode::Smooth
                } else {
                    TurnMode::Snap
                }
            }
            (1, 1) => vr.snap_angle = cycle(vr.snap_angle, &[30.0, 45.0, 60.0]),
            (1, 2) => vr.smooth_speed = cycle(vr.smooth_speed, &[45.0, 60.0, 90.0, 120.0, 180.0]),
            (1, 3) => {
                vr.turning = defaults.turning;
                vr.snap_angle = defaults.snap_angle;
                vr.smooth_speed = defaults.smooth_speed;
            }
            _ => {}
        }
    }

    pub fn canvas(&self, assets: &mut AssetCache, pointer: Option<Vector2<f32>>) -> UiCanvas {
        let layout = OptionsLayout::load(assets);
        let settings = user_settings::get().vr;
        let mut canvas = UiCanvas::new(vec2(640.0, 480.0));
        canvas.image(Rect::new(0.0, 0.0, 640.0, 480.0), "OPTIONS.PCX");
        let hover = pointer.and_then(|p| self.hit(&layout, p));
        for (i, label) in ["Comfort", "Controls", "Audio", "Display"]
            .iter()
            .enumerate()
        {
            canvas
                .text_native_fit(
                    layout.tabs[i],
                    label,
                    "metafont.fon",
                    HAlign::Center,
                    VAlign::Middle,
                )
                .opacity(if i >= 2 {
                    0.25
                } else if i == self.tab || hover == Some(OptionsEvent::Tab(i)) {
                    1.0
                } else {
                    0.55
                });
        }
        let on_off = |on| if on { "On" } else { "Off" };
        let labels = if self.tab == 0 {
            vec![
                format!("Movement vignette: {}", settings.vignette.label()),
                format!("While moving: {}", on_off(settings.vignette_movement)),
                format!(
                    "While smooth turning: {}",
                    on_off(settings.vignette_turning)
                ),
                "Reset comfort defaults".to_owned(),
            ]
        } else {
            vec![
                format!(
                    "Turning: {}",
                    if settings.turning == TurnMode::Snap {
                        "Snap"
                    } else {
                        "Smooth"
                    }
                ),
                format!("Snap angle: {} degrees", settings.snap_angle),
                format!("Smooth speed: {} degrees/sec", settings.smooth_speed),
                "Reset turning defaults".to_owned(),
            ]
        };
        for (i, label) in labels.iter().enumerate() {
            canvas
                .text_native_fit(
                    layout.rows[i],
                    label,
                    "metafont.fon",
                    HAlign::Center,
                    VAlign::Middle,
                )
                .opacity(if hover == Some(OptionsEvent::Row(i)) {
                    1.0
                } else {
                    0.7
                });
        }
        let body = layout.frame[1];
        canvas.text(
            Rect::new(body.x + 16.0, body.y + body.h - 66.0, body.w - 32.0, 24.0),
            "Click a value to change it",
            "mainfont.fon",
            14.0,
            HAlign::Center,
            VAlign::Middle,
        );
        canvas.text(
            Rect::new(body.x + 16.0, body.y + body.h - 38.0, body.w - 32.0, 24.0),
            if self.error {
                "Could not save settings. Previous values kept."
            } else {
                "Saved automatically."
            },
            "mainfont.fon",
            14.0,
            HAlign::Center,
            VAlign::Middle,
        );
        let strings = assets.get_opt(&STRINGS_IMPORTER, "options.str");
        let done = resolve_menu_label(strings.as_deref(), "done", "Done");
        canvas
            .text_native_fit(
                layout.frame[0],
                &done,
                "metafont.fon",
                HAlign::Center,
                VAlign::Middle,
            )
            .opacity(if hover == Some(OptionsEvent::Done) {
                1.0
            } else {
                0.7
            });
        canvas
    }
}

fn cycle(value: f32, values: &[f32]) -> f32 {
    values[(values.iter().position(|v| *v == value).unwrap_or(0) + 1) % values.len()]
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn retail_hit_regions_disable_unimplemented_tabs() {
        let panel = OptionsPanel::default();
        let layout = OptionsLayout {
            tabs: TABS.to_vec(),
            frame: FRAME.to_vec(),
            rows: ROWS.to_vec(),
        };
        assert_eq!(
            panel.hit(&layout, vec2(320.0, 440.0)),
            Some(OptionsEvent::Done)
        );
        assert_eq!(
            panel.hit(&layout, vec2(230.0, 30.0)),
            Some(OptionsEvent::Tab(1))
        );
        assert_eq!(panel.hit(&layout, vec2(400.0, 30.0)), None);
        assert_eq!(
            panel.hit(&layout, vec2(320.0, 80.0)),
            Some(OptionsEvent::Row(0))
        );
    }
}
