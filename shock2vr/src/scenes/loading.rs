//! Loading screen.
//!
//! A `GameScene` that draws the original animated loading screen while a level
//! loads, so the window keeps presenting frames instead of freezing on a black
//! frame (projects/loading-screen.md). The original screen is a 3-layer composite,
//! all shipped in `intrface.crf`:
//!
//! 1. `LOADING.PCX` — the 640x480 backdrop (frame + bracket).
//! 2. `meters/LOADA_01..20.PCX` — a 272x272 center disc, cycled for the rotation.
//! 3. `meters/PROGRESS.PCX` — a 246x20 bar, clipped 0..1 as the "% Transfer
//!    Completed" fill.
//!
//! The rotation is driven by the frame clock (always animates). The bar fill is
//! driven by `progress`: `new_demo()` sweeps it for visual inspection via the
//! `debug_loading` scene; a real transition sets it from the deferred-transition
//! checkpoints (`Game::update` via `set_progress`).

use cgmath::{Quaternion, Vector2, Vector3, vec2, vec3};
use dark::{importers::UI_LAYOUT_IMPORTER, map::MapRect};
use engine::{
    assets::asset_cache::AssetCache,
    audio::AudioContext,
    scene::{SceneObject, light::SpotLight},
};
use shipyard::{EntityId, UniqueViewMut, World};

use crate::{
    GameOptions,
    game_scene::GameScene,
    input_context::InputContext,
    mission::GlobalContext,
    scripts::{Effect, GlobalEffect},
    time::Time,
    ui::{FrontendCanvasPresenter, FrontendPanelAnchor, Rect, ScaleMode, UiCanvas},
};

/// The loading art is authored on the original 640x480 `LOADING.PCX` canvas.
const CANVAS_W: f32 = 640.0;
const CANVAS_H: f32 = 480.0;
const BACKDROP_TEXTURE: &str = "LOADING.PCX";
const PROGRESS_TEXTURE: &str = "meters/PROGRESS.PCX";
/// The 4:3 art is letterboxed (not stretched) on non-4:3 windows.
const SCALE_MODE: ScaleMode = ScaleMode::PreserveAspect;

/// Original widget layout for the loading screen — a list of LTRB rects (`UI_LAYOUT_IMPORTER`).
/// Index order is the screen's convention: `[0]` = disc, `[1]` = bar, `[2]` = status text.
const LAYOUT_FILE: &str = "loadingr.BIN";
const DISC_RECT_INDEX: usize = 0;
const BAR_RECT_INDEX: usize = 1;
/// Fallbacks if `loadingr.BIN` is missing — the values decoded from it.
const DISC_FALLBACK: Rect = Rect::new(184.0, 120.0, 272.0, 272.0);
const BAR_FALLBACK: Rect = Rect::new(197.0, 394.0, 246.0, 20.0);

const LOADA_FRAME_COUNT: u32 = 20;
/// Rotation speed; 20 frames at 15 fps -> a full cycle every ~1.3s.
const LOADA_FPS: f32 = 15.0;

/// Convert a `loadingr.BIN` rect at `index` to a canvas `Rect`, or `fallback` if absent.
fn layout_rect(layout: Option<&[MapRect]>, index: usize, fallback: Rect) -> Rect {
    match layout.and_then(|rects| rects.get(index)) {
        Some(r) => Rect::new(
            r.ul_x as f32,
            r.ul_y as f32,
            r.width() as f32,
            r.height() as f32,
        ),
        None => fallback,
    }
}

/// Demo sweep period (seconds) for the `debug_loading` scene: fill 0->1, repeat.
const DEMO_FILL_SECS: f32 = 4.0;

pub struct LoadingScene {
    world: World,
    scene_name: String,
    /// Load progress in 0..=1 (drives the bar fill).
    progress: f32,
    /// Total elapsed seconds (drives the rotation, independent of progress).
    elapsed_secs: f32,
    /// When true, `progress` is swept from `elapsed_secs` for visual inspection.
    demo: bool,
    /// Where the VR panel is. Unused in flat presentation.
    panel_anchor: FrontendPanelAnchor,
}

impl LoadingScene {
    /// A loading scene whose bar is driven externally via [`set_progress`].
    pub fn new() -> Self {
        Self::build(false)
    }

    /// A self-animating loading scene for the `debug_loading` inspection scene.
    pub fn new_demo() -> Self {
        Self::build(true)
    }

    fn build(demo: bool) -> Self {
        let world = super::ui_scene_world();

        Self {
            world,
            scene_name: "loading".to_owned(),
            progress: 0.0,
            elapsed_secs: 0.0,
            demo,
            panel_anchor: FrontendPanelAnchor::new(),
        }
    }

    /// Set the load progress (0..=1). Driven by the deferred transition's checkpoints.
    pub fn set_progress(&mut self, progress: f32) {
        self.progress = progress.clamp(0.0, 1.0);
    }

    /// The `meters/LOADA_NN.PCX` frame name for the current rotation phase.
    fn current_disc_frame(&self) -> String {
        let frame = ((self.elapsed_secs * LOADA_FPS) as u32 % LOADA_FRAME_COUNT) + 1;
        format!("meters/LOADA_{frame:02}.PCX")
    }

