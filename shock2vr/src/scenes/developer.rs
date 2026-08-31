//! Developer screen (live-tunable runtime parameters, and the debug-scene
//! launcher).
//!
//! The frontend host for [`crate::ui::dev_params_panel`]: the shared row
//! builder on the `GAMELOD.PCX` archive frame, reached from the main menu's
//! Developer entry (`GlobalEffect::ShowDeveloper`); "Done" returns there. The
//! pause overlay hosts the very same builder as its Developer page, so the
//! screen is identical whichever way it is reached.
//!
//! The screen has a second page: a launcher, reached from the upper framed
//! button (the load screen's "Load" frame) and returning to the parameters
//! with its own "Done". It exists because a headset picks its scene from a
//! file read at startup, so without an in-game entry point every change of
//! scene - or every death inside one - costs an APK relaunch. Two tabs share
//! the one list: "Missions" enumerates the `*.mis` files in the data root and
//! launches through the same `TransitionLevel` the main menu's New Game uses;
//! "Debug Scenes" lists the registry `--mission debug_x` dispatches from
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
    /// The mission / debug-scene launcher.
    Scenes,
}

/// Which list the launcher shows. The tabs live in the header rect, so they
/// cost no pane space and the list machinery below them is shared.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SceneTab {
    /// The `*.mis` files in the data root.
    Missions,
    /// The debug-scene registry.
    DebugScenes,
}

impl SceneTab {
    const ALL: [SceneTab; 2] = [SceneTab::Missions, SceneTab::DebugScenes];

    /// Both labels together must fit the 202px header the tabs split -
    /// "Debug Scenes" measured ~139px in the menu font and overprinted its
    /// neighbour, hence the short form (drawn `_fit` besides, so a wide label
    /// can never spill into the other tab again).
    fn label(self) -> &'static str {
        match self {
            SceneTab::Missions => "Missions",
            SceneTab::DebugScenes => "Debug",
        }
    }
}

/// What a click on the screen asks for, whichever page it landed on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeveloperAction {
    /// Something on the parameter page: a step, its scroll, or "Done".
    Param(DevParamsEvent),
    /// Open the launcher.
    OpenScenes,
    /// Show this tab's list in the launcher.
    SelectTab(SceneTab),
    /// Highlight the entry at this index in the showing tab's list.
    SelectScene(usize),
    /// Launch the highlighted entry.
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
    /// How many entries the launcher's showing tab lists - the only thing the
    /// shared list geometry needs to know about the data source.
    list_len: usize,
    /// List index drawn in the launcher's top row.
    scene_scroll: usize,
    /// Whether a row is highlighted - "Launch" is inert without one,
    /// exactly as the load screen's "Load" is.
    has_selection: bool,
}

/// How many debug scenes the launcher lists.
fn scene_count() -> usize {
    super::debug_scene_names().count()
}

/// The launcher list's geometry - paging, the gutter rocker and the
/// slot-to-entry mapping - shared with every other frontend list.
fn scene_list(rects: PanelRects) -> list_scroll::ListGeometry {
    list_scroll::ListGeometry {
        pane: rects.list_rect(),
        bottom_limit: FIELD_TOP_Y,
        row_h: SCENE_ROW_H,
        text_inset: SCENE_TEXT_INSET,
    }
}

fn scene_visible_rows(
    rects: PanelRects,
    len: usize,
    scene_scroll: usize,
) -> std::ops::Range<usize> {
    scene_list(rects).visible_rows(len, scene_scroll)
}

/// The canvas rect of the tab header for `tab`: the header rect split in two,
/// so the tabs ride the backdrop's authored header line in both presentations.
fn tab_rect(rects: PanelRects, tab: SceneTab) -> Rect {
    let header = rects.header_rect();
    let half = header.w / 2.0;
    let x = match tab {
        SceneTab::Missions => header.x,
        SceneTab::DebugScenes => header.x + half,
    };
    Rect::new(x, header.y, half, header.h)
}

/// The canvas rect of the launcher's `slot`-th visible row. Production code
/// reaches rows through `scene_list(..).hit(..)`; the tests still name them.
#[cfg(test)]
fn scene_row_rect(rects: PanelRects, len: usize, slot: usize) -> Rect {
    scene_list(rects).row_rect(len, slot)
}

