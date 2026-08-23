//! Developer screen (live-tunable runtime parameters, and the debug-scene
//! launcher).
//!
//! The frontend host for [`crate::ui::dev_params_panel`]: the shared row
//! builder on the `GAMELOD.PCX` archive frame, reached from the main menu's
//! Developer entry (`GlobalEffect::ShowDeveloper`); "Done" returns there. The
//! pause overlay hosts the very same builder as its Developer page, so the
//! screen is identical whichever way it is reached.
//!
//! The screen has a second page: a list of the debug scenes, reached from the
//! upper framed button (the load screen's "Load" frame) and returning to the
//! parameters with its own "Done". It exists because a headset picks its scene
//! from a file read at startup, so without an in-game entry point every change
//! of debug scene - or every death inside one - costs an APK relaunch. The
//! names come from the same registry `--mission debug_x` dispatches from
//! ([`crate::scenes::debug_scene_names`]), so the launcher cannot drift from
//! what the game can actually start.
//!
//! Structurally a sibling of [`crate::scenes::LoadGameScene`]: one canvas,
//! rendered screen-space when flat and on a world panel in VR, with the
//! shared rising-edge click rules. Every rect on both pages is resolved once,
//! in canvas pixels, and handed to the hit test and the draw alike - which is
//! what makes the flat pointer and the VR ray land on the same widget
//! (AGENTS.md §3).

use cgmath::{Quaternion, Vector2, Vector3, vec2, vec3};
use engine::{
    assets::asset_cache::AssetCache,
    audio::AudioContext,
    scene::{SceneObject, light::SpotLight},
};
use shipyard::{EntityId, UniqueViewMut, World};

use crate::{
    GameOptions, PresentationMode,
    game_scene::GameScene,
    input_context::InputContext,
    mission::GlobalContext,
    scripts::{Effect, GlobalEffect},
    time::Time,
    ui::{
        FrontendMenu,
        HAlign,
        Rect,
        ScaleMode,
        UiCanvas,
        VAlign,
        dev_params_panel,
        // The archive-database frame: a header line, a dark pane the rows sit
        // in, and framed button art for "Done" - the geometry the panel is
        // laid out against, owned by the panel so both hosts share it.
        dev_params_panel::{BACKDROP_TEXTURE, DevParamsEvent, FIELD_TOP_Y, PanelRects},
        list_scroll::{self, ScrollHalf},
    },
};

#[cfg(test)]
use crate::{
    input_context::Pointer2D,
    ui::{
        resolve_click_at as shell_resolve_click_at, resolve_flat_click, vr_frontend_pointer_pass,
    },
};

/// The screen is authored on the original 640x480 canvas.
const CANVAS_W: f32 = 640.0;
const CANVAS_H: f32 = 480.0;
/// The 4:3 art is letterboxed (not stretched) on non-4:3 windows.
const SCALE_MODE: ScaleMode = ScaleMode::PreserveAspect;

/// Display font for the header and the framed buttons, and the small data font
/// the scene names read as data in - the same pairing the parameter rows and
/// the load screen's save rows use.
const MENU_FONT: &str = "metafont.fon";
const LIST_FONT: &str = "mainfont.fon";

/// Row pitch of the launcher's list, matching the load screen's save rows -
/// the same art, the same font, the same rows.
const SCENE_ROW_H: f32 = 19.0;
/// Horizontal inset for a scene name, on both edges of the row so the text
/// clears the pane's border. Hit-testing uses the whole row, so it costs no
/// click.
const SCENE_TEXT_INSET: f32 = 8.0;

/// Opacity for a button that cannot be acted on, one that can, and the
/// selected row / hovered button - the load screen's three levels.
const DISABLED_OPACITY: f32 = 0.3;
const IDLE_OPACITY: f32 = 0.65;
const ACTIVE_OPACITY: f32 = 1.0;

const SCENES_HEADER_LABEL: &str = "Select a debug scene.";
/// The upper framed button: the launcher's door on the parameters page, and
/// the launch itself on the launcher page.
const OPEN_SCENES_LABEL: &str = "Scenes";
const LAUNCH_LABEL: &str = "Launch";
const DONE_LABEL: &str = "Done";

/// Which page of the screen is showing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeveloperPage {
    /// The tunable parameter rows ([`dev_params_panel`]).
    Params,
    /// The debug-scene launcher.
    Scenes,
}

