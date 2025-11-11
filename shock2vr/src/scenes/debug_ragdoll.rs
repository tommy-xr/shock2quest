use std::collections::HashMap;

use cgmath::{InnerSpace, Matrix4, Point3, Quaternion, Vector2, Vector3, vec3};
use dark::{
    SCALE_FACTOR,
    mission::{SongParams, room_database::RoomDatabase},
    properties::PropTemplateId,
    ss2_entity_info::SystemShock2EntityInfo,
};
use engine::{
    assets::asset_cache::AssetCache,
    audio::AudioContext,
    scene::{SceneObject, color_material, light::SpotLight},
};
use rapier3d::prelude::{Collider, ColliderBuilder};
use shipyard::{EntityId, IntoIter, IntoWithId};

use crate::{
    GameOptions,
    game_scene::GameScene,
    input_context::InputContext,
    mission::{
        AbstractMission, AlwaysVisible, GlobalContext, SpawnLocation,
        entity_creator::CreateEntityOptions,
        entity_populator::empty_entity_populator::EmptyEntityPopulator, mission_core::MissionCore,
    },
    quest_info::QuestInfo,
    save_load::HeldItemSaveData,
    scripts::{Effect, GlobalEffect},
    time::Time,
};

const FLOOR_COLOR: Vector3<f32> = Vector3::new(0.15, 0.15, 0.20);
const FLOOR_SIZE: Vector3<f32> = Vector3::new(120.0, 0.5, 120.0);

const IMPULSE_STRENGTH: f32 = 1.0;
const PULL_FORCE: f32 = 1.0;

/// Debug scene for testing ragdoll physics implementation
/// Based on debug_entity_playground but focused on spawning and slaying creatures for ragdoll testing
pub struct DebugRagdollScene {
    core: MissionCore,
    pipe_hybrid_spawned: bool,
    slay_timer: f32,
    last_left_impulse: bool,
    last_right_pull: bool,
}

impl DebugRagdollScene {
    pub fn new(
        global_context: &GlobalContext,
        game_options: &GameOptions,
        asset_cache: &mut AssetCache,
        audio_context: &mut AudioContext<EntityId, String>,
    ) -> Self {
        // Create debug AbstractMission with minimal data
        let abstract_mission = Self::create_debug_mission();

        // Create MissionCore using our new load method
        let core = MissionCore::load(
            "debug_ragdoll".to_string(),
            abstract_mission,
            asset_cache,
            audio_context,
            global_context,
            SpawnLocation::PositionRotation(
                vec3(0.0, 5.0 / SCALE_FACTOR, 0.0), // Start well above the floor
                Quaternion::new(1.0, 0.0, 0.0, 0.0),
            ),
            QuestInfo::new(),
            Box::new(EmptyEntityPopulator {}),
            HeldItemSaveData::empty(),
            game_options,
        );

        println!(
            "[debug_ragdoll] Controls:\n\
             - Left mouse button (trigger) to apply upward impulse to ragdoll\n\
             - Right mouse button (squeeze) to pull ragdoll toward center with continuous force"
        );

        Self {
            core,
            pipe_hybrid_spawned: false,
            slay_timer: 0.0,
            last_left_impulse: false,
            last_right_pull: false,
        }
    }

    /// Create a debug AbstractMission with minimal required data
    fn create_debug_mission() -> AbstractMission {
        // Create visual scene objects (floor)
        let scene_objects = Self::create_floor_scene_objects();

        // Create custom physics geometry (floor collision)
        let physics_geometry = Self::create_floor_physics();

        // Create minimal entity info and obj map
        let entity_info = Self::create_empty_entity_info();
        let obj_map = HashMap::new();

        AbstractMission {
            scene_objects,
            song_params: SongParams {
                song: String::new(),
            },
            room_db: RoomDatabase { rooms: Vec::new() },
            physics_geometry: Some(physics_geometry),
            spatial_data: None, // No spatial queries needed for simple debug scene
            entity_info,
            obj_map,
            visibility_engine: Box::new(AlwaysVisible),
        }
    }