/// The rect a row's name is drawn in: the row, inset on both edges.
fn scene_text_rect(rects: PanelRects, len: usize, slot: usize) -> Rect {
    scene_list(rects).text_rect(len, slot)
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
            if let Some(tab) = SceneTab::ALL
                .into_iter()
                .find(|tab| tab_rect(state.rects, *tab).contains(point))
            {
                return Some(DeveloperAction::SelectTab(tab));
            }
            // "Launch" is inert with nothing selected rather than launching
            // whatever happens to be first.
            if state.has_selection && state.rects.action_rect().contains(point) {
                return Some(DeveloperAction::LaunchScene);
            }
            if state.rects.done_rect().contains(point) {
                return Some(DeveloperAction::CloseScenes);
            }
            // Rows carry the index of the entry scrolled into that slot, never
            // the slot itself: a positional index would launch whatever entry
            // *used* to be in the row the moment the list scrolls. That rule,
            // and the inert-rocker-end one, live in `list_scroll`.
            match scene_list(state.rects).hit(state.list_len, state.scene_scroll, point)? {
                list_scroll::ListHit::Row(index) => Some(DeveloperAction::SelectScene(index)),
                list_scroll::ListHit::Scroll(half) => Some(DeveloperAction::ScrollScenes(half)),
            }
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
    /// Which of the launcher's tabs is showing.
    tab: SceneTab,
    /// The `*.mis` files the Missions tab lists, enumerated from the data
    /// root when the launcher opens.
    missions: Vec<String>,
    /// Index of the entry drawn in the launcher's top row.
    scene_scroll: usize,
    /// The highlighted entry, as an index into the showing tab's list.
    selected_scene: Option<usize>,
}

