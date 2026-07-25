//! Flatscreen main menu.
//!
//! A minimal `GameScene` that draws the original `MAIN.PCX` backdrop with
//! mouse-clickable "New Game" / "Quit" items, described on the shared
//! [`UiCanvas`]. It reads `InputContext::pointer` (normalized screen coords) and
//! emits a `GlobalEffect` on click: New Game -> `TransitionLevel` into the first
//! mission, Quit -> `Quit`.
//!
//! See `projects/flatscreen-and-vr-architecture.md` (Slice 3).

use std::collections::HashMap;

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
    inventory::PlayerInventoryEntity,
    mission::{GlobalContext, GlobalEntityMetadata, GlobalTemplateIdMap, PlayerInfo},
    quest_info::QuestInfo,
    scripts::{Effect, GlobalEffect},
    time::Time,
    ui::{HAlign, Rect, ScaleMode, UiCanvas, VAlign, pointer_to_canvas},
};

/// Mission loaded when the player chooses "New Game".
const NEW_GAME_MISSION: &str = "earth.mis";

/// The menu is authored on the original 640x480 `MAIN.PCX` canvas.
const CANVAS_W: f32 = 640.0;
const CANVAS_H: f32 = 480.0;
/// The original menu draws its labels with the default GUI style font -
/// METAFONT.FON (`res/intrface/`), a 20px-cell antialias-16 display face.
const MENU_FONT: &str = "metafont.fon";
/// Original widget layout for `MAIN.PCX` - LTRB rects for the six buttons
/// (top to bottom) plus the corner logo (`UI_LAYOUT_IMPORTER`).
const LAYOUT_FILE: &str = "MAINR.BIN";
/// The 4:3 menu art is letterboxed (not stretched) on non-4:3 windows.
const SCALE_MODE: ScaleMode = ScaleMode::PreserveAspect;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MenuAction {
    NewGame,
    Quit,
}

struct MenuItem {
    label: &'static str,
    action: MenuAction,
    /// Index of this button's rect in the `MAINR.BIN` layout (six buttons top
    /// to bottom, then the corner logo).
    layout_index: usize,
    /// Hit/draw rect in canvas pixels if the layout file is absent (the decoded
    /// `MAINR.BIN` values: buttons at x=400, 179x60, 76px vertical pitch).
    fallback_rect: Rect,
}

// MAIN.PCX (native 640x480) has a vertical stack of six buttons down the right
// side; New Game = top button, Quit = bottom button. Labels are centered in the
// button rect.
const MENU_ITEMS: &[MenuItem] = &[
    MenuItem {
        label: "NEW GAME",
        action: MenuAction::NewGame,
        layout_index: 0,
        fallback_rect: Rect::new(400.0, 20.0, 179.0, 60.0),
    },
    MenuItem {
        label: "QUIT",
        action: MenuAction::Quit,
        layout_index: 5,
        fallback_rect: Rect::new(400.0, 400.0, 179.0, 60.0),
    },
];

/// Resolve each menu item's canvas rect from the `MAINR.BIN` layout (falling
/// back to the decoded values if it's absent). Parallel to [`MENU_ITEMS`].
fn menu_rects(layout: Option<&[MapRect]>) -> Vec<Rect> {
    MENU_ITEMS
        .iter()
        .map(
            |item| match layout.and_then(|rects| rects.get(item.layout_index)) {
                Some(r) => Rect::new(
                    r.ul_x as f32,
                    r.ul_y as f32,
                    r.width() as f32,
                    r.height() as f32,
                ),
                None => item.fallback_rect,
            },
        )
        .collect()
}

/// Pure click resolution: on a rising press edge over an item (`rects` is
/// parallel to [`MENU_ITEMS`]), return its action. Also returns the new
/// `last_pressed` to track for the next frame.
fn resolve_click(
    pointer: Option<Pointer2D>,
    last_pressed: bool,
    screen_size: Vector2<f32>,
    rects: &[Rect],
) -> (Option<MenuAction>, bool) {
    match pointer {
        Some(p) => {
            let action = if p.pressed && !last_pressed {
                pointer_to_canvas(
                    vec2(CANVAS_W, CANVAS_H),
                    p.position,
                    screen_size,
                    SCALE_MODE,
                )
                .and_then(|c| {
                    MENU_ITEMS
                        .iter()
                        .zip(rects)
                        .find(|(_, rect)| rect.contains(c))
                })
                .map(|(it, _)| it.action)
            } else {
                None
            };
            (action, p.pressed)
        }
        None => (None, false),
    }
}

