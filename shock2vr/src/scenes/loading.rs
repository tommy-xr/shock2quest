//! Loading screen.
//!
//! A minimal `GameScene` that draws the original `LOADING.PCX` backdrop while a
//! level loads, so the window keeps presenting frames instead of freezing on a
//! black frame. This is the static first slice (projects/loading-screen.md, PR 1):
//! just the authentic backdrop.
//!
//! `LOADING.PCX` already contains the original loading UI — the Von Braun logo and a
//! bracketed "<n>% Transfer Completed" bar. The original engine drew the live number
//! and filled that bracket; wiring our progress into that built-in slot is PR 2, and
//! actually driving it from a background load is PR 3.
//!
//! The `debug_loading` scene renders this so the UI can be inspected independent of
//! any real loading.

use std::collections::HashMap;

use cgmath::{Quaternion, Vector2, Vector3, vec2, vec3};
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
    inventory::PlayerInventoryEntity,
    mission::{GlobalContext, GlobalEntityMetadata, GlobalTemplateIdMap, PlayerInfo},
    quest_info::QuestInfo,
    scripts::{Effect, GlobalEffect},
    time::Time,
    ui::{Rect, ScaleMode, UiCanvas},
};

/// The loading art is authored on the original 640x480 `LOADING.PCX` canvas.
const CANVAS_W: f32 = 640.0;
const CANVAS_H: f32 = 480.0;
const LOADING_TEXTURE: &str = "LOADING.PCX";
/// The 4:3 art is letterboxed (not stretched) on non-4:3 windows.
const SCALE_MODE: ScaleMode = ScaleMode::PreserveAspect;

pub struct LoadingScene {
    world: World,
    scene_name: String,
}

impl LoadingScene {
    pub fn new() -> Self {
        // Mirror `MainMenuScene`'s minimal world so the transition machinery
        // (`switch_mission` -> `to_save_data` on the outgoing scene) has the uniques
        // it expects.
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
            scene_name: "loading".to_owned(),
        }
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
        _input_context: &InputContext,
        _asset_cache: &mut AssetCache,
        _game_options: &GameOptions,
        _command_effects: Vec<Effect>,
    ) -> Vec<Effect> {
        if let Ok(mut world_time) = self.world.borrow::<UniqueViewMut<Time>>() {
            *world_time = time.clone();
        }
        Vec::new()
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
        let mut canvas = UiCanvas::new(vec2(CANVAS_W, CANVAS_H));
        // Full-screen authentic loading backdrop.
        canvas.image(Rect::new(0.0, 0.0, CANVAS_W, CANVAS_H), LOADING_TEXTURE);
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

    #[test]
    fn scene_name_is_loading() {
        assert_eq!(LoadingScene::new().scene_name(), "loading");
    }

    #[test]
    fn world_supports_transition_save_data() {
        // On a transition, `switch_mission` calls `to_save_data` on the outgoing
        // scene's world, so the loading world must carry the expected uniques.
        let scene = LoadingScene::new();
        let _ = crate::save_load::to_save_data(scene.world());
    }
}
