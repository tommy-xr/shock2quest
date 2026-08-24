//! Game-over screen.
//!
//! Where terminal death lands. In the original, dying with no active
//! Quantum Bio-Reconstruction machine ends the run and hands the player back
//! to the Tri-Optimum archive database (the load-game screen) to pick a save;
//! this screen is that destination in this port's idiom - the original
//! `GAMELOD.PCX` backdrop and `GAMELODR.BIN` widget rects, showing the death
//! message plus the most recent save, with "Load" and "Quit".
//!
//! Structurally it is a sibling of [`crate::scenes::MainMenuScene`]: a pointer
//! driven `GameScene` that emits a `GlobalEffect` on click.

use cgmath::{Quaternion, Vector2, Vector3, vec2, vec3};
#[cfg(test)]
use dark::map::MapRect;
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
    mission::{GlobalContext, PlayerLifeState},
    save_load::{SaveFile, latest_save},
    scripts::{Effect, GlobalEffect},
    time::Time,
    ui::{FrontendMenu, HAlign, Rect, ScaleMode, UiCanvas, VAlign},
};

#[cfg(test)]
use crate::{
    input_context::Pointer2D,
    ui::{
        resolve_click_at as shell_resolve_click_at, resolve_flat_click, resolve_menu_rects,
        vr_frontend_pointer_pass,
    },
};

/// The screen is authored on the original 640x480 `GAMELOD.PCX` canvas.
const CANVAS_W: f32 = 640.0;
const CANVAS_H: f32 = 480.0;
const BACKDROP_TEXTURE: &str = "GAMELOD.PCX";
/// The original load screen's widget layout - LTRB rects (`UI_LAYOUT_IMPORTER`).
const LAYOUT_FILE: &str = "GAMELODR.BIN";
/// Same display font the main menu uses (`res/intrface/METAFONT.FON`).
const MENU_FONT: &str = "metafont.fon";
/// The small in-game font the load screen draws save names in, so the one row
/// this screen shows reads as the same kind of data.
const LIST_FONT: &str = "mainfont.fon";
/// Height of that single row, and the horizontal padding on each of its edges -
/// matching the load screen's list geometry.
const LIST_ROW_HEIGHT: f32 = 19.0;
const LIST_TEXT_INSET: f32 = 8.0;
/// The 4:3 art is letterboxed (not stretched) on non-4:3 windows.
const SCALE_MODE: ScaleMode = ScaleMode::PreserveAspect;

/// Death message shown in the archive header.
const DEATH_MESSAGE: &str = "YOU HAVE DIED";
/// `GAMELOD.STR`'s `unused` string - shown when there is nothing to load.
const NO_SAVE_LABEL: &str = "< EMPTY >";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GameOverAction {
    /// Reload the most recent save and resume play.
    Load,
    /// Leave this game. The main menu, *not* the process: dying is not a
    /// reason to be thrown out of the application, and the menu is where the
    /// other recovery paths (a different save, a new game) live. On the Quest
    /// this was the difference between a death costing one click and costing
    /// a relaunch of the APK.
    Quit,
}

/// Indices into the `GAMELODR.BIN` rect list, in the screen's authored order.
const HEADER_RECT_INDEX: usize = 0;
const LIST_RECT_INDEX: usize = 1;
const LOAD_RECT_INDEX: usize = 2;
const QUIT_RECT_INDEX: usize = 3;

/// What a clicked button does. Split out of `update` so the mapping can be
/// asserted without standing up a scene - the Quit destination in particular
/// is easy to regress and expensive to notice, since noticing it means dying.
fn effects_for(action: Option<GameOverAction>, save: Option<&SaveFile>) -> Vec<Effect> {
    match action {
        Some(GameOverAction::Load) => save
            .map(|save| {
                Effect::GlobalEffect(GlobalEffect::Load {
                    file_name: save.path.to_string_lossy().into_owned(),
                })
            })
            .into_iter()
            .collect(),
        Some(GameOverAction::Quit) => vec![Effect::GlobalEffect(GlobalEffect::ShowMainMenu)],
        None => Vec::new(),
    }
}

/// Decoded `GAMELODR.BIN` values, used when the layout file is absent.
const FALLBACK_RECTS: [Rect; 4] = [
    Rect::new(261.0, 31.0, 202.0, 20.0),
    Rect::new(261.0, 54.0, 202.0, 290.0),
    Rect::new(527.0, 161.0, 96.0, 62.0),
    Rect::new(527.0, 405.0, 95.0, 62.0),
];

/// Resolve the screen's widget rects from `GAMELODR.BIN`, falling back to the
/// decoded values when it is absent.
#[cfg(test)]
fn screen_rects(layout: Option<&[MapRect]>) -> Vec<Rect> {
    resolve_menu_rects(layout, &FALLBACK_RECTS)
}