pub struct MainMenuScene {
    world: World,
    scene_name: String,
    /// Pointer from the latest update, used for hover highlighting in render.
    pointer: Option<Pointer2D>,
    /// Whether the pointer was pressed last frame (for rising-edge clicks).
    last_pressed: bool,
    /// Screen size from the latest render, so `update` can map the pointer into
    /// canvas space consistently with how the canvas is drawn.
    last_screen_size: Vector2<f32>,
}

impl MainMenuScene {
    pub fn new() -> Self {
        let mut world = World::new();

        let player_entity = world.add_entity(());

        let inventory_entity = PlayerInventoryEntity::create(&mut world);
        PlayerInventoryEntity::set_position_rotation(
            &mut world,
            vec3(0.0, -1000.0, 0.0),
            Quaternion::new(1.0, 0.0, 0.0, 0.0),
        );

        world.add_unique(PlayerInfo {
            pos: vec3(0.0, 0.0, 0.0),
            rotation: Quaternion::new(1.0, 0.0, 0.0, 0.0),
            entity_id: player_entity,
            left_hand_entity_id: None,
            right_hand_entity_id: None,
            inventory_entity_id: inventory_entity,
        });
        world.add_unique(QuestInfo::new());
        world.add_unique(GlobalTemplateIdMap(HashMap::new()));
        world.add_unique(GlobalEntityMetadata(HashMap::new()));
        world.add_unique(Time::default());

        Self {
            world,
            scene_name: "main_menu".to_owned(),
            pointer: None,
            last_pressed: false,
            last_screen_size: vec2(CANVAS_W, CANVAS_H),
        }
    }
}

impl Default for MainMenuScene {
    fn default() -> Self {
        Self::new()
    }
}

impl GameScene for MainMenuScene {
    fn update(
        &mut self,
        time: &Time,
        input_context: &InputContext,
        asset_cache: &mut AssetCache,
        _game_options: &GameOptions,
        _command_effects: Vec<Effect>,
    ) -> Vec<Effect> {
        if let Ok(mut world_time) = self.world.borrow::<UniqueViewMut<Time>>() {
            *world_time = time.clone();
        }

        let layout = asset_cache.get_opt(&UI_LAYOUT_IMPORTER, LAYOUT_FILE);
        let rects = menu_rects(layout.as_deref().map(|r| r.as_slice()));

        self.pointer = input_context.pointer;
        let (action, last_pressed) = resolve_click(
            input_context.pointer,
            self.last_pressed,
            self.last_screen_size,
            &rects,
        );
        self.last_pressed = last_pressed;

        match action {
            Some(MenuAction::NewGame) => {
                vec![Effect::GlobalEffect(GlobalEffect::TransitionLevel {
                    level_file: NEW_GAME_MISSION.to_owned(),
                    loc: None,
                    entities_to_trigger: vec![],
                    vitals_transition:
                        crate::scripts::PlayerVitalsTransition::InitializeFromDestination,
                })]
            }
            Some(MenuAction::Quit) => vec![Effect::GlobalEffect(GlobalEffect::Quit)],
            None => Vec::new(),
        }
    }

