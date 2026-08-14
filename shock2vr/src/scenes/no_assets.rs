//! The screen shown when there is no game data to load.
//!
//! shock2quest ships no game content: it needs a retail install copied into
//! place. Until now, not having done that produced a panic - on the desktop a
//! stack trace, and on Quest a silent return to the Horizon shell. Neither says
//! what is wrong or what to do, and on a headset there is nowhere to read a
//! stack trace anyway.
//!
//! This screen is that message. Two things make it unusual among the scenes:
//!
//! - **It cannot use any asset.** Every other screen draws a `.PCX` backdrop
//!   and `METAFONT.FON` text out of the game data - exactly what is missing
//!   here. So it is text-only, in [`crate::ui::BUILTIN_FONT`], the font
//!   compiled into the engine.
//! - **It runs without a [`crate::Game`].** `Game::init` needs the gamesys, so
//!   the runtimes construct this scene directly instead.
//!
//! It is otherwise an ordinary [`GameScene`] on the shared [`UiCanvas`], so it
//! renders in flat and in VR through the same single layout pass as every other
//! screen (see AGENTS.md "UI Renders Identically in Flatscreen and VR").

use cgmath::{Quaternion, Vector2, Vector3, vec2, vec3};
use engine::{
    assets::asset_cache::AssetCache,
    audio::AudioContext,
    scene::{SceneObject, light::SpotLight},
};
use shipyard::{EntityId, World};

use crate::{
    GameOptions, PresentationMode,
    game_scene::GameScene,
    input_context::InputContext,
    install::InstallStatus,
    mission::GlobalContext,
    scripts::{Effect, GlobalEffect},
    time::Time,
    ui::{
        BUILTIN_FONT, HAlign, Rect, ScaleMode, UiCanvas, VAlign, VR_COMPONENT_Z_STEP,
        frontend_panel,
    },
};

/// Authored on the same 640x480 canvas as the rest of the frontend.
const CANVAS_W: f32 = 640.0;
const CANVAS_H: f32 = 480.0;
/// Letterboxed like the other 4:3 screens, so text never stretches.
const SCALE_MODE: ScaleMode = ScaleMode::PreserveAspect;

const TITLE_SIZE: f32 = 24.0;
const BODY_SIZE: f32 = 16.0;
const LINE_HEIGHT: f32 = 22.0;
const TITLE_Y: f32 = 90.0;
const BODY_Y: f32 = 150.0;

const TITLE: &str = "GAME DATA NOT FOUND";

/// Body rows that fit between [`BODY_Y`] and the bottom of the canvas.
fn max_body_lines() -> usize {
    (((CANVAS_H - BODY_Y) / LINE_HEIGHT).floor() as usize).max(1)
}

/// How many characters fit across the canvas at `size`.
///
/// Exact, not an estimate: the builtin font is monospace, so one glyph is one
/// advance of `size` canvas pixels.
fn columns_at(size: f32) -> usize {
    (CANVAS_W / size) as usize
}

/// Break `text` to at most `columns` characters per line, preferring a space.
///
/// A filesystem path has no spaces and can be longer than the canvas, so it
/// hard-splits rather than overflowing - a path shown with its middle missing
/// is useless, and this screen exists to be acted on.
fn wrap(text: &str, columns: usize) -> Vec<String> {
    if columns == 0 {
        return Vec::new();
    }
    let mut lines = Vec::new();
    for paragraph in text.split('\n') {
        if paragraph.chars().count() <= columns {
            lines.push(paragraph.to_owned());
            continue;
        }
        let mut current = String::new();
        for word in paragraph.split(' ') {
            let mut word = word;
            // A single word longer than the line (a path) is split across lines.
            while word.chars().count() > columns {
                if !current.is_empty() {
                    lines.push(std::mem::take(&mut current));
                }
                let split = word
                    .char_indices()
                    .nth(columns)
                    .map(|(i, _)| i)
                    .unwrap_or(word.len());
                lines.push(word[..split].to_owned());
                word = &word[split..];
            }
            let needed = word.chars().count() + usize::from(!current.is_empty());
            if current.chars().count() + needed > columns {
                lines.push(std::mem::take(&mut current));
            }
            if !current.is_empty() {
                current.push(' ');
            }
            current.push_str(word);
        }
        lines.push(current);
    }
    lines
}