/// The install's missions, sorted by name - loose `.mis` files on a classic
/// install, the archived `data/*.mis` entries on a 25AE one
/// ([`crate::data_files::mission_names`]). Read when the launcher opens, so a
/// changed install shows up without a restart.
fn mission_files() -> Vec<String> {
    crate::data_files::mission_names(&crate::paths::data_root())
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
            tab: SceneTab::Missions,
            missions: Vec::new(),
            scene_scroll: 0,
            selected_scene: None,
        }
    }

    /// How many entries the showing tab lists.
    fn tab_list_len(&self) -> usize {
        match self.tab {
            SceneTab::Missions => self.missions.len(),
            SceneTab::DebugScenes => scene_count(),
        }
    }

    /// The name of the showing tab's `index`-th entry.
    fn tab_name(&self, index: usize) -> Option<String> {
        match self.tab {
            SceneTab::Missions => self.missions.get(index).cloned(),
            SceneTab::DebugScenes => super::debug_scene_names().nth(index).map(str::to_owned),
        }
    }

    /// Reset the launcher's list to its top with the first entry highlighted -
    /// on opening it and on switching tabs, so a stale index can never point
    /// into the other tab's list.
    fn reset_list(&mut self) {
        self.scene_scroll = 0;
        self.selected_scene = (self.tab_list_len() > 0).then_some(0);
    }

    /// Everything a click on this frame depends on, in one value shared by the
    /// hit test and the draw.
    fn state(&self) -> ScreenState {
        ScreenState {
            page: self.page,
            rects: self.panel_rects,
            scroll: self.scroll,
            list_len: self.tab_list_len(),
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
                // The tabs ride the header line: the showing one at full
                // opacity, the other at the buttons' idle level.
                for tab in SceneTab::ALL {
                    let active =
                        self.tab == tab || hovered == Some(DeveloperAction::SelectTab(tab));
                    // `_fit` so a label stays inside its own half of the
                    // header - drawn wider it would overprint the other tab
                    // while the rect-based hit test kept the boundary.
                    canvas
                        .text_native_fit(
                            tab_rect(self.panel_rects, tab),
                            tab.label(),
                            MENU_FONT,
                            HAlign::Center,
                            VAlign::Middle,
                        )
                        .opacity(if active { ACTIVE_OPACITY } else { IDLE_OPACITY });
                }

                let len = self.tab_list_len();
                let rows = scene_visible_rows(self.panel_rects, len, self.scene_scroll);
                // The visible page's names, resolved once (not per row).
                let names: Vec<String> = match self.tab {
                    SceneTab::Missions => self.missions[rows.clone()].to_vec(),
                    SceneTab::DebugScenes => super::debug_scene_names()
                        .skip(rows.start)
                        .take(rows.len())
                        .map(str::to_owned)
                        .collect(),
                };
                for (slot, name) in names.iter().enumerate() {
                    canvas
                        .text_native_fit(
                            scene_text_rect(self.panel_rects, len, slot),
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

                if let Some(rocker) = scene_list(self.panel_rects).rocker(len) {
                    list_scroll::draw(
                        &mut canvas,
                        &rocker,
                        rows.start,
                        scene_list(self.panel_rects).max_scroll(len),
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
            DeveloperAction::OpenScenes => {
                self.page = DeveloperPage::Scenes;
                // The launcher always opens on the Missions tab, whatever tab
                // it was left on.
                self.tab = SceneTab::Missions;
                self.missions = mission_files();
                self.reset_list();
            }
            DeveloperAction::CloseScenes => self.page = DeveloperPage::Params,
            DeveloperAction::SelectTab(tab) => {
                if self.tab != tab {
                    self.tab = tab;
                    self.reset_list();
                }
            }
            DeveloperAction::SelectScene(index) => self.selected_scene = Some(index),
            DeveloperAction::ScrollScenes(half) => {
                let max = scene_list(self.panel_rects).max_scroll(self.tab_list_len());
                list_scroll::apply(half, &mut self.scene_scroll, max)
            }
            DeveloperAction::LaunchScene => {
                let Some(name) = self.selected_scene.and_then(|index| self.tab_name(index)) else {
                    return Vec::new();
                };
                return match self.tab {
                    // The very same effect the main menu's New Game emits, so
                    // launching from here is the new-game boot with the level
                    // swapped: default spawn, vitals from the destination map.
                    SceneTab::Missions => {
                        vec![Effect::GlobalEffect(GlobalEffect::new_game_transition(
                            name,
                        ))]
                    }
                    SceneTab::DebugScenes => {
                        vec![Effect::GlobalEffect(GlobalEffect::LaunchDebugScene {
                            name,
                        })]
                    }
                };
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
            list_len: scene_count(),
            scene_scroll: 0,
            has_selection: true,
        }
    }

    /// The launcher at `scene_scroll`, showing a list of `len` entries.
    fn scenes_state(scene_scroll: usize, has_selection: bool, len: usize) -> ScreenState {
        ScreenState {
            page: DeveloperPage::Scenes,
            rects: PanelRects::default(),
            scroll: 0,
            list_len: len,
            scene_scroll,
            has_selection,
        }
    }

    /// A launcher scene showing the debug-scene tab, as a click would reach
    /// it: opened (which enumerates missions), then tabbed over.
    fn debug_tab_scene() -> DeveloperScene {
        let mut scene = DeveloperScene::new();
        scene.handle_action(DeveloperAction::OpenScenes);
        scene.handle_action(DeveloperAction::SelectTab(SceneTab::DebugScenes));
        scene
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
        let len = names.len();
        for (index, name) in names.iter().enumerate() {
            // Scrolling to a scene's own index always shows it (clamped when
            // it is inside the last page).
            let rows = scene_visible_rows(rects, len, index);
            assert!(rows.contains(&index), "scene {name} is off screen");
            let row = scene_row_rect(rects, len, index - rows.start);
            assert_eq!(
                hit(scenes_state(index, true, len), row.center()),
                Some(DeveloperAction::SelectScene(index)),
                "row for {name}"
            );

            // ...and selecting it launches *that* scene.
            let mut scene = debug_tab_scene();
            scene.handle_action(DeveloperAction::SelectScene(index));
            assert_eq!(
                launched_scene(scene.handle_action(DeveloperAction::LaunchScene)),
                Some((*name).to_owned()),
                "launch of {name}"
            );
        }
    }

    #[test]
    fn every_mission_is_reachable_and_launches_its_own_file() {
        // The list is data the fs hands the scene, so the mapping is tested on
        // a fake install; the enumeration itself is thin (`mission_files`).
        let missions: Vec<String> = (1..=23).map(|n| format!("level{n:02}.mis")).collect();
        let rects = PanelRects::default();
        let len = missions.len();
        for (index, name) in missions.iter().enumerate() {
            let rows = scene_visible_rows(rects, len, index);
            assert!(rows.contains(&index), "mission {name} is off screen");
            let row = scene_row_rect(rects, len, index - rows.start);
            assert_eq!(
                hit(scenes_state(index, true, len), row.center()),
                Some(DeveloperAction::SelectScene(index)),
                "row for {name}"
            );

            let mut scene = DeveloperScene::new();
            scene.page = DeveloperPage::Scenes;
            scene.missions = missions.clone();
            scene.handle_action(DeveloperAction::SelectScene(index));
            match global_effect(scene.handle_action(DeveloperAction::LaunchScene)) {
                Some(GlobalEffect::TransitionLevel {
                    level_file,
                    loc,
                    entities_to_trigger,
                    ..
                }) => {
                    assert_eq!(&level_file, name);
                    assert_eq!(loc, None, "map-default spawn");
                    assert!(entities_to_trigger.is_empty());
                }
                other => panic!("launch of {name} produced {other:?}"),
            }
        }
    }

    /// The negative test the tabs demand: the very same Launch click must emit
    /// a mission transition on one tab and a debug-scene launch on the other -
    /// never the wrong one.
    #[test]
    fn launch_follows_the_showing_tab() {
        let mut scene = DeveloperScene::new();
        scene.page = DeveloperPage::Scenes;
        scene.missions = vec!["earth.mis".to_owned()];
        scene.reset_list();
        assert_eq!(scene.tab, SceneTab::Missions);
        let effect = global_effect(scene.handle_action(DeveloperAction::LaunchScene));
        assert!(
            matches!(
                effect,
                Some(GlobalEffect::TransitionLevel { ref level_file, .. }) if level_file == "earth.mis"
            ),
            "Missions tab must emit TransitionLevel, got {effect:?}"
        );

        scene.handle_action(DeveloperAction::SelectTab(SceneTab::DebugScenes));
        assert!(matches!(
            global_effect(scene.handle_action(DeveloperAction::LaunchScene)),
            Some(GlobalEffect::LaunchDebugScene { .. })
        ));
    }

    /// Switching tabs resets the scroll and the selection - a stale index
    /// would point into the other tab's list. Re-clicking the showing tab
    /// keeps the selection.
    #[test]
    fn the_tabs_share_the_header_and_reset_the_list() {
        let rects = PanelRects::default();
        let header = rects.header_rect();
        // Both tabs sit on the authored header line, side by side.
        for tab in SceneTab::ALL {
            let r = tab_rect(rects, tab);
            assert_eq!(r.y, header.y);
            assert_eq!(
                hit(scenes_state(0, true, scene_count()), r.center()),
                Some(DeveloperAction::SelectTab(tab)),
                "tab {tab:?}"
            );
        }
        // The halves are disjoint - a draw kept inside its own rect (`_fit`)
        // can therefore never overprint the other tab or its click target.
        let missions = tab_rect(rects, SceneTab::Missions);
        let debug = tab_rect(rects, SceneTab::DebugScenes);
        assert!(missions.x + missions.w <= debug.x);

        let mut scene = DeveloperScene::new();
        scene.page = DeveloperPage::Scenes;
        scene.missions = vec!["earth.mis".to_owned(), "station.mis".to_owned()];
        scene.reset_list();
        scene.handle_action(DeveloperAction::SelectScene(1));
        // Re-clicking the showing tab is a no-op.
        scene.handle_action(DeveloperAction::SelectTab(SceneTab::Missions));
        assert_eq!(scene.selected_scene, Some(1));
        // Switching resets to the other list's first entry.
        scene.handle_action(DeveloperAction::SelectTab(SceneTab::DebugScenes));
        assert_eq!(scene.tab, SceneTab::DebugScenes);
        assert_eq!(scene.scene_scroll, 0);
        assert_eq!(scene.selected_scene, Some(0));
    }

    /// Opening the launcher enumerates the install's missions and lands on the
    /// Missions tab with its first entry preselected - even when it was left
    /// on the Debug Scenes tab last time.
    #[test]
    fn opening_the_launcher_enumerates_missions() {
        let mut scene = DeveloperScene::new();
        scene.tab = SceneTab::DebugScenes;
        scene.handle_action(DeveloperAction::OpenScenes);
        assert_eq!(scene.tab, SceneTab::Missions);
        assert_eq!(scene.missions, mission_files());
        assert_eq!(
            scene.selected_scene,
            (!scene.missions.is_empty()).then_some(0)
        );
        // The enumeration is sorted and lists only `*.mis` files.
        let mut sorted = scene.missions.clone();
        sorted.sort_by_key(|name| name.to_ascii_lowercase());
        assert_eq!(scene.missions, sorted);
        assert!(
            scene
                .missions
                .iter()
                .all(|name| name.to_ascii_lowercase().ends_with(".mis"))
        );
    }

    /// The bug a scrolling list invites: the rows move but the hit test still
    /// indexes positionally, so clicking a row launches whatever scene *used*
    /// to be there.
    #[test]
    fn the_launcher_hit_test_follows_its_scroll() {
        let rects = PanelRects::default();
        let len = scene_count();
        let max = scene_list(rects).max_scroll(len);
        assert!(max > 0, "the shipped scene list must scroll to test this");
        for scroll in 1..=max {
            let slot0 = scene_row_rect(rects, len, 0);
            assert_eq!(
                hit(scenes_state(scroll, true, len), slot0.center()),
                Some(DeveloperAction::SelectScene(scroll)),
                "top row at scroll {scroll}"
            );
        }
    }

    #[test]
    fn the_launchers_rocker_scrolls_and_stops_at_both_ends() {
        let rects = PanelRects::default();
        let len = scene_count();
        let rocker = scene_list(rects)
            .rocker(len)
            .expect("the shipped scene list scrolls");
        let max = scene_list(rects).max_scroll(len);

        // At the top the up half is inert; at the bottom, the down half is.
        assert_eq!(hit(scenes_state(0, true, len), rocker.up.center()), None);
        assert_eq!(
            hit(scenes_state(0, true, len), rocker.down.center()),
            Some(DeveloperAction::ScrollScenes(ScrollHalf::Down))
        );
        assert_eq!(
            hit(scenes_state(max, true, len), rocker.down.center()),
            None
        );
        assert_eq!(
            hit(scenes_state(max, true, len), rocker.up.center()),
            Some(DeveloperAction::ScrollScenes(ScrollHalf::Up))
        );

        let mut scene = debug_tab_scene();
        for _ in 0..max + 3 {
            scene.handle_action(DeveloperAction::ScrollScenes(ScrollHalf::Down));
        }
        assert_eq!(scene.scene_scroll, max);
        scene.handle_action(DeveloperAction::ScrollScenes(ScrollHalf::Up));
        assert_eq!(scene.scene_scroll, max - 1);

        // A row's text must never run under the rocker it shares the pane with.
        let text = scene_text_rect(rects, len, 0);
        assert!(text.x + text.w <= rocker.up.x);
    }

    #[test]
    fn launch_requires_a_selection() {
        assert_eq!(
            hit(scenes_state(0, true, scene_count()), action_point()),
            Some(DeveloperAction::LaunchScene)
        );
        // With nothing selected the button is inert rather than launching
        // whatever happens to be first.
        assert_eq!(
            hit(scenes_state(0, false, scene_count()), action_point()),
            None
        );
        let mut scene = DeveloperScene::new();
        scene.selected_scene = None;
        assert!(scene.handle_action(DeveloperAction::LaunchScene).is_empty());
    }

    #[test]
    fn done_leaves_the_launcher_for_the_parameters_not_the_main_menu() {
        let done = PanelRects::default().done_rect().center();
        assert_eq!(
            hit(scenes_state(0, true, scene_count()), done),
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
            hit(scenes_state(0, true, scene_count()), arrow),
            Some(DeveloperAction::Param(_))
        ));
        // And the launcher's rows are not on the parameters page.
        let row = scene_row_rect(PanelRects::default(), scene_count(), 0);
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
            resolve_click_at(
                scenes_state(0, true, scene_count()),
                Some(point),
                pressed,
                false
            )
            .0,
            Some(DeveloperAction::LaunchScene)
        );

        // A row on the launcher, through the same ray.
        let row = scene_row_rect(PanelRects::default(), scene_count(), 2).center();
        let (point, pressed) = aim(row);
        assert_eq!(
            resolve_click_at(
                scenes_state(0, true, scene_count()),
                Some(point),
                pressed,
                false
            )
            .0,
            Some(DeveloperAction::SelectScene(2))
        );

        // ...and a tab, through the same ray - the parity the tabs must keep.
        let tab = tab_rect(PanelRects::default(), SceneTab::Missions).center();
        let (point, pressed) = aim(tab);
        assert_eq!(
            resolve_click_at(
                scenes_state(0, true, scene_count()),
                Some(point),
                pressed,
                false
            )
            .0,
            Some(DeveloperAction::SelectTab(SceneTab::Missions))
        );
    }

    /// Every launcher row - and both framed buttons - must sit inside the
    /// backdrop's own art, clear of the painted field the rows stop above.
    #[test]
    fn the_launchers_rows_stay_inside_the_pane() {
        let rects = PanelRects::default();
        let len = scene_count();
        let list = rects.list_rect();
        let rows = scene_visible_rows(rects, len, 0);
        assert!(rows.len() >= 10, "the pane should show a useful page");
        let last = scene_row_rect(rects, len, rows.len() - 1);
        assert!(last.y + last.h <= FIELD_TOP_Y);
        assert!(last.y + last.h <= list.y + list.h);
        assert_eq!(scene_row_rect(rects, len, 0).x, list.x);
    }
}