/// What a click on the screen asks for, whichever page it landed on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeveloperAction {
    /// Something on the parameter page: a step, its scroll, or "Done".
    Param(DevParamsEvent),
    /// Open the debug-scene launcher.
    OpenScenes,
    /// Highlight the debug scene at this index in the registry.
    SelectScene(usize),
    /// Launch the highlighted debug scene.
    LaunchScene,
    /// Scroll the launcher's list.
    ScrollScenes(ScrollHalf),
    /// Leave the launcher, back to the parameters.
    CloseScenes,
}

/// Everything a click depends on, resolved once per frame and handed to the
/// hit test and the draw alike so the two can never disagree about where a
/// widget is - or about which page it belongs to.
#[derive(Clone, Copy, Debug, PartialEq)]
struct ScreenState {
    page: DeveloperPage,
    rects: PanelRects,
    /// Registry index drawn in the parameter pane's top row.
    scroll: usize,
    /// Registry index drawn in the launcher's top row.
    scene_scroll: usize,
    /// Whether a scene row is highlighted - "Launch" is inert without one,
    /// exactly as the load screen's "Load" is.
    has_selection: bool,
}

/// How many debug scenes the launcher lists.
fn scene_count() -> usize {
    super::debug_scene_names().count()
}

fn scene_rows_per_page(rects: PanelRects) -> usize {
    list_scroll::rows_per_page(rects.list_rect(), FIELD_TOP_Y, SCENE_ROW_H)
}

fn scene_max_scroll(rects: PanelRects) -> usize {
    list_scroll::max_scroll(scene_count(), scene_rows_per_page(rects))
}

/// The registry indices on screen at `scene_scroll`, clamped to the page.
fn scene_visible_rows(rects: PanelRects, scene_scroll: usize) -> std::ops::Range<usize> {
    list_scroll::visible_rows(scene_count(), scene_rows_per_page(rects), scene_scroll)
}

/// The launcher's scroll rocker, in the gutter down the pane's right edge -
/// the very same one the parameter page scrolls with.
fn scene_rocker(rects: PanelRects) -> Option<list_scroll::Rocker> {
    list_scroll::rocker(rects.list_rect(), FIELD_TOP_Y, scene_max_scroll(rects) > 0)
}

/// The canvas rect of the launcher's `slot`-th visible row. The rows stop
/// short of the scroll gutter whenever it is in use, so a row and the rocker
/// can never claim the same point.
fn scene_row_rect(rects: PanelRects, slot: usize) -> Rect {
    let list = rects.list_rect();
    let gutter = if scene_max_scroll(rects) > 0 {
        list_scroll::GUTTER_W
    } else {
        0.0
    };
    Rect::new(
        list.x,
        list.y + slot as f32 * SCENE_ROW_H,
        (list.w - gutter).max(0.0),
        SCENE_ROW_H,
    )
}

/// The rect a scene name is drawn in: the row, inset on both edges.
fn scene_text_rect(rects: PanelRects, slot: usize) -> Rect {
    let row = scene_row_rect(rects, slot);
    Rect::new(
        row.x + SCENE_TEXT_INSET,
        row.y,
        (row.w - 2.0 * SCENE_TEXT_INSET).max(0.0),
        row.h,
    )
}

/// What is at a canvas point, on whichever page is showing. Shared by the
/// click, the hover highlight and the rollover sound - and by both
/// presentations, so the flat pointer and the VR ray resolve identically.
fn hit(state: ScreenState, point: Vector2<f32>) -> Option<DeveloperAction> {
    match state.page {
        DeveloperPage::Params => {
            if state.rects.action_rect().contains(point) {
                return Some(DeveloperAction::OpenScenes);
            }
            dev_params_panel::hit(state.rects, state.scroll, point).map(DeveloperAction::Param)
        }
        DeveloperPage::Scenes => {
            // "Launch" is inert with nothing selected rather than launching
            // whatever happens to be first.
            if state.has_selection && state.rects.action_rect().contains(point) {
                return Some(DeveloperAction::LaunchScene);
            }
            if state.rects.done_rect().contains(point) {
                return Some(DeveloperAction::CloseScenes);
            }
            let rows = scene_visible_rows(state.rects, state.scene_scroll);
            if let Some(rocker) = scene_rocker(state.rects) {
                if let Some(half) =
                    list_scroll::hit(&rocker, rows.start, scene_max_scroll(state.rects), point)
                {
                    return Some(DeveloperAction::ScrollScenes(half));
                }
            }
            // Rows carry the index of the scene scrolled into that slot, never
            // the slot itself: a positional index would launch whatever scene
            // *used* to be in the row the moment the list scrolls.
            (0..rows.len())
                .find(|slot| scene_row_rect(state.rects, *slot).contains(point))
                .map(|slot| DeveloperAction::SelectScene(rows.start + slot))
        }
    }
}