/// Replace anything the builtin font cannot draw with `?`.
///
/// The font covers printable ASCII only (U+0020..U+007E). Unmapped characters
/// are silently dropped by `measure_text_width`, so a path containing them
/// would be shown *wrong* - and a wrapped chunk consisting entirely of them
/// measures zero-wide, builds a mesh with no vertices, and panics. A screen
/// whose whole purpose is to replace a panic must not have one.
fn to_drawable(text: &str) -> String {
    text.chars()
        .map(|c| if (' '..='~').contains(&c) { c } else { '?' })
        .collect()
}

/// The message body for `status`, already wrapped to the canvas.
///
/// Pure, so what the player is told is unit-testable without a renderer.
pub fn message_lines(status: &InstallStatus) -> Vec<String> {
    let columns = columns_at(BODY_SIZE);
    let path = to_drawable(&status.absolute_data_root().display().to_string());

    // Everything but the path is fixed, so the path is the only thing that can
    // push the message off the bottom of the canvas. Drop leading path segments
    // until it fits, rather than letting the closing instruction scroll into the
    // void: the deepest segments are the ones that identify the directory, and
    // the untrimmed path is in the startup log either way.
    //
    // Iterating over a fixed segment list (rather than re-trimming a string
    // that has already grown a "..." prefix) is what makes this terminate.
    let segments: Vec<&str> = path.split(['/', '\\']).collect();
    for dropped in 0..segments.len() {
        let shown = if dropped == 0 {
            path.clone()
        } else {
            format!("...{}", segments[dropped..].join("/"))
        };
        let lines = wrap(&body_text(&shown), columns);
        if lines.len() <= max_body_lines() {
            return lines;
        }
    }

    // A single unsplittable segment longer than the canvas allows.
    let mut truncated: String = path.chars().take(columns.saturating_sub(3)).collect();
    truncated.push_str("...");
    wrap(&body_text(&truncated), columns)
}

/// The message around `path`.
fn body_text(path: &str) -> String {
    format!(
        "shock2quest needs a copy of\n\
         System Shock 2: 25th Anniversary Remaster.\n\
         \n\
         Copy sshock2.kpf and the mods folder\n\
         from your install into:\n\
         \n\
         {path}\n\
         \n\
         then start shock2quest again."
    )
}

/// See the module docs.
pub struct NoAssetsScene {
    lines: Vec<String>,
    head_rotation: Quaternion<f32>,
    world: World,
}

impl NoAssetsScene {
    pub fn new(status: &InstallStatus) -> NoAssetsScene {
        NoAssetsScene {
            lines: message_lines(status),
            head_rotation: Quaternion::new(1.0, 0.0, 0.0, 0.0),
            world: World::new(),
        }
    }

    /// The single layout pass. Both presentations map this same canvas, so they
    /// cannot place the text differently.
    fn build_canvas(&self) -> UiCanvas {
        let mut canvas = UiCanvas::new(vec2(CANVAS_W, CANVAS_H));
        canvas.text(
            Rect::new(0.0, TITLE_Y, CANVAS_W, TITLE_SIZE),
            TITLE,
            BUILTIN_FONT,
            TITLE_SIZE,
            HAlign::Center,
            VAlign::Middle,
        );
        for (index, line) in self.lines.iter().enumerate() {
            // Blank lines are vertical spacing, not content. Emitting one as a
            // text element builds a mesh with no vertices, which panics in
            // `engine::scene::mesh`; the line still advances `index`, so the
            // spacing it represents is preserved.
            if line.trim().is_empty() {
                continue;
            }
            canvas.text(
                Rect::new(
                    0.0,
                    BODY_Y + index as f32 * LINE_HEIGHT,
                    CANVAS_W,
                    BODY_SIZE,
                ),
                line,
                BUILTIN_FONT,
                BODY_SIZE,
                HAlign::Center,
                VAlign::Middle,
            );
        }
        canvas
    }
}