/// Shared click core: both presentations reduce to "a point on the canvas plus
/// a pressed flag", so the rising-edge rule and the hit regions live here once.
#[cfg(test)]
fn resolve_click_at(
    point: Option<Vector2<f32>>,
    pressed: bool,
    last_pressed: bool,
    rects: &[Rect],
    can_load: bool,
) -> (Option<GameOverAction>, bool) {
    shell_resolve_click_at(point, pressed, last_pressed, |point| {
        hit(point, rects, can_load)
    })
}

/// Pure click resolution: on a rising press edge over an enabled button,
/// return its action. Also returns the new `last_pressed` for the next frame.
/// `can_load` disables "Load" when no save exists, so the screen never offers
/// a recovery it cannot perform.
#[cfg(test)]
fn resolve_click(
    pointer: Option<Pointer2D>,
    last_pressed: bool,
    screen_size: Vector2<f32>,
    rects: &[Rect],
    can_load: bool,
) -> (Option<GameOverAction>, bool, Option<Vector2<f32>>) {
    resolve_flat_click(
        pointer,
        last_pressed,
        screen_size,
        vec2(CANVAS_W, CANVAS_H),
        SCALE_MODE,
        |point| hit(point, rects, can_load),
    )
}

/// The button at a canvas point, if any. Shared by the click and the rollover
/// sound so the two always agree on where a button is.
fn hit(point: Vector2<f32>, rects: &[Rect], can_load: bool) -> Option<GameOverAction> {
    if can_load && rects[LOAD_RECT_INDEX].contains(point) {
        Some(GameOverAction::Load)
    } else if rects[QUIT_RECT_INDEX].contains(point) {
        Some(GameOverAction::Quit)
    } else {
        None
    }
}

pub struct GameOverScene {
    world: World,
    scene_name: String,
    /// The save offered by "Load", resolved once when the screen opens so the
    /// label and the click agree.
    save: Option<SaveFile>,
    menu: FrontendMenu<GameOverAction>,
}

impl GameOverScene {
    pub fn new() -> Self {
        let world = super::ui_scene_world();
        // Keep the lifecycle signal (`/v1/info` player.life_state) honest once
        // the mission is gone: the run is over, not alive.
        world.add_unique(PlayerLifeState::GameOver);

        Self {
            world,
            scene_name: "game_over".to_owned(),
            save: latest_save(),
            menu: FrontendMenu::new(vec2(CANVAS_W, CANVAS_H), SCALE_MODE),
        }
    }
}

impl GameOverScene {
    /// The screen, described once. Screen-space and world-space presentation
    /// differ only in how this canvas is rendered, so the two can never drift
    /// apart in layout, labels, or which buttons look actionable.
    ///
    /// `pointer_canvas` is the hover position in canvas pixels, whatever
    /// produced it - the mouse or a VR controller ray.
    fn build_canvas(
        &self,
        asset_cache: &mut AssetCache,
        pointer_canvas: Option<Vector2<f32>>,
    ) -> UiCanvas {
        let mut canvas = UiCanvas::new(vec2(CANVAS_W, CANVAS_H));
        canvas.image(Rect::new(0.0, 0.0, CANVAS_W, CANVAS_H), BACKDROP_TEXTURE);

        let rects = self.menu.rects(asset_cache, LAYOUT_FILE, &FALLBACK_RECTS);

        canvas.text_native(
            rects[HEADER_RECT_INDEX],
            DEATH_MESSAGE,
            MENU_FONT,
            HAlign::Center,
            VAlign::Middle,
        );

        // The archive list holds the one save "Load" will restore (or the
        // original's empty-slot label when there is nothing to restore).
        let save_label = match &self.save {
            Some(save) => save.name.as_str(),
            None => NO_SAVE_LABEL,
        };
        let list = rects[LIST_RECT_INDEX];
        canvas.text_native_fit(
            // Inset on both edges so an ellipsized name stops short of the
            // panel rather than running flush with it.
            Rect::new(
                list.x + LIST_TEXT_INSET,
                list.y,
                (list.w - 2.0 * LIST_TEXT_INSET).max(0.0),
                LIST_ROW_HEIGHT,
            ),
            save_label,
            // Save names are player-authored and unbounded, so this row is
            // drawn the way the load screen draws its list: the small font,
            // ellipsized to the panel. In the screen's 20px MENU_FONT a
            // typical name spills clear across the backdrop art.
            LIST_FONT,
            HAlign::Center,
            VAlign::Middle,
        );

        for (index, label) in [(LOAD_RECT_INDEX, "LOAD"), (QUIT_RECT_INDEX, "QUIT")] {
            let rect = rects[index];
            let enabled = index != LOAD_RECT_INDEX || self.save.is_some();
            let hovered = enabled && pointer_canvas.is_some_and(|p| rect.contains(p));
            canvas
                .text_native(rect, label, MENU_FONT, HAlign::Center, VAlign::Middle)
                .opacity(match (enabled, hovered) {
                    (false, _) => 0.3,
                    (true, false) => 0.6,
                    (true, true) => 1.0,
                });
        }

        canvas
    }
}