    /// Create floor scene objects for rendering
    fn create_floor_scene_objects() -> Vec<SceneObject> {
        let floor_size_scaled = vec3(
            FLOOR_SIZE.x / SCALE_FACTOR,
            FLOOR_SIZE.y / SCALE_FACTOR,
            FLOOR_SIZE.z / SCALE_FACTOR,
        );

        let floor_transform = Matrix4::from_translation(vec3(0.0, 0.0, 0.0))
            * Matrix4::from_nonuniform_scale(
                floor_size_scaled.x,
                floor_size_scaled.y,
                floor_size_scaled.z,
            );

        let floor_material = color_material::create(FLOOR_COLOR);
        let mut floor_object =
            SceneObject::new(floor_material, Box::new(engine::scene::cube::create()));
        floor_object.set_transform(floor_transform);

        vec![floor_object]
    }

    /// Create floor physics collider
    fn create_floor_physics() -> Collider {
        let floor_size_scaled = vec3(
            FLOOR_SIZE.x / SCALE_FACTOR / 2.0, // Half extents for box collider
            FLOOR_SIZE.y / SCALE_FACTOR / 2.0,
            FLOOR_SIZE.z / SCALE_FACTOR / 2.0,
        );

        ColliderBuilder::cuboid(
            floor_size_scaled.x,
            floor_size_scaled.y,
            floor_size_scaled.z,
        )
        .build()
    }

    /// Create empty entity info for debug scene
    /// We'll create a basic one and rely on the merge_with_gamesys call in MissionCore::load
    fn create_empty_entity_info() -> SystemShock2EntityInfo {
        // Use the new empty() constructor from SystemShock2EntityInfo
        SystemShock2EntityInfo::empty()
    }

    /// Spawn a pipe hybrid (template -397) for ragdoll testing
    fn spawn_pipe_hybrid(
        &mut self,
        global_context: &GlobalContext,
        game_options: &GameOptions,
        asset_cache: &mut AssetCache,
        audio_context: &mut AudioContext<EntityId, String>,
    ) {
        if self.pipe_hybrid_spawned {
            return;
        }

        // Spawn position above the floor
        let spawn_position = Point3::new(-5.0, 5.0 / SCALE_FACTOR, -0.0);

        // Create spawn effect to summon pipe hybrid (template -397)
        let spawn_effect = Effect::CreateEntity {
            template_id: -397, // Pipe hybrid template ID
            position: spawn_position,
            orientation: Quaternion::new(1.0, 0.0, 0.0, 0.0),
            root_transform: Matrix4::from_translation(vec3(
                spawn_position.x,
                spawn_position.y,
                spawn_position.z,
            )),
            options: CreateEntityOptions::default(),
        };

        // Queue the spawn effect
        let effects = vec![spawn_effect];
        let _global_effects = self.core.handle_effects(
            effects,
            global_context,
            game_options,
            asset_cache,
            audio_context,
        );

        self.pipe_hybrid_spawned = true;
        self.slay_timer = 1.0; // Slay after 1 second

        println!("Spawned pipe hybrid for ragdoll testing");
    }

    /// Kill spawned entity with damage for ragdoll testing
    fn kill_spawned_entity(
        &mut self,
        global_context: &GlobalContext,
        game_options: &GameOptions,
        asset_cache: &mut AssetCache,
        audio_context: &mut AudioContext<EntityId, String>,
    ) {
        // Find entities with template ID -397 (pipe hybrid) and kill them with damage
        let world = self.core.world();

        // Query for entities with PropTemplateId of -397
        let entities_to_kill = world.run(|v_template_id: shipyard::View<PropTemplateId>| {
            let mut entities_to_kill = Vec::new();

            for (entity_id, template_id) in v_template_id.iter().with_id() {
                if template_id.template_id == -397 {
                    entities_to_kill.push(entity_id);
                    println!("Found pipe hybrid entity {:?} to kill", entity_id);
                }
            }

            entities_to_kill
        });

        // Kill all found pipe hybrid entities with massive damage (outside the world.run closure)
        for entity_id in entities_to_kill {
            // Apply massive damage (-1000 HP) to trigger proper death animation
            let damage_effect = Effect::AdjustHitPoints {
                entity_id,
                delta: -1000, // Large negative value should kill any creature
            };
            let effects = vec![damage_effect];
            let _global_effects = self.core.handle_effects(
                effects,
                global_context,
                game_options,
                asset_cache,
                audio_context,
            );
            self.core.spawn_debug_ragdoll(entity_id);
            println!(
                "Applied massive damage to pipe hybrid entity {:?} for ragdoll testing",
                entity_id
            );
        }
    }

