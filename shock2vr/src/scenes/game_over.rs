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
    input_context::{InputContext, Pointer2D},
    mission::{GlobalContext, PlayerLifeState},
    save_load::{SaveFile, latest_save},
    scenes::frontend_sfx::FrontendSfx,
    scripts::{Effect, GlobalEffect},
    time::Time,
    ui::{HAlign, Rect, ScaleMode, UiCanvas, VAlign, pointer_to_canvas},
};

/// The screen is authored on the original 640x480 `GAMELOD.PCX` canvas.
const CANVAS_W: f32 = 640.0;
const CANVAS_H: f32 = 480.0;
const BACKDROP_TEXTURE: &str = "GAMELOD.PCX";
/// The original load screen's widget layout - LTRB rects (`UI_LAYOUT_IMPORTER`).
const LAYOUT_FILE: &str = "GAMELODR.BIN";
/// Same display font the main menu uses (`res/intrface/METAFONT.FON`).
const MENU_FONT: &str = "metafont.fon";
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
    Quit,
}

/// Indices into the `GAMELODR.BIN` rect list, in the screen's authored order.
const HEADER_RECT_INDEX: usize = 0;
const LIST_RECT_INDEX: usize = 1;
const LOAD_RECT_INDEX: usize = 2;
const QUIT_RECT_INDEX: usize = 3;

/// Decoded `GAMELODR.BIN` values, used when the layout file is absent.
const FALLBACK_RECTS: [Rect; 4] = [
    Rect::new(261.0, 31.0, 202.0, 20.0),
    Rect::new(261.0, 54.0, 202.0, 290.0),
    Rect::new(527.0, 161.0, 96.0, 62.0),
    Rect::new(527.0, 405.0, 95.0, 62.0),
];

/// Resolve the screen's widget rects from `GAMELODR.BIN`, falling back to the
/// decoded values when it is absent.
fn screen_rects(layout: Option<&[MapRect]>) -> [Rect; 4] {
    let mut rects = FALLBACK_RECTS;
    for (index, rect) in rects.iter_mut().enumerate() {
        if let Some(r) = layout.and_then(|rects| rects.get(index)) {
            *rect = Rect::new(
                r.ul_x as f32,
                r.ul_y as f32,
                r.width() as f32,
                r.height() as f32,
            );
        }
    }
    rects
}

/// Pure click resolution: on a rising press edge over an enabled button,
/// return its action. Also returns the new `last_pressed` for the next frame.
/// `can_load` disables "Load" when no save exists, so the screen never offers
/// a recovery it cannot perform.
fn resolve_click(
    pointer: Option<Pointer2D>,
    last_pressed: bool,
    screen_size: Vector2<f32>,
    rects: &[Rect; 4],
    can_load: bool,
) -> (Option<GameOverAction>, bool, Option<Vector2<f32>>) {
    let Some(p) = pointer else {
        return (None, false, None);
    };
    let canvas_point = pointer_to_canvas(
        vec2(CANVAS_W, CANVAS_H),
        p.position,
        screen_size,
        SCALE_MODE,
    );
    if !p.pressed || last_pressed {
        return (None, p.pressed, canvas_point);
    }

    let action = canvas_point.and_then(|c| hit(c, rects, can_load));
    (action, p.pressed, canvas_point)
}

/// The button at a canvas point, if any. Shared by the click and the rollover
/// sound so the two always agree on where a button is.
fn hit(point: Vector2<f32>, rects: &[Rect; 4], can_load: bool) -> Option<GameOverAction> {
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
    /// Pointer from the latest update, used for hover highlighting in render.
    pointer: Option<Pointer2D>,
    /// Whether the pointer was pressed last frame (for rising-edge clicks).
    last_pressed: bool,
    /// Screen size from the latest render, so `update` maps the pointer into
    /// canvas space consistently with how the canvas is drawn.
    last_screen_size: Vector2<f32>,
    /// The frontend's hum, rollover and select sounds.
    sfx: FrontendSfx<GameOverAction>,
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
            pointer: None,
            last_pressed: false,
            last_screen_size: vec2(CANVAS_W, CANVAS_H),
            sfx: FrontendSfx::new(),
        }
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
        _game_options: &GameOptions,
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

        let layout = asset_cache.get_opt(&UI_LAYOUT_IMPORTER, LAYOUT_FILE);
        let rects = screen_rects(layout.as_deref().map(|r| r.as_slice()));

        self.pointer = input_context.pointer;
        let (action, last_pressed, point) = resolve_click(
            input_context.pointer,
            self.last_pressed,
            self.last_screen_size,
            &rects,
            self.save.is_some(),
        );
        self.last_pressed = last_pressed;

        self.sfx
            .hover(point.and_then(|p| hit(p, &rects, self.save.is_some())));
        if action.is_some() {
            self.sfx.click();
        }

        match action {
            Some(GameOverAction::Load) => {
                if let Some(save) = &self.save {
                    effects.push(Effect::GlobalEffect(GlobalEffect::Load {
                        file_name: save.path.to_string_lossy().into_owned(),
                    }));
                }
            }
            Some(GameOverAction::Quit) => effects.push(Effect::GlobalEffect(GlobalEffect::Quit)),
            None => {}
        }
        effects
    }

    fn render(
        &mut self,
        _asset_cache: &mut AssetCache,
        _options: &GameOptions,
    ) -> (Vec<SceneObject>, Vector3<f32>, Quaternion<f32>) {
        // Drawn in screen space in `render_per_eye`; the 3D scene is empty.
        (
            Vec::new(),
            vec3(0.0, 0.0, 0.0),
            Quaternion::new(1.0, 0.0, 0.0, 0.0),
        )
    }

    fn render_per_eye(
        &mut self,
        asset_cache: &mut AssetCache,
        _view: cgmath::Matrix4<f32>,
        _projection: cgmath::Matrix4<f32>,
        screen_size: Vector2<f32>,
        _options: &GameOptions,
    ) -> Vec<SceneObject> {
        self.last_screen_size = screen_size;
        let mut canvas = UiCanvas::new(vec2(CANVAS_W, CANVAS_H));
        canvas.image(Rect::new(0.0, 0.0, CANVAS_W, CANVAS_H), BACKDROP_TEXTURE);

        let layout = asset_cache.get_opt(&UI_LAYOUT_IMPORTER, LAYOUT_FILE);
        let rects = screen_rects(layout.as_deref().map(|r| r.as_slice()));
        let pointer_canvas = self.pointer.and_then(|p| {
            pointer_to_canvas(
                vec2(CANVAS_W, CANVAS_H),
                p.position,
                screen_size,
                SCALE_MODE,
            )
        });

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
        canvas.text_native(
            Rect::new(list.x, list.y, list.w, 20.0),
            save_label,
            MENU_FONT,
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
