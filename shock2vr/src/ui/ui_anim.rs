//! Little looping screen animations layered over a frontend backdrop.
//!
//! A screen `<panel>` may ship `<panel>M.STR` naming its anims, each as
//! `x,y fps mode`, with frames `<name>_1.PCX`, `<name>_2.PCX`, ... blitted at
//! (x, y) in canvas pixels at their authored size. The main menu's is
//! `anim: "18,18 6 l"` - the Tri-Optimum logo with scrolling binary behind it.
//! Mode `l` loops; `r` ping-pongs (1..n..2, repeat).

use std::collections::HashMap;

use cgmath::{Vector2, vec2};
use dark::importers::{STRINGS_IMPORTER, TEXTURE_IMPORTER};
use engine::assets::asset_cache::AssetCache;

use super::{ImageKind, Rect, UiCanvas, texture_options, texture_px};

#[derive(Clone, Copy, Debug, PartialEq)]
enum Mode {
    Loop,
    PingPong,
}

#[derive(Clone, Debug, PartialEq)]
struct AnimSpec {
    name: String,
    position: Vector2<f32>,
    seconds_per_frame: f32,
    mode: Mode,
}

/// The anims a `<panel>M.STR` table declares, in list order. Malformed entries
/// are skipped.
fn parse_specs(strings: &HashMap<String, String>) -> Vec<AnimSpec> {
    // The shipped list value trails a comment (`"anim" /* "maina ..." */`),
    // which the table parser folds into the value: keep up to its closing quote.
    let Some(list) = strings.get("list") else {
        return Vec::new();
    };
    let list = list.split('"').next().unwrap_or_default();
    list.split_whitespace()
        .filter_map(|name| {
            let entry = strings.get(&name.to_ascii_lowercase())?;
            let mut fields = entry.split_whitespace();
            let (x, y) = fields.next()?.split_once(',')?;
            let fps: f32 = fields.next()?.parse().ok()?;
            let mode = match fields.next() {
                Some(m) if m.eq_ignore_ascii_case("r") => Mode::PingPong,
                _ => Mode::Loop,
            };
            let position = vec2(x.trim().parse().ok()?, y.trim().parse().ok()?);
            (fps > 0.0).then(|| AnimSpec {
                name: name.to_owned(),
                position,
                seconds_per_frame: 1.0 / fps,
                mode,
            })
        })
        .collect()
}

/// Which of `frame_count` frames shows on tick `tick`.
fn frame_index(tick: usize, frame_count: usize, mode: Mode) -> usize {
    match mode {
        Mode::PingPong if frame_count > 1 => {
            let cycle = frame_count * 2 - 2;
            let t = tick % cycle;
            if t >= frame_count { cycle - t } else { t }
        }
        _ => tick % frame_count,
    }
}

struct Anim {
    spec: AnimSpec,
    /// Frame asset names with their authored pixel size.
    frames: Vec<(String, Vector2<f32>)>,
    tick: usize,
    /// Time banked toward the next tick.
    banked: f32,
}

impl Anim {
    /// Advance at most one frame per update and cap the backlog at one frame,
    /// as the original did, so a hitch never fast-forwards the loop.
    fn advance(&mut self, dt: f32) {
        self.banked += dt;
        if self.banked > self.spec.seconds_per_frame {
            self.banked =
                (self.banked - self.spec.seconds_per_frame).min(self.spec.seconds_per_frame);
            self.tick += 1;
        }
    }
}

/// Every anim of one frontend panel.
pub struct UiAnims {
    panel: String,
    anims: Vec<Anim>,
}

impl UiAnims {
    /// Load `<panel>M.STR` and its frames; empty when the panel has none.
    pub fn load(asset_cache: &mut AssetCache, panel: &str) -> Self {
        let specs = asset_cache
            .get_opt(&STRINGS_IMPORTER, &format!("{panel}M.STR"))
            .map(|strings| parse_specs(&strings))
            .unwrap_or_default();
        let anims = specs
            .into_iter()
            .filter_map(|spec| {
                let frames = load_frames(asset_cache, &spec.name);
                (!frames.is_empty()).then_some(Anim {
                    spec,
                    frames,
                    tick: 0,
                    banked: 0.0,
                })
            })
            .collect();
        Self {
            panel: panel.to_owned(),
            anims,
        }
    }