/// Shared click core: both presentations reduce to "a point on the canvas plus
/// a pressed flag", so the rising-edge rule lives here once.
#[cfg(test)]
fn resolve_click_at(
    state: ScreenState,
    point: Option<Vector2<f32>>,
    pressed: bool,
    last_pressed: bool,
) -> (Option<DeveloperAction>, bool) {
    shell_resolve_click_at(point, pressed, last_pressed, |point| hit(state, point))
}

/// Pure click resolution for the flat pointer.
#[cfg(test)]
fn resolve_click(
    state: ScreenState,
    pointer: Option<Pointer2D>,
    last_pressed: bool,
    screen_size: Vector2<f32>,
) -> (Option<DeveloperAction>, bool, Option<Vector2<f32>>) {
    resolve_flat_click(
        pointer,
        last_pressed,
        screen_size,
        vec2(CANVAS_W, CANVAS_H),
        SCALE_MODE,
        |point| hit(state, point),
    )
}

pub struct DeveloperScene {
    world: World,
    scene_name: String,
    menu: FrontendMenu<DeveloperAction>,
    /// The panel's widget rects, re-resolved from `GAMELODR.BIN` each update
    /// (the render path takes `&self`, so it reads the resolved value here).
    panel_rects: PanelRects,
    /// Index of the registry parameter drawn in the pane's top row. The panel
    /// itself is stateless, so the scroll position lives with the host and is
    /// handed to the hit test and the render alike.
    scroll: usize,
    /// Which page is showing.
    page: DeveloperPage,
    /// Index of the debug scene drawn in the launcher's top row.
    scene_scroll: usize,
    /// The highlighted debug scene, as an index into the registry.
    selected_scene: Option<usize>,
}

impl DeveloperScene {
    pub fn new() -> Self {
        Self {
            world: super::ui_scene_world(),
            scene_name: "developer".to_owned(),
            menu: FrontendMenu::new(vec2(CANVAS_W, CANVAS_H), SCALE_MODE),
            panel_rects: PanelRects::default(),
            scroll: 0,
            page: DeveloperPage::Params,
            // Preselect the first scene so "Launch" is immediately meaningful,
            // the way the load screen preselects the most recent save.
            scene_scroll: 0,
            selected_scene: (scene_count() > 0).then_some(0),
        }
    }

    /// Everything a click on this frame depends on, in one value shared by the
    /// hit test and the draw.
    fn state(&self) -> ScreenState {
        ScreenState {
            page: self.page,
            rects: self.panel_rects,
            scroll: self.scroll,
            scene_scroll: self.scene_scroll,
            has_selection: self.selected_scene.is_some(),
        }
    }

