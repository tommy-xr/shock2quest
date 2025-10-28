use std::collections::HashMap;

use cgmath::{vec3, Matrix4, Quaternion, Vector2, Vector3};
use dark::SCALE_FACTOR;
use engine::{
    assets::asset_cache::AssetCache,
    audio::AudioContext,
    scene::{light::SpotLight, SceneObject},
};
use shipyard::{UniqueViewMut, World};

use crate::{
    game_scene::GameScene,
    input_context::InputContext,
    inventory::PlayerInventoryEntity,
    mission::{GlobalEntityMetadata, GlobalTemplateIdMap, PlayerInfo},
    quest_info::QuestInfo,
    scripts::{Effect, GlobalEffect},
    time::Time,
    GameOptions,
};

/// Minimal debug scene that keeps the player anchored and renders a single cube
pub struct DebugMinimalScene {
    world: World,
    head_rotation: Quaternion<f32>,
    player_position: Vector3<f32>,
    player_rotation: Quaternion<f32>,
    head_height: f32,
    cube_distance: f32,
    cube_scale: f32,
    scene_name: String,
}

impl DebugMinimalScene {
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
            head_rotation: Quaternion::new(1.0, 0.0, 0.0, 0.0),
            player_position: vec3(0.0, 0.0, 0.0),
            player_rotation: Quaternion::new(1.0, 0.0, 0.0, 0.0),
            head_height: 4.0 / SCALE_FACTOR,
            cube_distance: 4.0 / SCALE_FACTOR,
            cube_scale: 0.35,
            scene_name: "debug_minimal".to_owned(),
        }
    }

    fn update_player_info(&mut self) {
        if let Ok(mut player_info) = self.world.borrow::<UniqueViewMut<PlayerInfo>>() {
            player_info.pos = self.player_position;
            player_info.rotation = self.player_rotation;
        }
    }

    fn cube_object(&self) -> SceneObject {
        let color = engine::scene::color_material::create(vec3(0.2, 0.8, 1.0));
        let mut cube = SceneObject::new(color, Box::new(engine::scene::cube::create()));

        let forward = self.head_rotation * vec3(0.0, 0.0, -1.0);
        let base = self.player_position + vec3(0.0, self.head_height, 0.0);
        let cube_position = base + forward * self.cube_distance;

        let transform =
            Matrix4::from_translation(cube_position) * Matrix4::from_scale(self.cube_scale);
        cube.set_transform(transform);
        cube
    }
}

impl Default for DebugMinimalScene {
    fn default() -> Self {
        Self::new()
    }
}

impl GameScene for DebugMinimalScene {
    fn update(
        &mut self,
        time: &Time,
        input_context: &InputContext,
        _asset_cache: &mut AssetCache,
        _game_options: &GameOptions,
        command_effects: Vec<Effect>,
    ) -> Vec<Effect> {
        let _ = command_effects;

        if let Ok(mut world_time) = self.world.borrow::<UniqueViewMut<Time>>() {
            *world_time = time.clone();
        }

        self.head_rotation = input_context.head.rotation;
        self.update_player_info();

        Vec::new()
    }

    fn render(
        &mut self,
        _asset_cache: &mut AssetCache,
        _options: &GameOptions,
    ) -> (Vec<SceneObject>, Vector3<f32>, Quaternion<f32>) {
        let cube = self.cube_object();
        (vec![cube], self.player_position, self.player_rotation)
    }

    fn render_per_eye(
        &mut self,
        _asset_cache: &mut AssetCache,
        _view: Matrix4<f32>,
        _projection: Matrix4<f32>,
        _screen_size: Vector2<f32>,
        _options: &GameOptions,
    ) -> Vec<SceneObject> {
        Vec::new()
    }

    fn finish_render(
        &mut self,
        _asset_cache: &mut AssetCache,
        _view: Matrix4<f32>,
        _projection: Matrix4<f32>,
        _screen_size: Vector2<f32>,
    ) {
    }

    fn handle_effects(
        &mut self,
        effects: Vec<Effect>,
        _global_context: &crate::mission::GlobalContext,
        _game_options: &GameOptions,
        _asset_cache: &mut AssetCache,
        _audio_context: &mut AudioContext<shipyard::EntityId, String>,
    ) -> Vec<GlobalEffect> {
        let _ = effects;
        Vec::new()
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
