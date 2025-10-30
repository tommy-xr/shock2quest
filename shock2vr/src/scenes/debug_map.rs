use std::collections::HashMap;

use cgmath::{vec3, InnerSpace, Matrix3, Matrix4, Quaternion, Vector3};
use engine::{
    assets::asset_cache::AssetCache,
    scene::{color_material, light::SpotLight, Renderable, SceneObject, TransformSceneObject},
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
const MAP_WIDTH: f32 = 614.0; // PAGE001.PCX dimensions
const MAP_HEIGHT: f32 = 260.0;
const MAP_SCALE: f32 = 0.002; // Back to original scale
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
        self.revealed_slots
            .iter()
            .filter(|&&revealed| revealed)
            .count()
    }

    pub fn render(&self, _asset_cache: &mut AssetCache) -> Vec<SceneObject> {
        // Create a transform group that handles world positioning
        let mut map_group = TransformSceneObject::new();

        // Final transform: position, rotation, scale, then center the map
        let pixel_to_world_scale = self.scale;
        let world_transform = Matrix4::from_translation(self.world_position)
            * Matrix4::from(self.world_rotation)
            * Matrix4::from_nonuniform_scale(pixel_to_world_scale, pixel_to_world_scale, 1.0)
            * Matrix4::from_translation(vec3(-MAP_WIDTH / 2.0, -MAP_HEIGHT / 2.0, 0.0)); // Center after scaling

        println!("Final World transform with centering: {:?}", world_transform);

        map_group.set_transform(world_transform);
        println!("World transform: {:?}", world_transform);

        // Create background quad in pixel space (0,0) -> (614,260) - make it semi-transparent for debugging
        let mut background = SceneObject::new(
            color_material::create(vec3(0.3, 0.3, 0.8)), // Semi-transparent blue background
            Box::new(engine::scene::quad::create()),
        );
        // CORRECT Z-ORDERING: Negative Z = closer, positive Z = further away
        let background_transform = Matrix4::from_translation(vec3(MAP_WIDTH / 2.0, MAP_HEIGHT / 2.0, 0.02)) // Behind chunks but visible
            * Matrix4::from_nonuniform_scale(MAP_WIDTH, MAP_HEIGHT, 1.0);
        background.set_transform(background_transform);
        map_group.add_scene_object(background);

        println!("Background transform: {:?}", background_transform);

        println!(
            "Map group: world_pos: {:?}, pixel_scale: {:.5}",
            self.world_position, pixel_to_world_scale
        );

        // Add test chunks to verify positioning works
        // Add a test chunk at known position - top-left of map area
        let mut test_chunk = SceneObject::new(
            color_material::create(vec3(1.0, 0.0, 1.0)), // Bright magenta
            Box::new(engine::scene::quad::create()),
        );
        // Magenta test chunk: negative Z = closer to camera
        let test_transform = Matrix4::from_translation(vec3(150.0, 80.0, -0.01)) // Slightly closer to camera
            * Matrix4::from_nonuniform_scale(200.0, 120.0, 1.0); // 200x120 pixel square - much larger
        test_chunk.set_transform(test_transform);
        map_group.add_scene_object(test_chunk);
        println!("Added test chunk at (0,0) with 100x100 size");

        // Add a GIANT test chunk that covers 1/4 of the map - should be impossible to miss
        let mut giant_chunk = SceneObject::new(
            color_material::create(vec3(0.0, 1.0, 0.0)), // Bright green
            Box::new(engine::scene::quad::create()),
        );
        // Green test chunk: negative Z = closer to camera, but behind magenta
        let giant_transform = Matrix4::from_translation(vec3(MAP_WIDTH * 0.7, MAP_HEIGHT * 0.6, 0.005)) // Between background and yellow chunks
            * Matrix4::from_nonuniform_scale(MAP_WIDTH * 0.6, MAP_HEIGHT * 0.8, 1.0); // Even larger - 60% x 80% of map
        giant_chunk.set_transform(giant_transform);
        map_group.add_scene_object(giant_chunk);
        println!("Added GIANT green chunk at bottom-right (should be impossible to miss!)");

        // Add revealed chunks in pixel space - coordinates directly from data!
        if let Some(ref map_data) = self.map_data {
            for (slot_idx, &is_revealed) in self.revealed_slots.iter().enumerate() {
                if !is_revealed {
                    continue;
                }

                if let Some(rect) = map_data.get_revealed_rect(slot_idx) {
                    // Work directly in pixel coordinates - no conversion needed!
                    let chunk_pixel_x = rect.ul_x as f32;
                    let chunk_pixel_y = rect.ul_y as f32;
                    let chunk_pixel_width = rect.width() as f32;
                    let chunk_pixel_height = rect.height() as f32;

                    println!("Creating chunk {} at pixels: ({}, {}) size: {}x{} (from rect: ({}, {}) -> ({}, {}))",
                        slot_idx, chunk_pixel_x, chunk_pixel_y, chunk_pixel_width, chunk_pixel_height,
                        rect.ul_x, rect.ul_y, rect.lr_x, rect.lr_y);

                    // Create chunk quad in pixel space
                    let mut chunk = SceneObject::new(
                        color_material::create(vec3(1.0, 1.0, 0.0)), // Bright yellow for all chunks
                        Box::new(engine::scene::quad::create()),
                    );

                    // Position in (0,0)→(614,260) pixel space - simple and clean!
                    let chunk_center_x = chunk_pixel_x + chunk_pixel_width / 2.0;
                    let chunk_center_y = chunk_pixel_y + chunk_pixel_height / 2.0;
                    let chunk_transform =
                        Matrix4::from_translation(vec3(chunk_center_x, chunk_center_y, -0.005)) // Closer than background, behind test chunks
                            * Matrix4::from_nonuniform_scale(
                                chunk_pixel_width,
                                chunk_pixel_height,
                                1.0,
                            );

                    println!("Chunk {} transform: {:?}", slot_idx, chunk_transform);
                    println!("Chunk {} final center: ({:.1}, {:.1}, {:.3}) spans: ({:.1}-{:.1}, {:.1}-{:.1})",
                        slot_idx, chunk_center_x, chunk_center_y, 0.01,
                        chunk_center_x - chunk_pixel_width/2.0, chunk_center_x + chunk_pixel_width/2.0,
                        chunk_center_y - chunk_pixel_height/2.0, chunk_center_y + chunk_pixel_height/2.0);

                    chunk.set_transform(chunk_transform);
                    map_group.add_scene_object(chunk);
                }
            }
        }

        // Flatten the transform group into the SceneObjects the engine expects
        let final_objects = map_group.render_objects();
        println!(
            "Total objects rendered: {} (background + {} chunks)",
            final_objects.len(),
            self.get_revealed_slot_count()
        );

        // Debug: Check final world positions of ALL objects with Z separation
        for (i, obj) in final_objects.iter().enumerate() {
            let world_pos = obj.get_world_position();
            println!("Object {} final world position: {:?} (should show clear Z separation)", i, world_pos);
        }

        final_objects
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

            let chunk_count = self
                .map_renderer
                .map_data
                .as_ref()
                .map(|d| d.chunk_count())
                .unwrap_or(0);
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