    /// The screen, described once; presentations differ only in how this
    /// canvas is rendered.
    fn build_canvas(&self, pointer_canvas: Option<Vector2<f32>>) -> UiCanvas {
        let mut canvas = UiCanvas::new(vec2(CANVAS_W, CANVAS_H));
        canvas.image(Rect::new(0.0, 0.0, CANVAS_W, CANVAS_H), BACKDROP_TEXTURE);

        let state = self.state();
        let hovered = pointer_canvas.and_then(|point| hit(state, point));
        let button = |canvas: &mut UiCanvas, rect: Rect, text: &str, action, enabled: bool| {
            let opacity = if !enabled {
                DISABLED_OPACITY
            } else if hovered == Some(action) {
                ACTIVE_OPACITY
            } else {
                IDLE_OPACITY
            };
            canvas
                .text_native(rect, text, MENU_FONT, HAlign::Center, VAlign::Middle)
                .opacity(opacity);
        };

        match self.page {
            DeveloperPage::Params => {
                // The rows, the header and "Done" are the shared panel's; the
                // launcher's door is this host's, because the pause overlay
                // hosts the same panel and cannot swap the scene under itself.
                dev_params_panel::draw(&mut canvas, self.panel_rects, self.scroll, pointer_canvas);
                button(
                    &mut canvas,
                    self.panel_rects.action_rect(),
                    OPEN_SCENES_LABEL,
                    DeveloperAction::OpenScenes,
                    true,
                );
            }
            DeveloperPage::Scenes => {
                canvas.text_native(
                    self.panel_rects.header_rect(),
                    SCENES_HEADER_LABEL,
                    MENU_FONT,
                    HAlign::Center,
                    VAlign::Middle,
                );

                let rows = scene_visible_rows(self.panel_rects, self.scene_scroll);
                for (slot, name) in super::debug_scene_names()
                    .skip(rows.start)
                    .take(rows.len())
                    .enumerate()
                {
                    canvas
                        .text_native_fit(
                            scene_text_rect(self.panel_rects, slot),
                            name,
                            LIST_FONT,
                            HAlign::Left,
                            VAlign::Middle,
                        )
                        .opacity(if self.selected_scene == Some(rows.start + slot) {
                            ACTIVE_OPACITY
                        } else {
                            IDLE_OPACITY
                        });
                }

                if let Some(rocker) = scene_rocker(self.panel_rects) {
                    list_scroll::draw(
                        &mut canvas,
                        &rocker,
                        rows.start,
                        scene_max_scroll(self.panel_rects),
                        match hovered {
                            Some(DeveloperAction::ScrollScenes(half)) => Some(half),
                            _ => None,
                        },
                    );
                }

                button(
                    &mut canvas,
                    self.panel_rects.action_rect(),
                    LAUNCH_LABEL,
                    DeveloperAction::LaunchScene,
                    self.selected_scene.is_some(),
                );
                button(
                    &mut canvas,
                    self.panel_rects.done_rect(),
                    DONE_LABEL,
                    DeveloperAction::CloseScenes,
                    true,
                );
            }
        }

        canvas
    }

    /// Apply a clicked action. Only a launch and leaving the screen reach
    /// `Game`; the pages, the scroll and the selection are absorbed here.
    fn handle_action(&mut self, action: DeveloperAction) -> Vec<Effect> {
        match action {
            DeveloperAction::Param(event) => {
                if dev_params_panel::activate(self.panel_rects, event, &mut self.scroll) {
                    return vec![Effect::GlobalEffect(GlobalEffect::ShowMainMenu)];
                }
            }
            DeveloperAction::OpenScenes => self.page = DeveloperPage::Scenes,
            DeveloperAction::CloseScenes => self.page = DeveloperPage::Params,
            DeveloperAction::SelectScene(index) => self.selected_scene = Some(index),
            DeveloperAction::ScrollScenes(half) => list_scroll::apply(
                half,
                &mut self.scene_scroll,
                scene_max_scroll(self.panel_rects),
            ),
            DeveloperAction::LaunchScene => {
                if let Some(name) = self
                    .selected_scene
                    .and_then(|index| super::debug_scene_names().nth(index))
                {
                    return vec![Effect::GlobalEffect(GlobalEffect::LaunchDebugScene {
                        name: name.to_owned(),
                    })];
                }
            }
        }
        Vec::new()
    }
}

impl Default for DeveloperScene {
    fn default() -> Self {
        Self::new()
    }
}

impl GameScene for DeveloperScene {
    fn update(
        &mut self,
        time: &Time,
        input_context: &InputContext,
        asset_cache: &mut AssetCache,
        game_options: &GameOptions,
        _command_effects: Vec<Effect>,
    ) -> Vec<Effect> {
        if let Ok(mut world_time) = self.world.borrow::<UniqueViewMut<Time>>() {
            *world_time = time.clone();
        }

        // Both pages ride the backdrop's authored widget rects, so they are
        // resolved from the layout file rather than hardcoded (see
        // `dev_params_panel::PanelRects`).
        self.panel_rects = dev_params_panel::rects(asset_cache);
        let state = self.state();

        let action = self.menu.update(
            time.elapsed,
            input_context,
            game_options.presentation_mode,
            |point| hit(state, point),
            |point| hit(state, point),
        );

        match action {
            Some(action) => self.handle_action(action),
            None => Vec::new(),
        }
    }

