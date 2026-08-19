//! Developer screen (live-tunable runtime parameters).
//!
//! The frontend host for [`crate::ui::dev_params_panel`]: the shared row
//! builder on the `GAMELOD.PCX` archive frame, reached from the main menu's
//! Developer entry (`GlobalEffect::ShowDeveloper`); "Done" returns there. The
//! pause overlay hosts the very same builder as its Developer page, so the
//! screen is identical whichever way it is reached.
//!
//! Structurally a sibling of [`crate::scenes::LoadGameScene`]: one canvas,
//! rendered screen-space when flat and on a world panel in VR, with the
//! shared rising-edge click rules.

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
    input_context::{InputContext, Pointer2D},
    mission::GlobalContext,
    scripts::{Effect, GlobalEffect},
    time::Time,
    ui::{
        FrontendPanelAnchor,
        FrontendPointerPass,
        FrontendSfx,
        PointerVisuals,
        Rect,
        ScaleMode,
        UiCanvas,
        VR_COMPONENT_Z_STEP,
        dev_params_panel,
        // The archive-database frame: a header line, a dark pane the rows sit
        // in, and framed button art for "Done" - the geometry the panel is
        // laid out against, owned by the panel so both hosts share it.
        dev_params_panel::{BACKDROP_TEXTURE, DevParamsEvent, PanelRects},
        pointer_to_canvas,
        vr_frontend_pointer_pass,
    },
};

/// The screen is authored on the original 640x480 canvas.
const CANVAS_W: f32 = 640.0;
const CANVAS_H: f32 = 480.0;
/// The 4:3 art is letterboxed (not stretched) on non-4:3 windows.
const SCALE_MODE: ScaleMode = ScaleMode::PreserveAspect;

/// Shared click core: both presentations reduce to "a point on the canvas plus
/// a pressed flag", so the rising-edge rule lives here once. The hit regions
/// themselves are the panel's ([`dev_params_panel::hit`]).
fn resolve_click_at(
    rects: PanelRects,
    point: Option<Vector2<f32>>,
    pressed: bool,
    last_pressed: bool,
) -> (Option<DevParamsEvent>, bool) {
    if !pressed || last_pressed {
        return (None, pressed);
    }
    (point.and_then(|p| dev_params_panel::hit(rects, p)), pressed)
}

/// Pure click resolution for the flat pointer.
fn resolve_click(
    rects: PanelRects,
    pointer: Option<Pointer2D>,
    last_pressed: bool,
    screen_size: Vector2<f32>,
) -> (Option<DevParamsEvent>, bool, Option<Vector2<f32>>) {
    match pointer {
        Some(p) => {
            let point = pointer_to_canvas(
                vec2(CANVAS_W, CANVAS_H),
                p.position,
                screen_size,
                SCALE_MODE,
            );
            let (event, pressed) = resolve_click_at(rects, point, p.pressed, last_pressed);
            (event, pressed, point)
        }
        // A frame with no pointer at all carries `last_pressed` through rather
        // than clearing it: clearing would re-arm the click edge under a
        // still-held button, so a trigger held across scene entry (which is
        // exactly why the scene starts with `last_pressed = true`) would fire
        // the moment the pointer reappears. Same rule the pause overlay keeps.
        None => (None, last_pressed, None),
    }
}

pub struct DeveloperScene {
    world: World,
    scene_name: String,
    /// Pointer from the latest update, used for hover highlighting in render.
    pointer: Option<Pointer2D>,
    /// Where the VR controller ray last met the panel, in canvas pixels.
    vr_pointer_canvas: Option<Vector2<f32>>,
    /// The pointer pass that hit-tested this frame, kept so `render` draws the
    /// beams and dot from the very rays `update` resolved the highlight from.
    vr_pointer: FrontendPointerPass,
    /// The drawn half of that pointer (hands, beams, dot).
    vr_pointer_visuals: PointerVisuals,
    /// Where the VR panel is anchored: placed from the head on scene entry
    /// and world-locked after that.
    panel_anchor: FrontendPanelAnchor,
    /// Whether the pointer was pressed last frame (for rising-edge clicks).
    last_pressed: bool,
    /// Screen size from the latest render, so `update` can map the pointer
    /// into canvas space consistently with how the canvas is drawn.
    last_screen_size: Vector2<f32>,
    /// The frontend's hum, rollover and select sounds.
    sfx: FrontendSfx<DevParamsEvent>,
    /// The panel's widget rects, re-resolved from `GAMELODR.BIN` each update
    /// (the render path takes `&self`, so it reads the resolved value here).
    panel_rects: PanelRects,
}