    /// The single layout pass. Both presentations map this same canvas, so
    /// neither can place the disc or the bar differently (AGENTS.md "UI Renders
    /// Identically in Flatscreen and VR").
    ///
    /// `layout` is the decoded `loadingr.BIN` rects, or `None` to use the
    /// fallbacks — taken as an argument rather than read from the asset cache
    /// here so the layout is testable without one.
    fn build_canvas(&self, layout: Option<&[MapRect]>) -> UiCanvas {
        let disc_rect = layout_rect(layout, DISC_RECT_INDEX, DISC_FALLBACK);
        let bar_rect = layout_rect(layout, BAR_RECT_INDEX, BAR_FALLBACK);

        let mut canvas = UiCanvas::new(vec2(CANVAS_W, CANVAS_H));

        // 1. Full-screen backdrop.
        canvas.image(Rect::new(0.0, 0.0, CANVAS_W, CANVAS_H), BACKDROP_TEXTURE);

        // 2. Rotating center disc (cycled LOADA frames).
        canvas.image(disc_rect, &self.current_disc_frame());

        // 3. Progress bar fill (clipped 0..1).
        canvas.bar(bar_rect, PROGRESS_TEXTURE, self.progress);

        canvas
    }

    /// The canvas for this frame, with the widget rects from the original
    /// `loadingr.BIN` layout (cached by the asset cache after the first load).
    fn build_canvas_from_cache(&self, asset_cache: &mut AssetCache) -> UiCanvas {
        let layout = asset_cache.get_opt(&UI_LAYOUT_IMPORTER, LAYOUT_FILE);
        self.build_canvas(layout.as_deref().map(|rects| rects.as_slice()))
    }
}

impl Default for LoadingScene {
    fn default() -> Self {
        Self::new()
    }
}

impl GameScene for LoadingScene {
    fn update(
        &mut self,
        time: &Time,
        input_context: &InputContext,
        _asset_cache: &mut AssetCache,
        _game_options: &GameOptions,
        _command_effects: Vec<Effect>,
    ) -> Vec<Effect> {
        if let Ok(mut world_time) = self.world.borrow::<UniqueViewMut<Time>>() {
            *world_time = time.clone();
        }

        self.panel_anchor.update(
            input_context.head.position,
            input_context.head.rotation,
            time.elapsed,
        );

        self.elapsed_secs += time.elapsed.as_secs_f32();
        if self.demo {
            self.progress = (self.elapsed_secs / DEMO_FILL_SECS).rem_euclid(1.0);
        }

        Vec::new()
    }

    fn render(
        &mut self,
        asset_cache: &mut AssetCache,
        options: &GameOptions,
    ) -> (Vec<SceneObject>, Vector3<f32>, Quaternion<f32>) {
        let identity = Quaternion::new(1.0, 0.0, 0.0, 0.0);
        let panel = self.panel_anchor.panel();
        let canvas = self.build_canvas_from_cache(asset_cache);
        let objects = FrontendCanvasPresenter::new(options.presentation_mode, SCALE_MODE)
            .render_world_space(asset_cache, &canvas, &panel, None);
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
        let canvas = self.build_canvas_from_cache(asset_cache);
        FrontendCanvasPresenter::new(options.presentation_mode, SCALE_MODE).render_screen_space(
            asset_cache,
            &canvas,
            screen_size,
        )
    }

    fn handle_effects(
        &mut self,
        effects: Vec<Effect>,
        _global_context: &GlobalContext,
        _game_options: &GameOptions,
        _asset_cache: &mut AssetCache,
        _audio_context: &mut AudioContext<EntityId, String>,
    ) -> Vec<GlobalEffect> {
        effects
            .into_iter()
            .filter_map(|e| match e {
                Effect::GlobalEffect(g) => Some(g),
                _ => None,
            })
            .collect()
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

    /// So `Game::update` can reach [`LoadingScene::set_progress`] through the
    /// active-scene box during a deferred transition.
    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        Some(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_progress_clamps() {
        let mut scene = LoadingScene::new();
        scene.set_progress(1.5);
        assert_eq!(scene.progress, 1.0);
        scene.set_progress(-0.2);
        assert_eq!(scene.progress, 0.0);
    }

    #[test]
    fn disc_frame_cycles_through_all_20() {
        let mut scene = LoadingScene::new();
        // frame 1 at t=0
        scene.elapsed_secs = 0.0;
        assert_eq!(scene.current_disc_frame(), "meters/LOADA_01.PCX");
        // frame 2 after one frame-period
        scene.elapsed_secs = 1.0 / LOADA_FPS;
        assert_eq!(scene.current_disc_frame(), "meters/LOADA_02.PCX");
        // wraps back to frame 1 after a full cycle
        scene.elapsed_secs = LOADA_FRAME_COUNT as f32 / LOADA_FPS;
        assert_eq!(scene.current_disc_frame(), "meters/LOADA_01.PCX");
    }

    /// The canvas is what the VR panel presents. Before the panel existed the
    /// screen was built inline in `render_per_eye`, so in VR there was nothing
    /// to present at all - this is the shared artifact both presentations map.
    #[test]
    fn the_canvas_carries_the_backdrop_disc_and_bar() {
        let scene = LoadingScene::new();
        let canvas = scene.build_canvas(None);
        assert_eq!(canvas.element_count(), 3);
    }

    /// Placement is decided once, in canvas pixels: the same rects must reach
    /// both presentations, so neither can move the disc or the bar.
    #[test]
    fn the_canvas_places_the_widgets_at_the_layout_rects() {
        let scene = LoadingScene::new();
        let rects: Vec<_> = scene
            .build_canvas(None)
            .elements()
            .iter()
            .map(|e| e.rect())
            .collect();
        assert_eq!(rects[0], Rect::new(0.0, 0.0, CANVAS_W, CANVAS_H));
        assert_eq!(rects[1], DISC_FALLBACK);
        assert_eq!(rects[2], BAR_FALLBACK);
    }

    #[test]
    fn world_supports_transition_save_data() {
        let scene = LoadingScene::new();
        let _ = crate::save_load::to_save_data(scene.world());
    }
}