impl Default for GameOverScene {
    fn default() -> Self {
        Self::new()
    }
}

impl GameScene for GameOverScene {
    fn update(
        &mut self,
        time: &Time,
        input_context: &InputContext,
        asset_cache: &mut AssetCache,
        game_options: &GameOptions,
        command_effects: Vec<Effect>,
    ) -> Vec<Effect> {
        if let Ok(mut world_time) = self.world.borrow::<UniqueViewMut<Time>>() {
            *world_time = time.clone();
        }

        // Discrete input actions (quick-load above all) must keep working here:
        // this screen is the only thing left, so swallowing them would restore
        // the dead end it exists to remove. Only global effects are meaningful
        // without a mission; the rest have nothing to act on.
        let mut effects: Vec<Effect> = command_effects
            .into_iter()
            .filter(|effect| matches!(effect, Effect::GlobalEffect(_)))
            .collect();

        let rects = self.menu.rects(asset_cache, LAYOUT_FILE, &FALLBACK_RECTS);
        let action = self.menu.update(
            time.elapsed,
            input_context,
            game_options.presentation_mode,
            |point| hit(point, &rects, self.save.is_some()),
            |point| hit(point, &rects, self.save.is_some()),
        );

        effects.extend(effects_for(action, self.save.as_ref()));
        effects
    }

