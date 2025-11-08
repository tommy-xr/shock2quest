use std::collections::HashMap;

use cgmath::{InnerSpace, Matrix3, Quaternion, Vector3, vec3};
use engine::{
    assets::asset_cache::AssetCache,
    scene::{SceneObject, light::SpotLight},
};
use shipyard::{UniqueViewMut, World};

use crate::{
    GameOptions,
    game_scene::GameScene,
    input_context::InputContext,
    inventory::PlayerInventoryEntity,
    map_renderer::MapRenderer,
    mission::{GlobalEntityMetadata, GlobalTemplateIdMap, PlayerInfo},
    quest_info::QuestInfo,
    scripts::Effect,
    time::Time,
};

/// Debug scene constants
const MAP_MISSION: &str = "MEDSCI1";
const MAP_SCALE: f32 = 0.002; // Back to original scale
const SLOT_REVEAL_INTERVAL: f32 = 1.0; // Reveal one slot every second

/// Debug scene for testing the 2D interface map system
pub struct DebugMapScene {
    world: World,
    player_position: Vector3<f32>,
    player_rotation: Quaternion<f32>,
    head_rotation: Quaternion<f32>,
    left_hand_position: Vector3<f32>,
    left_hand_rotation: Quaternion<f32>,
    right_hand_position: Vector3<f32>,
    right_hand_rotation: Quaternion<f32>,
    scene_name: String,
    map_renderer: MapRenderer,
    slot_timer: f32,
    current_slot_count: usize,
}

impl DebugMapScene {
    fn update_player_info(&mut self) {
        if let Ok(mut player_info) = self.world.borrow::<UniqueViewMut<PlayerInfo>>() {
            player_info.pos = self.player_position;
            player_info.rotation = self.player_rotation;
        }
    }

    fn head_base(&self) -> Vector3<f32> {
        self.player_position + vec3(0.0, 1.5, 0.0) // 1.5m head height
    }

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
        world.add_unique(GlobalEntityMetadata(HashMap::new()));
        world.add_unique(GlobalTemplateIdMap(HashMap::new()));
        world.add_unique(QuestInfo::new());
        world.add_unique(Time::default());

        // Create map renderer positioned in front of player in world space
        let map_position = vec3(0.0, 1.5, -2.0); // 2 meters in front, 1.5 meters high
        let map_renderer = MapRenderer::new(MAP_MISSION.to_string(), map_position, MAP_SCALE);

        Self {
            world,
            player_position: vec3(0.0, 0.0, 0.0),
            player_rotation: Quaternion::new(1.0, 0.0, 0.0, 0.0),
            head_rotation: Quaternion::new(1.0, 0.0, 0.0, 0.0),
            left_hand_position: vec3(-0.3, -0.3, -0.5),
            left_hand_rotation: Quaternion::new(1.0, 0.0, 0.0, 0.0),
            right_hand_position: vec3(0.3, -0.3, -0.5),
            right_hand_rotation: Quaternion::new(1.0, 0.0, 0.0, 0.0),
            scene_name: "Debug Map".to_string(),
            map_renderer,
            slot_timer: 0.0,
            current_slot_count: 0,
        }
    }
}

impl GameScene for DebugMapScene {
    fn update(
        &mut self,
        time: &Time,
        input_context: &InputContext,
        _asset_cache: &mut AssetCache,
        _game_options: &GameOptions,
        command_effects: Vec<Effect>,
    ) -> Vec<Effect> {
        let _ = command_effects;

        // Update world time
        if let Ok(mut world_time) = self.world.borrow::<UniqueViewMut<Time>>() {
            *world_time = time.clone();
        }

        // Update player head and hand positions from input
        self.head_rotation = input_context.head.rotation;
        self.left_hand_position = input_context.left_hand.position;
        self.left_hand_rotation = input_context.left_hand.rotation;
        self.right_hand_position = input_context.right_hand.position;
        self.right_hand_rotation = input_context.right_hand.rotation;
        self.update_player_info();

        // Update slot progression timer
        self.slot_timer += time.elapsed.as_secs_f32();

        // Reveal next slot every SLOT_REVEAL_INTERVAL seconds
        if self.slot_timer >= SLOT_REVEAL_INTERVAL {
            self.slot_timer = 0.0;

            let chunk_count = self
                .map_renderer
                .map_data
                .as_ref()
                .map(|d| d.chunk_count())
                .unwrap_or(0);
            if self.current_slot_count < chunk_count {
                self.current_slot_count += 1;

                let revealed_slots: Vec<usize> = (0..self.current_slot_count).collect();
                self.map_renderer.set_revealed_slots(&revealed_slots);
            }
        }

        Vec::new()
    }

    fn render(
        &mut self,
        asset_cache: &mut AssetCache,
        _options: &GameOptions,
    ) -> (Vec<SceneObject>, Vector3<f32>, Quaternion<f32>) {
        let mut objects = Vec::new();

        // Update map position relative to player head (like debug_minimal cube_object)
        let forward = self.head_rotation * vec3(0.0, 0.0, -1.0);
        let head_position = self.head_base();
        let map_world_position = head_position + forward * 1.5; // 1.5 meters in front
        self.map_renderer.world_position = map_world_position;

        // Make map face the camera like debug_minimal cube
        let mut look_dir = head_position - map_world_position;
        if look_dir.magnitude2() < 1e-6 {
            look_dir = vec3(0.0, 0.0, 1.0);
        } else {
            look_dir = look_dir.normalize();
        }

        let mut up = vec3(0.0, 1.0, 0.0);
        let mut right = look_dir.cross(up);
        if right.magnitude2() < 1e-6 {
            up = vec3(0.0, 0.0, 1.0);
            right = look_dir.cross(up);
        }
        right = right.normalize();
        let true_up = right.cross(look_dir).normalize();
        let rotation_matrix = Matrix3::from_cols(right, true_up, -look_dir);
        self.map_renderer.world_rotation = Quaternion::from(rotation_matrix);

        // Render the map
        objects.extend(self.map_renderer.render(asset_cache));

        (objects, self.player_position, self.player_rotation)
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