impl DeveloperScene {
    pub fn new() -> Self {
        Self {
            world: super::ui_scene_world(),
            scene_name: "developer".to_owned(),
            pointer: None,
            vr_pointer_canvas: None,
            vr_pointer: FrontendPointerPass::default(),
            vr_pointer_visuals: PointerVisuals::new(),
            panel_anchor: FrontendPanelAnchor::new(),
            // A press held across a scene swap must not read as a click here:
            // every frontend screen sits on the same 640x480 canvas and their
            // widgets overlap, so starting "already pressed" makes the next
            // rising edge require a real release first.
            last_pressed: true,
            last_screen_size: vec2(CANVAS_W, CANVAS_H),
            sfx: FrontendSfx::new(),
            panel_rects: PanelRects::default(),
        }
    }

    /// The screen, described once; presentations differ only in how this
    /// canvas is rendered.
    fn build_canvas(&self, pointer_canvas: Option<Vector2<f32>>) -> UiCanvas {
        let mut canvas = UiCanvas::new(vec2(CANVAS_W, CANVAS_H));
        canvas.image(Rect::new(0.0, 0.0, CANVAS_W, CANVAS_H), BACKDROP_TEXTURE);
        dev_params_panel::draw(&mut canvas, self.panel_rects, pointer_canvas);
        canvas
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

        // The rows ride the backdrop's authored widget rects, so they are
        // resolved from the layout file rather than hardcoded (see
        // `dev_params_panel::PanelRects`).
        self.panel_rects = dev_params_panel::rects(asset_cache);
        let rects = self.panel_rects;

        // The panel is placed from the head on scene entry and world-locked
        // after that; advancing it here keeps the ray and the render agreeing
        // on where the screen is, in either presentation.
        let panel = self.panel_anchor.update(
            input_context.head.position,
            input_context.head.rotation,
            time.elapsed,
        );

        let (event, last_pressed, point) = if game_options.presentation_mode == PresentationMode::Vr
        {
            // VR has no 2D cursor: the pointer is where a controller ray
            // meets the panel, and the trigger is the button.
            self.vr_pointer =
                vr_frontend_pointer_pass(input_context, vec2(CANVAS_W, CANVAS_H), &panel);
            let (point, pressed) = (self.vr_pointer.point(), self.vr_pointer.pressed);
            self.vr_pointer_canvas = point;
            self.pointer = None;
            let (event, last_pressed) = resolve_click_at(rects, point, pressed, self.last_pressed);
            (event, last_pressed, point)
        } else {
            self.pointer = input_context.pointer;
            self.vr_pointer = FrontendPointerPass::default();
            resolve_click(
                rects,
                input_context.pointer,
                self.last_pressed,
                self.last_screen_size,
            )
        };
        self.last_pressed = last_pressed;

        // Hover and click feedback from the same hit test that resolves the
        // click, so a sound plays exactly when a button lights up.
        self.sfx
            .hover(point.and_then(|p| dev_params_panel::hit(rects, p)));
        if event.is_some() {
            self.sfx.click();
        }

        if let Some(event) = event {
            // Steps are applied to the registry here; "Done" returns to the
            // main menu.
            if dev_params_panel::activate(event) {
                return vec![Effect::GlobalEffect(GlobalEffect::ShowMainMenu)];
            }
        }
        Vec::new()
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
        let panel = self.panel_anchor.panel();
        let canvas = self.build_canvas(self.vr_pointer_canvas);
        let mut objects = canvas.render_world_space(
            asset_cache,
            panel.transform(),
            self.vr_pointer_canvas,
            None,
            VR_COMPONENT_Z_STEP,
        );
        // The controllers and their aim rays, so the player can see where they
        // are pointing before a button lights up.
        let panel_layers = objects.len();
        objects.extend(self.vr_pointer_visuals.render(
            asset_cache,
            &self.vr_pointer,
            vec2(CANVAS_W, CANVAS_H),
            &panel,
            panel_layers,
        ));
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
        self.last_screen_size = screen_size;
        // In VR the screen lives on a world-space panel drawn by `render`; a
        // screen-space copy here would paste the whole canvas over both eyes
        // and hide it.
        if options.presentation_mode == PresentationMode::Vr {
            return Vec::new();
        }
        let pointer_canvas = self.pointer.and_then(|p| {
            pointer_to_canvas(
                vec2(CANVAS_W, CANVAS_H),
                p.position,
                screen_size,
                SCALE_MODE,
            )
        });
        let canvas = self.build_canvas(pointer_canvas);
        canvas.render_screen_space(asset_cache, screen_size, SCALE_MODE)
    }