    fn render(
        &mut self,
        asset_cache: &mut AssetCache,
        options: &GameOptions,
    ) -> (Vec<SceneObject>, Vector3<f32>, Quaternion<f32>) {
        let identity = Quaternion::new(1.0, 0.0, 0.0, 0.0);
        // In flat presentation the screen is drawn in screen space in
        // `render_per_eye` (which has the screen size); the 3D scene is empty.
        if options.presentation_mode != PresentationMode::Vr {
            return (Vec::new(), vec3(0.0, 0.0, 0.0), identity);
        }

        // In VR there is no screen to draw on, so the same canvas is presented
        // on a world-space panel in front of the player.
        let canvas = self.build_canvas(self.menu.pointer_canvas());
        let objects = self
            .menu
            .render_world_space(asset_cache, canvas, options.presentation_mode);
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
        // In VR the screen lives on a world-space panel drawn by `render`; a
        // screen-space copy here would paste the whole canvas over both eyes
        // and hide it.
        if options.presentation_mode == PresentationMode::Vr {
            return Vec::new();
        }
        let pointer_canvas = self.menu.screen_pointer_canvas(screen_size);
        let canvas = self.build_canvas(pointer_canvas);
        self.menu
            .render_screen_space(asset_cache, canvas, screen_size, options.presentation_mode)
    }

    fn handle_effects(
        &mut self,
        effects: Vec<Effect>,
        _global_context: &GlobalContext,
        _game_options: &GameOptions,
        asset_cache: &mut AssetCache,
        audio_context: &mut AudioContext<EntityId, String>,
    ) -> Vec<GlobalEffect> {
        self.menu.pump_sfx(asset_cache, audio_context);
        effects
            .into_iter()
            .filter_map(|e| match e {
                Effect::GlobalEffect(g) => Some(g),
                _ => None,
            })
            .collect()
    }

    fn on_exit(&mut self, audio_context: &mut AudioContext<EntityId, String>) {
        self.menu.stop_sfx(audio_context);
    }

    fn wants_pointer(&self) -> bool {
        true
    }

    fn get_hand_spotlights(&self, _options: &GameOptions) -> Vec<SpotLight> {
        Vec::new()
    }

    fn world(&self) -> &World {
        &self.world
    }

