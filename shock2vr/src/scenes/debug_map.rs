use std::collections::HashMap;

use cgmath::{vec3, Matrix3, Matrix4, Quaternion, Vector3, InnerSpace};
use engine::{
    assets::asset_cache::AssetCache,
    scene::{color_material, light::SpotLight, SceneObject},
};
use shipyard::{UniqueViewMut, World};

use crate::{
    game_scene::GameScene,
    input_context::InputContext,
    inventory::PlayerInventoryEntity,
    mission::{GlobalEntityMetadata, GlobalTemplateIdMap, PlayerInfo},
    quest_info::QuestInfo,
    scripts::Effect,
    time::Time,
    GameOptions,
};

/// Map rendering constants
const MAP_MISSION: &str = "MEDSCI2";
const MAP_WIDTH: f32 = 614.0;  // PAGE001.PCX dimensions
const MAP_HEIGHT: f32 = 260.0;
const MAP_SCALE: f32 = 0.002;  // Scale to make it reasonably sized in VR
const MAP_DISTANCE: f32 = 2.0; // Distance from player
const SLOT_REVEAL_INTERVAL: f32 = 1.0; // Reveal one slot every second

/// Reusable map renderer component that can be positioned anywhere in 3D space
pub struct MapRenderer {
    pub mission_name: String,
    pub map_data: Option<dark::map::MapChunkData>,
    pub revealed_slots: Vec<bool>,
    pub world_position: Vector3<f32>,
    pub world_rotation: Quaternion<f32>,
    pub scale: f32,
}

impl MapRenderer {
    pub fn new(mission_name: String, world_position: Vector3<f32>, scale: f32) -> Self {
        let map_data = dark::map::MapChunkData::load_from_mission("Data", &mission_name).ok();
        let slot_count = map_data.as_ref().map(|d| d.chunk_count()).unwrap_or(0);

        Self {
            mission_name,
            map_data,
            revealed_slots: vec![false; slot_count],
            world_position,
            world_rotation: Quaternion::new(1.0, 0.0, 0.0, 0.0),
            scale,
        }
    }

    pub fn set_revealed_slots(&mut self, slots: &[usize]) {
        // Reset all slots
        for slot in &mut self.revealed_slots {
            *slot = false;
        }

        // Reveal specified slots
        for &slot_idx in slots {
            if slot_idx < self.revealed_slots.len() {
                self.revealed_slots[slot_idx] = true;
            }
        }
    }

    pub fn get_revealed_slot_count(&self) -> usize {
        self.revealed_slots.iter().filter(|&&revealed| revealed).count()
    }

    pub fn render(&self, _asset_cache: &mut AssetCache) -> Vec<SceneObject> {
        let mut objects = Vec::new();

        // Calculate world space dimensions
        let map_world_width = MAP_WIDTH * self.scale;
        let map_world_height = MAP_HEIGHT * self.scale;

        // Create base transform matrix for the map in world space
        let base_transform = Matrix4::from_translation(self.world_position)
            * Matrix4::from(self.world_rotation)
            * Matrix4::from_translation(vec3(-map_world_width / 2.0, -map_world_height / 2.0, 0.0));

        // Render background map (PAGE001.PCX)
        let background_path = format!("res/intrface/{}/english/page001.pcx", self.mission_name.to_uppercase());
        println!("Loading map background: {}", background_path);
        println!("Map world position: {:?}, size: {}x{}", self.world_position, map_world_width, map_world_height);

        // For now, use a simple colored background to test positioning
        println!("Creating colored background for map positioning test");
        println!("Map background: world_pos: {:?}, size: {:.3}x{:.3}, scale: {:.5}",
            self.world_position, map_world_width, map_world_height, self.scale);
        let background_transform = base_transform
            * Matrix4::from_nonuniform_scale(map_world_width, map_world_height, 1.0);

        let material = color_material::create(vec3(0.3, 0.3, 0.8)); // Blue background
        let mut background_obj = SceneObject::new(material, Box::new(engine::scene::quad::create()));
        background_obj.set_transform(background_transform);
        objects.push(background_obj);

        // Render revealed map chunks
        if let Some(ref map_data) = self.map_data {
            for (slot_idx, &is_revealed) in self.revealed_slots.iter().enumerate() {
                if !is_revealed {
                    continue;
                }

                if let Some(rect) = map_data.get_revealed_rect(slot_idx) {
                    let _chunk_path = format!(
                        "res/intrface/{}/english/p001r{:03}.pcx",
                        self.mission_name.to_uppercase(),
                        slot_idx
                    );

                    // Calculate position and size based on rectangle coordinates
                    let chunk_world_width = (rect.width() as f32) * self.scale;
                    let chunk_world_height = (rect.height() as f32) * self.scale;

                    // Position relative to map background using same coordinate system as base map
                    let chunk_offset_x = (rect.ul_x as f32) * self.scale;
                    let chunk_offset_y = (rect.ul_y as f32) * self.scale;

                    // Use colored rectangles to visualize the revealed chunks
                    println!("Creating colored chunk {} at rect: ({}, {}) -> ({}, {}), size: {}x{}, offset: ({:.3}, {:.3})",
                        slot_idx, rect.ul_x, rect.ul_y, rect.lr_x, rect.lr_y,
                        rect.width(), rect.height(), chunk_offset_x, chunk_offset_y);

                    // Use the same base_transform approach as the background map
                    let chunk_transform = base_transform
                        * Matrix4::from_translation(vec3(chunk_offset_x, chunk_offset_y, 0.01)) // Larger Z offset to ensure chunks are in front
                        * Matrix4::from_nonuniform_scale(chunk_world_width, chunk_world_height, 1.0);

                    // Use different colors for different slots to see the progression
                    let color = match slot_idx % 3 {
                        0 => vec3(1.0, 0.0, 0.0), // Red
                        1 => vec3(0.0, 1.0, 0.0), // Green
                        _ => vec3(1.0, 1.0, 0.0), // Yellow
                    };
                    let material = color_material::create(color);
                    let mut chunk_obj = SceneObject::new(material, Box::new(engine::scene::quad::create()));
                    chunk_obj.set_transform(chunk_transform);
                    objects.push(chunk_obj);
                }
            }
        }

        objects
    }
}

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

            let chunk_count = self.map_renderer.map_data.as_ref().map(|d| d.chunk_count()).unwrap_or(0);
            if self.current_slot_count < chunk_count {
                self.current_slot_count += 1;

                // Update revealed slots
                let revealed_slots: Vec<usize> = (0..self.current_slot_count).collect();
                self.map_renderer.set_revealed_slots(&revealed_slots);

                println!(
                    "Revealed slot {}, total: {}/{}",
                    self.current_slot_count - 1,
                    self.current_slot_count,
                    chunk_count
                );
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