    fn render(
        &mut self,
        _asset_cache: &mut AssetCache,
        _options: &GameOptions,
    ) -> (Vec<SceneObject>, Vector3<f32>, Quaternion<f32>) {
        // The menu is drawn in screen space in `render_per_eye` (which has the
        // screen size); the 3D scene is empty.
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

        // Full-screen backdrop.
        canvas.image(Rect::new(0.0, 0.0, CANVAS_W, CANVAS_H), "MAIN.PCX");

        // Clickable items, centered in their button and brighter when hovered.
        // Button rects come from the original `MAINR.BIN` layout (cached by the
        // asset cache after the first load).
        let layout = asset_cache.get_opt(&UI_LAYOUT_IMPORTER, LAYOUT_FILE);
        let rects = menu_rects(layout.as_deref().map(|r| r.as_slice()));
        let pointer_canvas = self.pointer.and_then(|p| {
            pointer_to_canvas(
                vec2(CANVAS_W, CANVAS_H),
                p.position,
                screen_size,
                SCALE_MODE,
            )
        });
        for (item, rect) in MENU_ITEMS.iter().zip(&rects) {
            let hovered = pointer_canvas.is_some_and(|pp| rect.contains(pp));
            canvas
                .text_native(*rect, item.label, MENU_FONT, HAlign::Center, VAlign::Middle)
                .opacity(if hovered { 1.0 } else { 0.6 });
        }

        canvas.render_screen_space(asset_cache, screen_size, SCALE_MODE)
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

    fn pointer_at(x: f32, y: f32, pressed: bool) -> Option<Pointer2D> {
        Some(Pointer2D {
            position: vec2(x, y),
            pressed,
        })
    }

    // The runtimes render at a 4:3 resolution, so PreserveAspect == stretch and
    // normalized coords map straight to the 640x480 canvas.
    const SCREEN: Vector2<f32> = Vector2 { x: 800.0, y: 600.0 };

    #[test]
    fn rising_edge_over_new_game_activates_it() {
        // Press edge over the top button (normalized ~ canvas (512, 37)).
        let (action, last) = resolve_click(
            pointer_at(0.8, 0.078, true),
            false,
            SCREEN,
            &menu_rects(None),
        );
        assert_eq!(action, Some(MenuAction::NewGame));
        assert!(last);
    }

    #[test]
    fn rising_edge_over_quit_activates_it() {
        let (action, _) = resolve_click(
            pointer_at(0.8, 0.870, true),
            false,
            SCREEN,
            &menu_rects(None),
        );
        assert_eq!(action, Some(MenuAction::Quit));
    }

    #[test]
    fn held_press_does_not_re_activate() {
        // Already pressed last frame -> no new activation even over an item.
        let (action, last) = resolve_click(
            pointer_at(0.8, 0.078, true),
            true,
            SCREEN,
            &menu_rects(None),
        );
        assert_eq!(action, None);
        assert!(last);
    }

    #[test]
    fn click_outside_items_does_nothing() {
        let (action, _) = resolve_click(
            pointer_at(0.05, 0.05, true),
            false,
            SCREEN,
            &menu_rects(None),
        );
        assert_eq!(action, None);
    }

    #[test]
    fn no_pointer_means_no_action() {
        let (action, last) = resolve_click(None, true, SCREEN, &menu_rects(None));
        assert_eq!(action, None);
        assert!(!last);
    }

    #[test]
    fn menu_rects_prefer_layout_and_fall_back() {
        // With a layout: New Game = rect 0, Quit = rect 5.
        let layout: Vec<MapRect> = (0..7)
            .map(|i| MapRect::new(10, i * 70, 110, i * 70 + 50))
            .collect();
        let rects = menu_rects(Some(&layout));
        assert_eq!(rects[0], Rect::new(10.0, 0.0, 100.0, 50.0));
        assert_eq!(rects[1], Rect::new(10.0, 350.0, 100.0, 50.0));
        // Without: the decoded MAINR.BIN fallbacks.
        let rects = menu_rects(None);
        assert_eq!(rects[0], Rect::new(400.0, 20.0, 179.0, 60.0));
        assert_eq!(rects[1], Rect::new(400.0, 400.0, 179.0, 60.0));
    }

    #[test]
    fn world_supports_transition_save_data() {
        // On "New Game", `switch_mission` calls `to_save_data` on the outgoing
        // scene's world, so the menu world must support it (PlayerInfo,
        // GlobalTemplateIdMap, ...). This would panic if a unique were missing.
        let scene = MainMenuScene::new();
        let _ = crate::save_load::to_save_data(scene.world());
    }
}