    fn scene_name(&self) -> &str {
        &self.scene_name
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dev_params;

    // The runtimes render at a 4:3 resolution, so PreserveAspect == stretch
    // and normalized coords map straight onto the 640x480 canvas.
    const SCREEN: Vector2<f32> = Vector2 { x: 800.0, y: 600.0 };

    /// The parameters page at `scroll`, on the decoded fallback rects.
    fn params_state(scroll: usize) -> ScreenState {
        ScreenState {
            page: DeveloperPage::Params,
            rects: PanelRects::default(),
            scroll,
            scene_scroll: 0,
            has_selection: true,
        }
    }

    /// The debug-scene launcher at `scene_scroll`.
    fn scenes_state(scene_scroll: usize, has_selection: bool) -> ScreenState {
        ScreenState {
            page: DeveloperPage::Scenes,
            rects: PanelRects::default(),
            scroll: 0,
            scene_scroll,
            has_selection,
        }
    }

    fn pointer_at(canvas_point: Vector2<f32>, pressed: bool) -> Option<Pointer2D> {
        Some(Pointer2D {
            position: vec2(canvas_point.x / CANVAS_W, canvas_point.y / CANVAS_H),
            pressed,
        })
    }

    /// A canvas point over the first parameter's `>` arrow, found through the
    /// same hit test the screen uses (no duplicated geometry in the test).
    fn first_increment_point() -> Vector2<f32> {
        let (id, _) = dev_params::all().next().expect("registry is non-empty");
        // Scan the canvas for the arrow; coarse 2px grid is plenty at 20px
        // button widths.
        for y in (0..480).step_by(2) {
            for x in (0..640).step_by(2) {
                let p = vec2(x as f32, y as f32);
                if dev_params_panel::hit(PanelRects::default(), 0, p)
                    == Some(DevParamsEvent::Increment(id))
                {
                    return p;
                }
            }
        }
        panic!("no increment arrow found on the canvas");
    }

    fn first_increment_action() -> DeveloperAction {
        let (id, _) = dev_params::all().next().unwrap();
        DeveloperAction::Param(DevParamsEvent::Increment(id))
    }

    /// The single `GlobalEffect` an action produced, if any.
    fn global_effect(effects: Vec<Effect>) -> Option<GlobalEffect> {
        effects.into_iter().find_map(|effect| match effect {
            Effect::GlobalEffect(global) => Some(global),
            _ => None,
        })
    }

    fn launched_scene(effects: Vec<Effect>) -> Option<String> {
        match global_effect(effects) {
            Some(GlobalEffect::LaunchDebugScene { name }) => Some(name),
            _ => None,
        }
    }

    #[test]
    fn rising_edge_over_an_arrow_yields_its_event() {
        let (event, last, _) = resolve_click(
            params_state(0),
            pointer_at(first_increment_point(), true),
            false,
            SCREEN,
        );
        assert_eq!(event, Some(first_increment_action()));
        assert!(last);
    }

    #[test]
    fn held_press_does_not_re_activate() {
        let (event, last, _) = resolve_click(
            params_state(0),
            pointer_at(first_increment_point(), true),
            true,
            SCREEN,
        );
        assert_eq!(event, None, "a held press must not step the value again");
        assert!(last);
    }

    #[test]
    fn a_press_held_from_the_previous_scene_does_not_click() {
        // The scene starts with `last_pressed = true`, so a trigger still held
        // from the main menu's "Developer" click cannot immediately step a
        // value (or leave through "Done", whose rect overlaps the menu's
        // "Quit" area on the shared canvas).
        let scene = DeveloperScene::new();
        let (event, _, _) = resolve_click(
            params_state(0),
            pointer_at(first_increment_point(), true),
            scene.menu.last_pressed(),
            SCREEN,
        );
        assert_eq!(event, None);
    }

    /// The frontend shell must own the cross-scene press latch. Keeping a
    /// second latch on this host is exactly how otherwise-identical screens
    /// drift when the shared pointer rules change.
    #[test]
    fn the_shared_frontend_shell_owns_the_entry_press_guard() {
        let scene = DeveloperScene::new();
        assert!(scene.menu.last_pressed());
    }

    /// A frame with no pointer must not re-arm the click edge: the scene is
    /// entered with `last_pressed = true` precisely because a trigger can
    /// still be held from the main menu's Developer click, and clearing the
    /// flag on a pointerless frame would let that same unbroken press fire the
    /// moment the pointer reappears.
    #[test]
    fn a_pointerless_frame_keeps_the_held_press_guard() {
        let scene = DeveloperScene::new();
        assert!(scene.menu.last_pressed());
        let (event, last, point) =
            resolve_click(params_state(0), None, scene.menu.last_pressed(), SCREEN);
        assert_eq!(event, None);
        assert_eq!(point, None);
        assert!(last, "a pointerless frame must carry the guard through");

        // ...so the press reappearing still resolves to nothing.
        let (event, _, _) = resolve_click(
            params_state(0),
            pointer_at(first_increment_point(), true),
            last,
            SCREEN,
        );
        assert_eq!(event, None);
    }

    #[test]
    fn click_on_bare_backdrop_does_nothing() {
        let (event, _, _) = resolve_click(
            params_state(0),
            pointer_at(vec2(50.0, 50.0), true),
            false,
            SCREEN,
        );
        assert_eq!(event, None);
    }

    #[test]
    fn a_vr_ray_maps_onto_the_panel_buttons() {
        let point = first_increment_point();
        let hand = crate::ui::test_support::hand_aimed_at(vec2(CANVAS_W, CANVAS_H), point, 1.0);
        let input = InputContext {
            right_hand: hand,
            ..InputContext::default()
        };
        let pass = vr_frontend_pointer_pass(
            &input,
            vec2(CANVAS_W, CANVAS_H),
            &crate::ui::test_support::test_panel(),
        );
        let ray_point = pass.point().expect("the ray should land on the panel");
        assert_eq!(
            resolve_click_at(params_state(0), Some(ray_point), pass.pressed, false).0,
            Some(first_increment_action())
        );
    }

    #[test]
    fn world_supports_transition_save_data() {
        // Leaving the screen goes through the scene-swap path, which calls
        // `to_save_data` on the outgoing world; it must carry the uniques.
        let scene = DeveloperScene::new();
        let _ = crate::save_load::to_save_data(scene.world());
    }

    // ---------------------------------------------------------------------
    // The debug-scene launcher.
    // ---------------------------------------------------------------------

    /// The upper framed button (`GAMELODR.BIN` rect 2) - the load screen's
    /// "Load" frame, which this screen spends on the launcher.
    fn action_point() -> Vector2<f32> {
        PanelRects::default().action_rect().center()
    }

    #[test]
    fn the_upper_framed_button_opens_the_launcher() {
        assert_eq!(
            hit(params_state(0), action_point()),
            Some(DeveloperAction::OpenScenes)
        );
        // ...and it really is the authored rect, not a hand-placed one.
        assert_eq!(
            PanelRects::default().action_rect(),
            Rect::new(527.0, 161.0, 96.0, 62.0)
        );
        // A click there opens the page rather than reaching `Game`.
        let mut scene = DeveloperScene::new();
        assert!(scene.handle_action(DeveloperAction::OpenScenes).is_empty());
        assert_eq!(scene.page, DeveloperPage::Scenes);
    }

    #[test]
    fn every_debug_scene_is_reachable_and_carries_its_own_name() {
        let names: Vec<&str> = super::super::debug_scene_names().collect();
        assert!(!names.is_empty(), "there are debug scenes to launch");
        let rects = PanelRects::default();
        for (index, name) in names.iter().enumerate() {
            // Scrolling to a scene's own index always shows it (clamped when
            // it is inside the last page).
            let rows = scene_visible_rows(rects, index);
            assert!(rows.contains(&index), "scene {name} is off screen");
            let row = scene_row_rect(rects, index - rows.start);
            assert_eq!(
                hit(scenes_state(index, true), row.center()),
                Some(DeveloperAction::SelectScene(index)),
                "row for {name}"
            );

            // ...and selecting it launches *that* scene.
            let mut scene = DeveloperScene::new();
            scene.handle_action(DeveloperAction::SelectScene(index));
            assert_eq!(
                launched_scene(scene.handle_action(DeveloperAction::LaunchScene)),
                Some((*name).to_owned()),
                "launch of {name}"
            );
        }
    }

    /// The bug a scrolling list invites: the rows move but the hit test still
    /// indexes positionally, so clicking a row launches whatever scene *used*
    /// to be there.
    #[test]
    fn the_launcher_hit_test_follows_its_scroll() {
        let rects = PanelRects::default();
        let max = scene_max_scroll(rects);
        assert!(max > 0, "the shipped scene list must scroll to test this");
        for scroll in 1..=max {
            let slot0 = scene_row_rect(rects, 0);
            assert_eq!(
                hit(scenes_state(scroll, true), slot0.center()),
                Some(DeveloperAction::SelectScene(scroll)),
                "top row at scroll {scroll}"
            );
        }
    }

    #[test]
    fn the_launchers_rocker_scrolls_and_stops_at_both_ends() {
        let rects = PanelRects::default();
        let rocker = scene_rocker(rects).expect("the shipped scene list scrolls");
        let max = scene_max_scroll(rects);

        // At the top the "Up" half is inert; at the bottom, "Dn" is.
        assert_eq!(hit(scenes_state(0, true), rocker.up.center()), None);
        assert_eq!(
            hit(scenes_state(0, true), rocker.down.center()),
            Some(DeveloperAction::ScrollScenes(ScrollHalf::Down))
        );
        assert_eq!(hit(scenes_state(max, true), rocker.down.center()), None);
        assert_eq!(
            hit(scenes_state(max, true), rocker.up.center()),
            Some(DeveloperAction::ScrollScenes(ScrollHalf::Up))
        );

        let mut scene = DeveloperScene::new();
        scene.page = DeveloperPage::Scenes;
        for _ in 0..max + 3 {
            scene.handle_action(DeveloperAction::ScrollScenes(ScrollHalf::Down));
        }
        assert_eq!(scene.scene_scroll, max);
        scene.handle_action(DeveloperAction::ScrollScenes(ScrollHalf::Up));
        assert_eq!(scene.scene_scroll, max - 1);

        // A row's text must never run under the rocker it shares the pane with.
        assert!(scene_text_rect(rects, 0).x + scene_text_rect(rects, 0).w <= rocker.up.x);
    }

    #[test]
    fn launch_requires_a_selection() {
        assert_eq!(
            hit(scenes_state(0, true), action_point()),
            Some(DeveloperAction::LaunchScene)
        );
        // With nothing selected the button is inert rather than launching
        // whatever happens to be first.
        assert_eq!(hit(scenes_state(0, false), action_point()), None);
        let mut scene = DeveloperScene::new();
        scene.selected_scene = None;
        assert!(scene.handle_action(DeveloperAction::LaunchScene).is_empty());
    }

    #[test]
    fn done_leaves_the_launcher_for_the_parameters_not_the_main_menu() {
        let done = PanelRects::default().done_rect().center();
        assert_eq!(
            hit(scenes_state(0, true), done),
            Some(DeveloperAction::CloseScenes)
        );
        let mut scene = DeveloperScene::new();
        scene.page = DeveloperPage::Scenes;
        assert!(
            scene.handle_action(DeveloperAction::CloseScenes).is_empty(),
            "leaving the launcher must not swap the scene"
        );
        assert_eq!(scene.page, DeveloperPage::Params);

        // ...while "Done" on the parameters page still returns to the menu.
        assert!(matches!(
            global_effect(scene.handle_action(DeveloperAction::Param(DevParamsEvent::Done))),
            Some(GlobalEffect::ShowMainMenu)
        ));
    }

    /// The parameter rows belong to the other page: on the launcher, the pane
    /// is a list of scenes and nothing in it steps a value.
    #[test]
    fn the_pages_do_not_share_their_widgets() {
        let arrow = first_increment_point();
        assert!(matches!(
            hit(params_state(0), arrow),
            Some(DeveloperAction::Param(_))
        ));
        assert!(!matches!(
            hit(scenes_state(0, true), arrow),
            Some(DeveloperAction::Param(_))
        ));
        // And the launcher's rows are not on the parameters page.
        let row = scene_row_rect(PanelRects::default(), 0);
        assert!(!matches!(
            hit(params_state(0), row.center()),
            Some(DeveloperAction::SelectScene(_))
        ));
    }

    /// The VR controller ray resolves through the very same hit test as the
    /// flat pointer, so the launcher's widgets land in the same place in both
    /// presentations (AGENTS.md §3).
    #[test]
    fn a_vr_ray_can_open_the_launcher_and_launch_a_scene() {
        let aim = |point: Vector2<f32>| {
            let hand = crate::ui::test_support::hand_aimed_at(vec2(CANVAS_W, CANVAS_H), point, 1.0);
            let input = InputContext {
                right_hand: hand,
                ..InputContext::default()
            };
            let pass = vr_frontend_pointer_pass(
                &input,
                vec2(CANVAS_W, CANVAS_H),
                &crate::ui::test_support::test_panel(),
            );
            (
                pass.point().expect("the ray should land on the panel"),
                pass.pressed,
            )
        };

        let (point, pressed) = aim(action_point());
        assert_eq!(
            resolve_click_at(params_state(0), Some(point), pressed, false).0,
            Some(DeveloperAction::OpenScenes)
        );
        assert_eq!(
            resolve_click_at(scenes_state(0, true), Some(point), pressed, false).0,
            Some(DeveloperAction::LaunchScene)
        );

        // A row on the launcher, through the same ray.
        let row = scene_row_rect(PanelRects::default(), 2).center();
        let (point, pressed) = aim(row);
        assert_eq!(
            resolve_click_at(scenes_state(0, true), Some(point), pressed, false).0,
            Some(DeveloperAction::SelectScene(2))
        );
    }

    /// Every launcher row - and both framed buttons - must sit inside the
    /// backdrop's own art, clear of the painted field the rows stop above.
    #[test]
    fn the_launchers_rows_stay_inside_the_pane() {
        let rects = PanelRects::default();
        let list = rects.list_rect();
        let rows = scene_visible_rows(rects, 0);
        assert!(rows.len() >= 10, "the pane should show a useful page");
        let last = scene_row_rect(rects, rows.len() - 1);
        assert!(last.y + last.h <= FIELD_TOP_Y);
        assert!(last.y + last.h <= list.y + list.h);
        assert_eq!(scene_row_rect(rects, 0).x, list.x);
    }
}