    fn handle_ragdoll_input(&mut self, input_context: &InputContext) {
        let ragdoll_bodies = self.core.rag_doll_manager.get_ragdoll_bodies();

        if ragdoll_bodies.is_empty() {
            return;
        }

        // Left mouse button (trigger) - Apply upward impulse (one-shot)
        let left_impulse_pressed =
            input_context.left_hand.trigger_value > 0.5 && !self.last_left_impulse;

        if left_impulse_pressed {
            // Apply upward impulse to the first ragdoll body (like head/torso)
            if let Some(first_body) = self.core.rag_doll_manager.get_first_ragdoll_body() {
                let impulse = vec3(0.0, IMPULSE_STRENGTH / SCALE_FACTOR, 0.0);
                self.core.physics.apply_impulse(first_body, impulse);
                println!("Applied upward impulse to ragdoll");
            }
        }

        // Right mouse button (squeeze) - Pull ragdoll upward and toward center (continuous force)
        if input_context.right_hand.squeeze_value > 0.05 {
            // Apply pulling force to all ragdoll bodies - upward and toward a central point
            let center_position = vec3(0.0, 3.0 / SCALE_FACTOR, 0.0); // Pull toward center above floor

            for &body_handle in &ragdoll_bodies {
                if let Some(body_transform) = self.core.physics.get_body_transform(body_handle) {
                    let body_position = vec3(
                        body_transform.translation.x,
                        body_transform.translation.y,
                        body_transform.translation.z,
                    );

                    // Calculate direction from body toward center point
                    let pull_direction = (center_position - body_position).normalize();
                    let pull_force = pull_direction * PULL_FORCE / SCALE_FACTOR;

                    self.core.physics.apply_force(body_handle, pull_force);
                }
            }
        }

        self.last_left_impulse = input_context.left_hand.trigger_value > 0.5;
        self.last_right_pull = input_context.right_hand.squeeze_value > 0.05;
    }
}

impl Default for DebugRagdollScene {
    fn default() -> Self {
        // This won't work without required parameters, but satisfies the trait
        panic!(
            "DebugRagdollScene requires GlobalContext, AssetCache, and AudioContext - use new() instead"
        )
    }
}

impl GameScene for DebugRagdollScene {
    fn update(
        &mut self,
        time: &Time,
        input_context: &InputContext,
        asset_cache: &mut AssetCache,
        game_options: &GameOptions,
        command_effects: Vec<Effect>,
    ) -> Vec<Effect> {
        // Handle ragdoll physics interactions
        self.handle_ragdoll_input(input_context);

        // Delegate to core
        self.core.update(
            time,
            asset_cache,
            input_context,
            game_options,
            command_effects,
        )
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
        self.core
            .render_per_eye(asset_cache, view, projection, screen_size, options)
    }

    fn finish_render(
        &mut self,
        asset_cache: &mut AssetCache,
        view: Matrix4<f32>,
        projection: Matrix4<f32>,
        screen_size: Vector2<f32>,
    ) {
        // Delegate to core
        self.core
            .finish_render(asset_cache, view, projection, screen_size)
    }

    fn handle_effects(
        &mut self,
        effects: Vec<Effect>,
        global_context: &GlobalContext,
        game_options: &GameOptions,
        asset_cache: &mut AssetCache,
        audio_context: &mut AudioContext<EntityId, String>,
    ) -> Vec<GlobalEffect> {
        // Handle automatic spawning and slaying for ragdoll testing
        if !self.pipe_hybrid_spawned {
            self.spawn_pipe_hybrid(global_context, game_options, asset_cache, audio_context);
        } else if self.slay_timer > 0.0 {
            // We can't access time here directly, so we'll use a simple approach
            // This is a debug scene, so perfect timing isn't critical
            self.slay_timer -= 0.016; // Approximate 60fps delta
            if self.slay_timer <= 0.0 {
                self.kill_spawned_entity(global_context, game_options, asset_cache, audio_context);
                self.slay_timer = -1.0; // Prevent repeated killing
            }
        }

        // Delegate to core
        self.core.handle_effects(
            effects,
            global_context,
            game_options,
            asset_cache,
            audio_context,
        )
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