impl GameScene for NoAssetsScene {
    fn update(
        &mut self,
        _time: &Time,
        input_context: &InputContext,
        _asset_cache: &mut AssetCache,
        _game_options: &GameOptions,
        _command_effects: Vec<Effect>,
    ) -> Vec<Effect> {
        self.head_rotation = input_context.head.rotation;
        Vec::new()
    }

    fn render(
        &mut self,
        asset_cache: &mut AssetCache,
        options: &GameOptions,
    ) -> (Vec<SceneObject>, Vector3<f32>, Quaternion<f32>) {
        let identity = Quaternion::new(1.0, 0.0, 0.0, 0.0);
        if options.presentation_mode != PresentationMode::Vr {
            // Flat draws in screen space from `render_per_eye`, which is where
            // the screen size is known.
            return (Vec::new(), vec3(0.0, 0.0, 0.0), identity);
        }
        let panel = frontend_panel(self.head_rotation);
        let objects = self.build_canvas().render_world_space(
            asset_cache,
            panel.transform(),
            None,
            None,
            VR_COMPONENT_Z_STEP,
        );
        (objects, vec3(0.0, 0.0, 0.0), identity)
    }

    fn render_per_eye(
        &mut self,
        asset_cache: &mut AssetCache,
        _view: cgmath::Matrix4<f32>,
        _projection: cgmath::Matrix4<f32>,
        screen_size: Vector2<f32>,
        options: &GameOptions,
    ) -> Vec<SceneObject> {
        if options.presentation_mode == PresentationMode::Vr {
            // In VR the panel from `render` already carries the canvas; a
            // screen-space copy would paste it over both eyes and hide it.
            return Vec::new();
        }
        self.build_canvas()
            .render_screen_space(asset_cache, screen_size, SCALE_MODE)
    }

    fn handle_effects(
        &mut self,
        _effects: Vec<Effect>,
        _global_context: &GlobalContext,
        _game_options: &GameOptions,
        _asset_cache: &mut AssetCache,
        _audio_context: &mut AudioContext<EntityId, String>,
    ) -> Vec<GlobalEffect> {
        Vec::new()
    }

    fn get_hand_spotlights(&self, _options: &GameOptions) -> Vec<SpotLight> {
        Vec::new()
    }

    fn world(&self) -> &World {
        &self.world
    }

