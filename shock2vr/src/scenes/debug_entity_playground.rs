use std::collections::HashMap;

use cgmath::{vec3, Matrix4, Quaternion, Vector2, Vector3};
use dark::SCALE_FACTOR;
use engine::{
    assets::asset_cache::AssetCache,
    audio::AudioContext,
    scene::{color_material, light::SpotLight, SceneObject},
};
use shipyard::EntityId;

use crate::{
    game_scene::GameScene,
    input_context::InputContext,
    mission::{
        mission_core::MissionCore,
        GlobalContext, GlobalEntityMetadata, GlobalTemplateIdMap,
    },
    physics::{CollisionGroup, DynamicPhysicsOptions, PhysicsShape},
    scripts::{Effect, GlobalEffect},
    time::Time,
    GameOptions,
};

const FLOOR_COLOR: Vector3<f32> = Vector3::new(0.15, 0.15, 0.20);
const FLOOR_SIZE: Vector3<f32> = Vector3::new(120.0, 0.5, 120.0);

/// Debug scene that demonstrates MissionCore working with real SS2 entities
/// Spawns entities from shock2.gam on a simple floor plane for testing
pub struct DebugEntityPlaygroundScene {
    core: MissionCore,
    entities_to_spawn: Vec<&'static str>,
    spawn_index: usize,
    spawn_positions: Vec<Vector3<f32>>,
}

impl DebugEntityPlaygroundScene {
    pub fn new(global_context: &GlobalContext, game_options: &GameOptions) -> Self {
        let mut core = MissionCore::new("debug_entity_playground".to_string(), game_options);

        // For simple debug scene, we don't need SS2 template mapping
        // Just add empty mappings to satisfy other systems
        core.world.add_unique(GlobalEntityMetadata(HashMap::new()));
        core.world.add_unique(GlobalTemplateIdMap(HashMap::new()));

        // Create simple test environment
        Self::create_test_environment(&mut core);

        // Test entities using specific template IDs you provided
        let entities_to_spawn = vec![
            "Pistol",      // Template ID -17
            "Laser",       // Template ID -22
            "Wrench",      // Template ID -928
            "Vent Part",   // Template ID -1998
        ];

        // Define spawn positions much further from player spawn, high above the plane
        let spawn_positions = vec![
            vec3(10.0 / SCALE_FACTOR, 6.0 / SCALE_FACTOR, 10.0 / SCALE_FACTOR),
            vec3(-10.0 / SCALE_FACTOR, 6.0 / SCALE_FACTOR, 10.0 / SCALE_FACTOR),
            vec3(10.0 / SCALE_FACTOR, 6.0 / SCALE_FACTOR, -10.0 / SCALE_FACTOR),
            vec3(-10.0 / SCALE_FACTOR, 6.0 / SCALE_FACTOR, -10.0 / SCALE_FACTOR),
        ];

        // Create the test entities using template IDs
        let mut scene = Self {
            core,
            entities_to_spawn,
            spawn_index: 0,
            spawn_positions,
        };

        // Spawn the test entities immediately
        scene.spawn_test_entities();

        scene
    }


    /// Create simple test environment with floor
    fn create_test_environment(core: &mut MissionCore) {
        // Add a kinematic floor for physics interactions
        let floor_entity = core.world.add_entity(());

        // Scale floor size by SCALE_FACTOR for physics
        let floor_size_scaled = vec3(
            FLOOR_SIZE.x / SCALE_FACTOR,
            FLOOR_SIZE.y / SCALE_FACTOR,
            FLOOR_SIZE.z / SCALE_FACTOR
        );

        // Put floor at y=0 for simplicity
        core.physics.add_kinematic(
            floor_entity,
            vec3(0.0, 0.0, 0.0),
            Quaternion::new(1.0, 0.0, 0.0, 0.0),
            vec3(0.0, 0.0, 0.0),
            floor_size_scaled,
            CollisionGroup::entity(),
            false,
        );

        // Create visual floor object (match the physics size exactly)
        let floor_transform = Matrix4::from_translation(vec3(0.0, 0.0, 0.0))
            * Matrix4::from_nonuniform_scale(floor_size_scaled.x, floor_size_scaled.y, floor_size_scaled.z);

        let floor_material = color_material::create(FLOOR_COLOR);
        let mut floor_object = SceneObject::new(floor_material, Box::new(engine::scene::cube::create()));
        floor_object.set_transform(floor_transform);

        core.scene_objects.push(floor_object);
    }