    fn handle_effects(
        &mut self,
        effects: Vec<Effect>,
        _global_context: &GlobalContext,
        _game_options: &GameOptions,
        asset_cache: &mut AssetCache,
        audio_context: &mut AudioContext<EntityId, String>,
    ) -> Vec<GlobalEffect> {
        self.sfx.pump(asset_cache, audio_context);
        effects
            .into_iter()
            .filter_map(|e| match e {
                Effect::GlobalEffect(g) => Some(g),
                _ => None,
            })
            .collect()
    }

    fn on_exit(&mut self, audio_context: &mut AudioContext<EntityId, String>) {
        self.sfx.stop(audio_context);
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
                if dev_params_panel::hit(PanelRects::default(), p)
                    == Some(DevParamsEvent::Increment(id))
                {
                    return p;
                }
            }
        }
        panic!("no increment arrow found on the canvas");
    }

    #[test]
    fn rising_edge_over_an_arrow_yields_its_event() {
        let (id, _) = dev_params::all().next().unwrap();
        let (event, last, _) = resolve_click(
            PanelRects::default(),
            pointer_at(first_increment_point(), true),
            false,
            SCREEN,
        );
        assert_eq!(event, Some(DevParamsEvent::Increment(id)));
        assert!(last);
    }

    #[test]
    fn held_press_does_not_re_activate() {
        let (event, last, _) = resolve_click(
            PanelRects::default(),
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
            PanelRects::default(),
            pointer_at(first_increment_point(), true),
            scene.last_pressed,
            SCREEN,
        );
        assert_eq!(event, None);
    }

    /// A frame with no pointer must not re-arm the click edge: the scene is
    /// entered with `last_pressed = true` precisely because a trigger can
    /// still be held from the main menu's Developer click, and clearing the
    /// flag on a pointerless frame would let that same unbroken press fire the
    /// moment the pointer reappears.
    #[test]
    fn a_pointerless_frame_keeps_the_held_press_guard() {
        let scene = DeveloperScene::new();
        assert!(scene.last_pressed);
        let (event, last, point) =
            resolve_click(PanelRects::default(), None, scene.last_pressed, SCREEN);
        assert_eq!(event, None);
        assert_eq!(point, None);
        assert!(last, "a pointerless frame must carry the guard through");

        // ...so the press reappearing still resolves to nothing.
        let (event, _, _) = resolve_click(
            PanelRects::default(),
            pointer_at(first_increment_point(), true),
            last,
            SCREEN,
        );
        assert_eq!(event, None);
    }

    #[test]
    fn click_on_bare_backdrop_does_nothing() {
        let (event, _, _) = resolve_click(
            PanelRects::default(),
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
        let (id, _) = dev_params::all().next().unwrap();
        assert_eq!(
            resolve_click_at(PanelRects::default(), Some(ray_point), pass.pressed, false).0,
            Some(DevParamsEvent::Increment(id))
        );
    }

    #[test]
    fn world_supports_transition_save_data() {
        // Leaving the screen goes through the scene-swap path, which calls
        // `to_save_data` on the outgoing world; it must carry the uniques.
        let scene = DeveloperScene::new();
        let _ = crate::save_load::to_save_data(scene.world());
    }
}
