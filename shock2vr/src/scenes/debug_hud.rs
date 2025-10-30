use std::collections::HashMap;

use cgmath::{vec3, Deg, Euler, Quaternion, Vector3};
use dark::properties::{PropHitPoints, PropMaxHitPoints};
use engine::{
    assets::asset_cache::AssetCache,
    scene::{light::SpotLight, SceneObject},
};
use shipyard::{UniqueViewMut, World};

use crate::{
    game_scene::GameScene,
    hud::create_arm_hud_panels,
    input_context::InputContext,
    inventory::PlayerInventoryEntity,
    mission::{GlobalEntityMetadata, GlobalTemplateIdMap, PlayerInfo},
    quest_info::QuestInfo,
    scripts::Effect,
    time::Time,
    GameOptions,
};

/// Debug HUD positioning constants
const DEBUG_HEAD_HEIGHT: f32 = 0.0;
const DEBUG_HAND_FORWARD_DISTANCE: f32 = 0.6;
const DEBUG_HAND_LATERAL_SPREAD: f32 = 0.15;
const DEBUG_HAND_VERTICAL_OFFSET: f32 = 1.45;

/// Health animation constants
const HEALTH_ANIMATION_SPEED: f32 = 0.25; // Cycles per second
const MIN_HEALTH_PERCENTAGE: f32 = 0.1; // 10% minimum
const MAX_HEALTH_PERCENTAGE: f32 = 1.0; // 100% maximum

/// Debug scene focused on testing the virtual arms HUD system
/// Positions hands closer to camera for easy inspection
pub struct DebugHudScene {
    world: World,
    player_entity: shipyard::EntityId,
    player_position: Vector3<f32>,
    player_rotation: Quaternion<f32>,
    head_rotation: Quaternion<f32>,
    left_hand_position: Vector3<f32>,
    left_hand_rotation: Quaternion<f32>,
    right_hand_position: Vector3<f32>,
    right_hand_rotation: Quaternion<f32>,
    scene_name: String,
}

impl DebugHudScene {
    pub fn new() -> Self {
        let mut world = World::new();

        // Create player entity with mock health data
        let player_entity = world.add_entity((
            PropHitPoints { hit_points: 75 }, // 75% health for testing
            PropMaxHitPoints { hit_points: 100 },
        ));

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
            player_entity,
            player_position: vec3(0.0, 0.0, 0.0),
            player_rotation: Quaternion::new(1.0, 0.0, 0.0, 0.0),
            head_rotation: Quaternion::new(1.0, 0.0, 0.0, 0.0),
            left_hand_position: vec3(0.0, 0.0, 0.0),
            left_hand_rotation: Quaternion::new(1.0, 0.0, 0.0, 0.0),
            right_hand_position: vec3(0.0, 0.0, 0.0),
            right_hand_rotation: Quaternion::new(1.0, 0.0, 0.0, 0.0),
            scene_name: "debug_hud".to_owned(),
        }
    }

    fn head_base(&self) -> Vector3<f32> {
        self.player_position + vec3(0.0, DEBUG_HEAD_HEIGHT, 0.0)
    }

    fn update_player_info(&mut self) {
        if let Ok(mut player_info) = self.world.borrow::<UniqueViewMut<PlayerInfo>>() {
            player_info.pos = self.player_position;
            player_info.rotation = self.player_rotation;
        }
    }

    fn calculate_hand_positions(&mut self, _time: &Time) {
        let head_base = self.head_base();

        // Calculate camera forward vector (negative Z in camera space)
        let forward = self.head_rotation * vec3(0.0, 0.0, -1.0);
        // Calculate camera right vector (positive X in camera space)
        let right = self.head_rotation * vec3(1.0, 0.0, 0.0);

        // Position hands in front of camera, spread laterally
        let center_position = head_base
            + forward * DEBUG_HAND_FORWARD_DISTANCE
            + vec3(0.0, DEBUG_HAND_VERTICAL_OFFSET, 0.0);

        let center_offset = -0.25;
        self.left_hand_position =
            center_position - right * DEBUG_HAND_LATERAL_SPREAD + (right * center_offset);
        self.right_hand_position =
            center_position + right * DEBUG_HAND_LATERAL_SPREAD + (right * center_offset);

        // Set hand rotations to face the camera for optimal HUD viewing
        // The hands should be rotated so the HUD panels face toward the camera
        // Use the head rotation as the base, then add additional rotations for proper forearm orientation
        let base_rotation = self.head_rotation;

        // For debug viewing, we want the panels to face more directly toward the camera
        // Add a slight inward rotation so both panels angle toward the center
        let left_inward_rotation = Quaternion::from(Euler::new(Deg(-90.0), Deg(90.0), Deg(180.0)));
        let right_inward_rotation = Quaternion::from(Euler::new(Deg(90.0), Deg(90.0), Deg(0.0)));

        self.left_hand_rotation = base_rotation * left_inward_rotation;
        self.right_hand_rotation = base_rotation * right_inward_rotation;
    }

    /// Update player health dynamically using a sin wave for testing
    fn update_dynamic_health(&mut self, time: &Time) {
        use shipyard::{Get, ViewMut};

        // Calculate animated health percentage using sin wave
        let time_seconds = time.total.as_secs_f32();
        let sin_value = (time_seconds * HEALTH_ANIMATION_SPEED * 2.0 * std::f32::consts::PI).sin();
        // Map sin wave from [-1, 1] to [MIN_HEALTH_PERCENTAGE, MAX_HEALTH_PERCENTAGE]
        let health_percentage = (sin_value + 1.0) / 2.0
            * (MAX_HEALTH_PERCENTAGE - MIN_HEALTH_PERCENTAGE)
            + MIN_HEALTH_PERCENTAGE;

        // Update the player entity's health component
        if let Ok(mut v_hit_points) = self.world.borrow::<ViewMut<PropHitPoints>>() {
            if let Ok(hit_points) = (&mut v_hit_points).get(self.player_entity) {
                hit_points.hit_points = (health_percentage * 100.0) as i32;
            }
        }
    }
}

impl Default for DebugHudScene {
    fn default() -> Self {
        Self::new()
    }
}

impl GameScene for DebugHudScene {
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

        // Update head rotation from input
        self.head_rotation = input_context.head.rotation;

        // Update dynamic health for testing
        self.update_dynamic_health(&time);

        // Update hand positions for HUD testing
        self.calculate_hand_positions(&time);
        self.update_player_info();

        Vec::new()
    }

    fn render(
        &mut self,
        asset_cache: &mut AssetCache,
        _options: &GameOptions,
    ) -> (Vec<SceneObject>, Vector3<f32>, Quaternion<f32>) {
        // Only render the virtual arms HUD panels
        let hud_panels = create_arm_hud_panels(
            asset_cache,
            &self.world,
            self.left_hand_position,
            self.left_hand_rotation,
            self.right_hand_position,
            self.right_hand_rotation,
        );

        (hud_panels, self.player_position, self.player_rotation)
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
