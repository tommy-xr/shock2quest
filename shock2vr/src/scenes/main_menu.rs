//! Flatscreen main menu.
//!
//! A minimal `GameScene` that draws the original `MAIN.PCX` backdrop with
//! mouse-clickable "New Game" / "Quit" items. It reads `InputContext::pointer`
//! (normalized screen coords) and emits a `GlobalEffect` on click:
//! New Game -> `TransitionLevel` into the first mission, Quit -> `Quit`.
//!
//! See `projects/flatscreen-and-vr-architecture.md` (Slice 3).

use std::collections::HashMap;
use std::rc::Rc;

use cgmath::{Quaternion, Vector2, Vector3, vec2, vec3};
use dark::importers::{FONT_IMPORTER, TEXTURE_IMPORTER};
use engine::{
    assets::asset_cache::AssetCache,
    audio::AudioContext,
    scene::{SceneObject, light::SpotLight},
    texture::{TextureOptions, TextureTrait},
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
};

/// Mission loaded when the player chooses "New Game".
const NEW_GAME_MISSION: &str = "earth.mis";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MenuAction {
    NewGame,
    Quit,
}

struct MenuItem {
    label: &'static str,
    action: MenuAction,
    /// Normalized [0, 1] hit rect: (min_x, min_y, max_x, max_y), origin top-left.
    rect: (f32, f32, f32, f32),
}

const MENU_ITEMS: &[MenuItem] = &[
    MenuItem {
        label: "NEW GAME",
        action: MenuAction::NewGame,
        rect: (0.40, 0.46, 0.60, 0.52),
    },
    MenuItem {
        label: "QUIT",
        action: MenuAction::Quit,
        rect: (0.40, 0.55, 0.60, 0.61),
    },
];

fn in_rect(rect: (f32, f32, f32, f32), p: Vector2<f32>) -> bool {
    p.x >= rect.0 && p.y >= rect.1 && p.x <= rect.2 && p.y <= rect.3
}

/// Pure click resolution: on a rising press edge over an item, return its
/// action. Also returns the new `last_pressed` to track for the next frame.
fn resolve_click(pointer: Option<Pointer2D>, last_pressed: bool) -> (Option<MenuAction>, bool) {
    match pointer {
        Some(p) => {
            let action = if p.pressed && !last_pressed {
                MENU_ITEMS
                    .iter()
                    .find(|it| in_rect(it.rect, p.position))
                    .map(|it| it.action)
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
        _asset_cache: &mut AssetCache,
        _game_options: &GameOptions,
        _command_effects: Vec<Effect>,
    ) -> Vec<Effect> {
        if let Ok(mut world_time) = self.world.borrow::<UniqueViewMut<Time>>() {
            *world_time = time.clone();
        }

        self.pointer = input_context.pointer;
        let (action, last_pressed) = resolve_click(input_context.pointer, self.last_pressed);
        self.last_pressed = last_pressed;

        match action {
            Some(MenuAction::NewGame) => {
                vec![Effect::GlobalEffect(GlobalEffect::TransitionLevel {
                    level_file: NEW_GAME_MISSION.to_owned(),
                    loc: None,
                    entities_to_trigger: vec![],
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
        let texture_options = TextureOptions { wrap: false };
        let mut objs = Vec::new();

        // Full-screen backdrop.
        let bg = asset_cache.get_ext(&TEXTURE_IMPORTER, "MAIN.PCX", &texture_options);
        objs.push(SceneObject::screen_space_quad(
            bg.clone() as Rc<dyn TextureTrait>,
            vec2(0.0, 0.0),
            screen_size,
        ));

        // Clickable items, brighter when hovered.
        let font = asset_cache.get(&FONT_IMPORTER, "mainfont.fon").clone();
        let pointer_pos = self.pointer.map(|p| p.position);
        for item in MENU_ITEMS {
            let hovered = pointer_pos.is_some_and(|pp| in_rect(item.rect, pp));
            let (min_x, min_y, _, _) = item.rect;
            let x = min_x * screen_size.x;
            let y = min_y * screen_size.y;
            let font_size = 0.045 * screen_size.y;
            let opacity = if hovered { 1.0 } else { 0.6 };
            objs.push(SceneObject::screen_space_text(
                item.label,
                font.clone(),
                font_size,
                opacity,
                x,
                y,
            ));
        }

        objs
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

    #[test]
    fn rising_edge_over_new_game_activates_it() {
        // Press edge: not pressed last frame, pressed now, over "NEW GAME".
        let (action, last) = resolve_click(pointer_at(0.5, 0.49, true), false);
        assert_eq!(action, Some(MenuAction::NewGame));
        assert!(last);
    }

    #[test]
    fn rising_edge_over_quit_activates_it() {
        let (action, _) = resolve_click(pointer_at(0.5, 0.58, true), false);
        assert_eq!(action, Some(MenuAction::Quit));
    }

    #[test]
    fn held_press_does_not_re_activate() {
        // Already pressed last frame -> no new activation even over an item.
        let (action, last) = resolve_click(pointer_at(0.5, 0.49, true), true);
        assert_eq!(action, None);
        assert!(last);
    }

    #[test]
    fn click_outside_items_does_nothing() {
        let (action, _) = resolve_click(pointer_at(0.05, 0.05, true), false);
        assert_eq!(action, None);
    }

    #[test]
    fn no_pointer_means_no_action() {
        let (action, last) = resolve_click(None, true);
        assert_eq!(action, None);
        assert!(!last);
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