    fn render(
        &mut self,
        asset_cache: &mut AssetCache,
        options: &GameOptions,
    ) -> (Vec<SceneObject>, Vector3<f32>, Quaternion<f32>) {
        let identity = Quaternion::new(1.0, 0.0, 0.0, 0.0);
        let canvas = self.build_canvas(asset_cache, self.menu.pointer_canvas());
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
        let pointer_canvas = self.menu.screen_pointer_canvas(screen_size);
        let canvas = self.build_canvas(asset_cache, pointer_canvas);
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

    /// The runtimes render 4:3, so PreserveAspect maps normalized coordinates
    /// straight onto the 640x480 canvas.
    const SCREEN: Vector2<f32> = Vector2 { x: 800.0, y: 600.0 };

    fn pointer_at(rect: Rect, pressed: bool) -> Option<Pointer2D> {
        let center = rect.center();
        Some(Pointer2D {
            position: vec2(center.x / CANVAS_W, center.y / CANVAS_H),
            pressed,
        })
    }

    #[test]
    fn clicking_load_with_a_save_reloads() {
        let rects = screen_rects(None);
        let (action, last, _) = resolve_click(
            pointer_at(rects[LOAD_RECT_INDEX], true),
            false,
            SCREEN,
            &rects,
            true,
        );
        assert_eq!(action, Some(GameOverAction::Load));
        assert!(last);
    }

    #[test]
    fn load_is_inert_without_a_save() {
        let rects = screen_rects(None);
        let (action, _, _) = resolve_click(
            pointer_at(rects[LOAD_RECT_INDEX], true),
            false,
            SCREEN,
            &rects,
            false,
        );
        assert_eq!(action, None);
    }

    /// Dying must not be able to close the game. On the Quest, `GlobalEffect::Quit`
    /// here meant every death ended the session and cost a relaunch of the APK -
    /// the screen's own doc calls this the "load/quit recovery path", and a
    /// recovery path that exits the process recovers nothing.
    #[test]
    fn quit_returns_to_the_main_menu_rather_than_closing_the_game() {
        let effects = effects_for(Some(GameOverAction::Quit), None);

        assert!(
            matches!(
                effects.as_slice(),
                [Effect::GlobalEffect(GlobalEffect::ShowMainMenu)]
            ),
            "game-over Quit should return to the main menu: {effects:?}"
        );
    }

    #[test]
    fn quit_is_always_available() {
        let rects = screen_rects(None);
        let (action, _, _) = resolve_click(
            pointer_at(rects[QUIT_RECT_INDEX], true),
            false,
            SCREEN,
            &rects,
            false,
        );
        assert_eq!(action, Some(GameOverAction::Quit));
    }

    #[test]
    fn held_press_does_not_re_activate() {
        let rects = screen_rects(None);
        let (action, last, _) = resolve_click(
            pointer_at(rects[LOAD_RECT_INDEX], true),
            true,
            SCREEN,
            &rects,
            true,
        );
        assert_eq!(action, None);
        assert!(last);
    }

    #[test]
    fn clicking_outside_the_buttons_does_nothing() {
        let rects = screen_rects(None);
        let (action, _, _) = resolve_click(
            pointer_at(rects[LIST_RECT_INDEX], true),
            false,
            SCREEN,
            &rects,
            true,
        );
        assert_eq!(action, None);
    }

    /// The panel the anchor places on scene entry from the default head pose -
    /// what `update` would have hit-tested against on the screen's first frame.
    fn test_panel() -> crate::ui::WorldPanel {
        crate::ui::test_support::test_panel()
    }

    /// A hand aimed at a canvas point on this screen's VR panel.
    fn hand_aimed_at(point: Vector2<f32>, trigger: f32) -> crate::input_context::Hand {
        crate::ui::test_support::hand_aimed_at(vec2(CANVAS_W, CANVAS_H), point, trigger)
    }

    fn vr_input(hand: crate::input_context::Hand) -> InputContext {
        InputContext {
            right_hand: hand,
            ..InputContext::default()
        }
    }

    #[test]
    fn a_vr_ray_can_press_load_and_quit() {
        let rects = screen_rects(None);
        for (index, expected) in [
            (LOAD_RECT_INDEX, GameOverAction::Load),
            (QUIT_RECT_INDEX, GameOverAction::Quit),
        ] {
            let pass = vr_frontend_pointer_pass(
                &vr_input(hand_aimed_at(rects[index].center(), 1.0)),
                vec2(CANVAS_W, CANVAS_H),
                &test_panel(),
            );
            let (point, pressed) = (pass.point(), pass.pressed);
            let point = point.expect("the ray should land on the panel");
            assert!(rects[index].contains(point));
            assert_eq!(
                resolve_click_at(Some(point), pressed, false, &rects, true).0,
                Some(expected)
            );
        }
    }

    #[test]
    fn a_vr_ray_over_load_is_inert_without_a_save() {
        let rects = screen_rects(None);
        let pass = vr_frontend_pointer_pass(
            &vr_input(hand_aimed_at(rects[LOAD_RECT_INDEX].center(), 1.0)),
            vec2(CANVAS_W, CANVAS_H),
            &test_panel(),
        );
        let (point, pressed) = (pass.point(), pass.pressed);
        assert_eq!(
            resolve_click_at(point, pressed, false, &rects, false).0,
            None
        );
    }

    #[test]
    fn a_trigger_held_from_the_death_that_opened_this_screen_does_not_click() {
        // The screen is entered straight out of gameplay, so the trigger that
        // was being fired can still be down on the first frame. Without the
        // constructor's `last_pressed: true` that reads as a rising edge over
        // whatever the ray happens to cross - up to and including Quit.
        let rects = screen_rects(None);
        let pass = vr_frontend_pointer_pass(
            &vr_input(hand_aimed_at(rects[QUIT_RECT_INDEX].center(), 1.0)),
            vec2(CANVAS_W, CANVAS_H),
            &test_panel(),
        );
        let (point, pressed) = (pass.point(), pass.pressed);
        assert!(pressed);
        let (action, last) = resolve_click_at(point, pressed, true, &rects, true);
        assert_eq!(action, None, "a carried-over press must not activate Quit");

        // Releasing and pressing again is a real click.
        let (_, last) = resolve_click_at(point, false, last, &rects, true);
        assert_eq!(
            resolve_click_at(point, true, last, &rects, true).0,
            Some(GameOverAction::Quit)
        );
    }

    #[test]
    fn a_vr_press_held_across_frames_clicks_once() {
        let rects = screen_rects(None);
        let point = Some(rects[QUIT_RECT_INDEX].center());
        let (action, last) = resolve_click_at(point, true, false, &rects, true);
        assert_eq!(action, Some(GameOverAction::Quit));
        assert_eq!(resolve_click_at(point, true, last, &rects, true).0, None);
    }

    #[test]
    fn screen_rects_prefer_the_layout_file() {
        let layout: Vec<MapRect> = (0..4)
            .map(|i| MapRect::new(10, i * 70, 110, i * 70 + 50))
            .collect();
        let rects = screen_rects(Some(&layout));
        assert_eq!(rects[LOAD_RECT_INDEX], Rect::new(10.0, 140.0, 100.0, 50.0));
        assert_eq!(screen_rects(None), FALLBACK_RECTS);
    }

    #[test]
    fn world_supports_transition_save_data() {
        // The transition machinery calls `to_save_data` on the outgoing scene's
        // world, so the game-over world must support it.
        let scene = GameOverScene::new();
        let _ = crate::save_load::to_save_data(scene.world());
    }
}