    pub fn panel(&self) -> &str {
        &self.panel
    }

    pub fn advance(&mut self, dt: f32) {
        for anim in &mut self.anims {
            anim.advance(dt);
        }
    }

    /// Draw each anim's current frame; call right after the backdrop.
    pub fn draw(&self, canvas: &mut UiCanvas) {
        for anim in &self.anims {
            let (texture, size) =
                &anim.frames[frame_index(anim.tick, anim.frames.len(), anim.spec.mode)];
            let at = anim.spec.position;
            canvas.image(Rect::new(at.x, at.y, size.x, size.y), texture);
        }
    }
}

/// `<name>_1.PCX`, `<name>_2.PCX`, ... up to the first missing number. The
/// shipped main-menu set spells two of its twelve frames `ANIM_08`/`ANIM_09`,
/// so a zero-padded name is accepted too.
fn load_frames(asset_cache: &mut AssetCache, name: &str) -> Vec<(String, Vector2<f32>)> {
    let options = texture_options(ImageKind::Ui);
    let mut frames = Vec::new();
    for number in 1.. {
        let found = [
            format!("{name}_{number}.PCX"),
            format!("{name}_{number:02}.PCX"),
        ]
        .into_iter()
        .find_map(|file| {
            let texture = asset_cache.get_ext_opt(&TEXTURE_IMPORTER, &file, &options)?;
            Some((file, texture_px(&texture)))
        });
        match found {
            Some(frame) => frames.push(frame),
            None => break,
        }
    }
    frames
}

#[cfg(test)]
mod tests {
    use super::*;
    use dark::importers::parse_strings;

    /// `MAINM.STR` as shipped.
    const MAINM_STR: &str = r#"// string file for anims.
// Format is list of anims
// then x,y fps (r|l)
// where 'r' means reverse, and 'l' means loop normally
list: "anim" /* "maina mainb mainc main" */
anim: "18,18 6 l""#;

    fn strings(source: &str) -> HashMap<String, String> {
        parse_strings(&source.lines().map(str::to_owned).collect::<Vec<_>>())
    }

    #[test]
    fn parses_the_shipped_main_menu_anim() {
        assert_eq!(
            parse_specs(&strings(MAINM_STR)),
            vec![AnimSpec {
                name: "anim".to_owned(),
                position: vec2(18.0, 18.0),
                seconds_per_frame: 1.0 / 6.0,
                mode: Mode::Loop,
            }]
        );
    }

    #[test]
    fn an_empty_list_declares_nothing() {
        let table = strings("list: \"\" /* \"maina\" */\nmaina: \"17,69 12.1 r\"");
        assert!(parse_specs(&table).is_empty());
    }

    #[test]
    fn loop_wraps_and_ping_pong_bounces() {
        let looped: Vec<_> = (0..5).map(|t| frame_index(t, 3, Mode::Loop)).collect();
        assert_eq!(looped, [0, 1, 2, 0, 1]);
        let bounced: Vec<_> = (0..7).map(|t| frame_index(t, 3, Mode::PingPong)).collect();
        assert_eq!(bounced, [0, 1, 2, 1, 0, 1, 2]);
        assert_eq!(frame_index(5, 1, Mode::PingPong), 0);
    }

    #[test]
    fn advances_one_frame_per_update_without_fast_forwarding() {
        let mut anim = Anim {
            spec: parse_specs(&strings(MAINM_STR)).remove(0),
            frames: Vec::new(),
            tick: 0,
            banked: 0.0,
        };
        anim.advance(0.1);
        assert_eq!(anim.tick, 0);
        anim.advance(0.1);
        assert_eq!(anim.tick, 1);
        // A two-second hitch still moves one frame.
        anim.advance(2.0);
        assert_eq!(anim.tick, 2);
    }
}