    /// Spawn test entities using specific template IDs
    fn spawn_test_entities(&mut self) {
        let template_ids = vec![-17, -22, -928, -1998]; // Pistol, Laser, Wrench, Vent Part

        for (i, &template_id) in template_ids.iter().enumerate() {
            if i < self.spawn_positions.len() {
                let position = self.spawn_positions[i];
                let entity_name = self.entities_to_spawn.get(i).unwrap_or(&"Unknown");

                println!("Spawning entity: {} (Template ID: {}) at position {:?}",
                         entity_name, template_id, position);

                // Create colored cube entities that will fall and move with physics
                let color = match i {
                    0 => Vector3::new(1.0, 0.2, 0.2), // Red for pistol
                    1 => Vector3::new(0.2, 1.0, 0.2), // Green for laser
                    2 => Vector3::new(0.2, 0.2, 1.0), // Blue for wrench
                    3 => Vector3::new(1.0, 1.0, 0.2), // Yellow for vent part
                    _ => Vector3::new(0.5, 0.5, 0.5), // Gray default
                };

                let cube_size = 1.0 / SCALE_FACTOR;

                // Use MissionCore's test entity creation method
                let _cube_entity = self.core.create_test_entity(
                    position,
                    Quaternion::new(1.0, 0.0, 0.0, 0.0),
                    cube_size,
                    color,
                );
            }
        }

        println!("Spawned {} test entities as colored cubes", template_ids.len());
    }
}

impl Default for DebugEntityPlaygroundScene {
    fn default() -> Self {
        // This won't work without GlobalContext, but satisfies the trait
        panic!("DebugEntityPlaygroundScene requires GlobalContext - use new() instead")
    }
}

impl GameScene for DebugEntityPlaygroundScene {
    fn update(
        &mut self,
        time: &Time,
        input_context: &InputContext,
        asset_cache: &mut AssetCache,
        game_options: &GameOptions,
        command_effects: Vec<Effect>,
    ) -> Vec<Effect> {
        // Delegate to core
        self.core.update(time, input_context, asset_cache, game_options, command_effects)
    }

    fn render(
        &mut self,
        asset_cache: &mut AssetCache,
        options: &GameOptions,
    ) -> (Vec<SceneObject>, Vector3<f32>, Quaternion<f32>) {
        // Delegate to core
        self.core.render(asset_cache, options)
    }

    fn render_per_eye(
        &mut self,
        asset_cache: &mut AssetCache,
        view: Matrix4<f32>,
        projection: Matrix4<f32>,
        screen_size: Vector2<f32>,
        options: &GameOptions,
    ) -> Vec<SceneObject> {
        // Delegate to core
        self.core.render_per_eye(asset_cache, view, projection, screen_size, options)
    }

    fn finish_render(
        &mut self,
        asset_cache: &mut AssetCache,
        view: Matrix4<f32>,
        projection: Matrix4<f32>,
        screen_size: Vector2<f32>,
    ) {
        // Delegate to core
        self.core.finish_render(asset_cache, view, projection, screen_size)
    }

    fn handle_effects(
        &mut self,
        effects: Vec<Effect>,
        global_context: &GlobalContext,
        game_options: &GameOptions,
        asset_cache: &mut AssetCache,
        audio_context: &mut AudioContext<EntityId, String>,
    ) -> Vec<GlobalEffect> {
        // Delegate to core
        self.core.handle_effects(effects, global_context, game_options, asset_cache, audio_context)
    }

    fn get_hand_spotlights(&self, options: &GameOptions) -> Vec<SpotLight> {
        // Delegate to core
        self.core.get_hand_spotlights(options)
    }

    fn world(&self) -> &shipyard::World {
        // Delegate to core
        self.core.world()
    }

    fn scene_name(&self) -> &str {
        // Delegate to core
        self.core.scene_name()
    }

    fn queue_entity_trigger(&mut self, entity_name: String) {
        // Delegate to core
        self.core.queue_entity_trigger(entity_name)
    }
}