    fn scene_name(&self) -> &str {
        "no_assets"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::install::{InstallKind, InstallStatus};
    use std::path::PathBuf;

    fn missing(root: &str) -> InstallStatus {
        InstallStatus {
            data_root: PathBuf::from(root),
            kind: InstallKind::Missing,
            found: Vec::new(),
            missing_mods: Vec::new(),
        }
    }

    /// The player has to be able to read every line, so nothing may run past
    /// the canvas. Monospace makes this exact rather than approximate.
    #[test]
    fn no_line_overflows_the_canvas() {
        let columns = columns_at(BODY_SIZE);
        for line in message_lines(&missing("/sdcard/shock2quest")) {
            assert!(
                line.chars().count() <= columns,
                "line {line:?} is {} columns, over the {columns} that fit",
                line.chars().count()
            );
        }
        assert!(TITLE.chars().count() as f32 * TITLE_SIZE <= CANVAS_W);
    }

    /// The whole point of the screen: say where to put the files.
    #[test]
    fn the_message_names_the_data_root_and_what_to_copy() {
        let lines = message_lines(&missing("/sdcard/shock2quest")).join("\n");
        assert!(lines.contains("/sdcard/shock2quest"), "{lines}");
        assert!(lines.contains("sshock2.kpf"), "{lines}");
        assert!(lines.contains("mods"), "{lines}");
    }

    /// A path can be longer than the canvas and has no spaces to break on.
    /// Truncating it would defeat the screen, so it wraps across lines and
    /// every character survives.
    #[test]
    fn a_long_path_wraps_instead_of_overflowing() {
        let long =
            "/Users/somebody/very/deeply/nested/directory/tree/that/keeps/going/shock2quest-data";
        let columns = columns_at(BODY_SIZE);
        let lines = message_lines(&missing(long));
        for line in &lines {
            assert!(line.chars().count() <= columns, "{line:?}");
        }
        let rejoined: String = lines.join("").replace(' ', "");
        assert!(
            rejoined.contains(&long.replace(' ', "")),
            "the path must survive wrapping intact: {lines:?}"
        );
    }

    /// A blank line is spacing, and a text element with no glyphs builds an
    /// empty mesh - which panics at render. Negative test for that crash.
    #[test]
    fn the_canvas_never_holds_an_empty_string() {
        let scene = NoAssetsScene::new(&missing("/sdcard/shock2quest"));
        let canvas = scene.build_canvas();
        assert!(canvas.element_count() > 0, "the screen must draw something");
        for element in canvas.elements() {
            if let crate::ui::UiElement::Text { text, .. } = element {
                assert!(!text.trim().is_empty(), "empty text element in the canvas");
            }
        }
    }

    /// The blank lines still have to *space* the block, so dropping them from
    /// the canvas must not pull the following lines up.
    #[test]
    fn blank_lines_still_occupy_their_row() {
        let scene = NoAssetsScene::new(&missing("/sdcard/shock2quest"));
        let blanks = scene.lines.iter().filter(|l| l.trim().is_empty()).count();
        assert!(blanks > 0, "the message is expected to have blank lines");
        let drawn = scene.build_canvas().element_count() - 1; // minus the title
        assert_eq!(drawn, scene.lines.len() - blanks);

        // ...and the rows after a blank must not slide up into its place.
        let first_blank = scene
            .lines
            .iter()
            .position(|l| l.trim().is_empty())
            .unwrap();
        let next_text = first_blank + 1;
        let canvas = scene.build_canvas();
        let expected_y = BODY_Y + next_text as f32 * LINE_HEIGHT;
        let found = canvas
            .elements()
            .iter()
            .any(|e| (e.rect().y - expected_y).abs() < 0.01);
        assert!(
            found,
            "no element sits at y={expected_y}, so a blank row collapsed"
        );
    }

    /// A pathological data root must not push the message off the bottom of
    /// the canvas - the horizontal checks above would still pass.
    #[test]
    fn a_pathological_root_still_fits_vertically() {
        let deep = format!("/{}", vec!["averylongdirectoryname"; 12].join("/"));
        let lines = message_lines(&missing(&deep));
        let bottom = BODY_Y + (lines.len().saturating_sub(1)) as f32 * LINE_HEIGHT + BODY_SIZE;
        assert!(
            bottom <= CANVAS_H,
            "{} lines run to y={bottom}, past the {CANVAS_H} canvas",
            lines.len()
        );
    }

    /// Non-ASCII cannot reach the glyph table: it would render wrong, and a
    /// wrapped chunk of nothing but unmapped characters would build an empty
    /// mesh and panic.
    #[test]
    fn a_non_ascii_root_is_made_drawable() {
        let lines = message_lines(&missing("/Users/\u{5c71}\u{7530}/\u{0161}ock2"));
        let joined = lines.join("");
        assert!(joined.is_ascii(), "{joined:?}");
        assert!(joined.contains('?'));
        for line in &lines {
            assert!(!line.trim().is_empty() || line.is_empty());
        }
    }

    #[test]
    fn wrap_breaks_on_spaces_when_it_can() {
        assert_eq!(wrap("alpha beta gamma", 11), vec!["alpha beta", "gamma"]);
    }
}
