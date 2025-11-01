use std::{
    collections::HashMap,
    rc::Rc,
};

use cgmath::{vec3, Matrix4, Point3, Quaternion, Rad, Rotation, Rotation3, SquareMatrix, Vector2, Vector3};

use dark::{
    motion::AnimationPlayer,
    properties::{PropLocalPlayer, PropPosition, WrappedEntityId},
    BitmapAnimation, SCALE_FACTOR,
};
use engine::{
    assets::asset_cache::AssetCache,
    audio::AudioContext,
    scene::{light::SpotLight, ParticleSystem, SceneObject},
};
use crate::physics::PhysicsWorld;
use rapier3d::prelude::RigidBodyHandle;
use crate::scripts::ScriptWorld;

use shipyard::{EntitiesView, EntityId, Get, UniqueView, UniqueViewMut, View, ViewMut, World};

use crate::{
    game_scene::GameScene,
    gui::GuiManager,
    input_context::InputContext,
    inventory::PlayerInventoryEntity,
    mission::{CreateEntityOptions, entity_creator::EntityCreationInfo},
    physics::PlayerHandle,
    quest_info::QuestInfo,
    runtime_props::{RuntimePropDoNotSerialize, RuntimePropTransform},
    scripts::{Effect, GlobalEffect, Message, MessagePayload},
    teleport::TeleportSystem,
    time::Time,
    util::vec3_to_point3,
    virtual_hand::{VirtualHand, VirtualHandEffect},
    vr_config, GameOptions,
    creature::HitBoxManager,
};

use super::{
    visibility_engine::VisibilityEngine,
    DebugLine, EntityMetadata, PlayerInfo, EffectQueue, GlobalContext,
};

/// Core mission functionality separated from SS2-specific level loading
/// Contains all generic game scene systems: ECS, physics, rendering, player management, etc.
pub struct MissionCore {
    // Core Systems
    pub world: World,
    pub physics: PhysicsWorld,
    pub script_world: ScriptWorld,

    // Rendering Systems
    pub scene_objects: Vec<SceneObject>,
    pub id_to_model: HashMap<EntityId, dark::model::Model>,
    pub id_to_scene_object: HashMap<EntityId, SceneObject>,
    pub id_to_animation_player: HashMap<EntityId, AnimationPlayer>,
    pub id_to_bitmap: HashMap<EntityId, Rc<BitmapAnimation>>,
    pub id_to_particle_system: HashMap<EntityId, ParticleSystem>,
    pub id_to_physics: HashMap<EntityId, RigidBodyHandle>,

    // Player Systems
    pub player_handle: PlayerHandle,
    pub left_hand: VirtualHand,
    pub right_hand: VirtualHand,

    // Game Systems
    pub gui: GuiManager,
    pub hit_boxes: HitBoxManager,
    pub teleport_system: TeleportSystem,
    pub visibility_engine: Box<dyn VisibilityEngine>,

    // Debug and Utility
    pub debug_lines: Vec<DebugLine>,
    pub pending_entity_triggers: Vec<String>,

    // Metadata and Templates
    pub scene_name: String,
    pub template_name_to_template_id: HashMap<String, EntityMetadata>,
    pub template_to_entity_id: HashMap<i32, WrappedEntityId>,
}

impl MissionCore {
    /// Create a new MissionCore with default initialization
    pub fn new(scene_name: String, game_options: &GameOptions) -> Self {
        let mut world = World::new();
        let mut physics = PhysicsWorld::new();

        // Create player entity
        let player_entity = world.add_entity((PropLocalPlayer {}, RuntimePropDoNotSerialize {}));

        // Initialize inventory (placed far away to be invisible by default)
        let inventory_entity = PlayerInventoryEntity::create(&mut world);
        PlayerInventoryEntity::set_position_rotation(
            &mut world,
            vec3(0.0, -1000.0, 0.0),
            Quaternion::new(1.0, 0.0, 0.0, 0.0),
        );

        // Default player starting position (much higher above the floor to be safe)
        let start_pos = vec3(0.0, 3.0 / SCALE_FACTOR, 0.0);
        let start_rotation = Quaternion::new(1.0, 0.0, 0.0, 0.0);

        let player_handle = physics.create_player(start_pos, player_entity);

        // Add core world resources
        world.add_unique(PlayerInfo {
            pos: start_pos,
            rotation: start_rotation,
            entity_id: player_entity,
            left_hand_entity_id: None,
            right_hand_entity_id: None,
            inventory_entity_id: inventory_entity,
        });

        world.add_unique(QuestInfo::new());
        world.add_unique(EffectQueue { effects: Vec::new() });
        world.add_unique(Time::default());

        // Initialize teleport system based on game options
        let teleport_system = if game_options.experimental_features.contains("teleport") {
            let teleport_config = crate::teleport::TeleportConfig {
                enabled: true,
                button_mapping: crate::teleport::TeleportButton::Trigger,
                trigger_threshold: 0.5,
                max_distance: 20.0,
                ..Default::default()
            };
            TeleportSystem::new(teleport_config)
        } else {
            let teleport_config = crate::teleport::TeleportConfig {
                enabled: false,
                ..Default::default()
            };
            TeleportSystem::new(teleport_config)
        };

        Self {
            world,
            physics,
            script_world: ScriptWorld::new(),
            scene_objects: Vec::new(),
            id_to_model: HashMap::new(),
            id_to_scene_object: HashMap::new(),
            id_to_animation_player: HashMap::new(),
            id_to_bitmap: HashMap::new(),
            id_to_particle_system: HashMap::new(),
            id_to_physics: HashMap::new(),
            player_handle,
            left_hand: VirtualHand::new(vr_config::Handedness::Left),
            right_hand: VirtualHand::new(vr_config::Handedness::Right),
            gui: GuiManager::new(),
            hit_boxes: HitBoxManager::new(),
            teleport_system,
            visibility_engine: Box::new(super::visibility_engine::PortalVisibilityEngine::new()),
            debug_lines: Vec::new(),
            pending_entity_triggers: Vec::new(),
            scene_name,
            template_name_to_template_id: HashMap::new(),
            template_to_entity_id: HashMap::new(),
        }
    }

    /// Make an entity non-physical (remove from physics world)
    pub fn make_un_physical(&mut self, entity_id: EntityId) {
        let current_entity = self.id_to_physics.get(&entity_id);
        if current_entity.is_none() {
            return;
        }

        self.physics.remove(entity_id);
        self.id_to_physics.remove(&entity_id);
    }

    /// Make an entity physical (add to physics world)
    pub fn make_physical(&mut self, entity_id: EntityId) {
        let current_entity = self.id_to_physics.get(&entity_id);
        if current_entity.is_some() {
            return;
        }

        let maybe_model = self.id_to_model.get(&entity_id);

        let maybe_phys_obj = super::entity_creator::create_physics_representation(
            &mut self.world,
            &mut self.physics,
            &maybe_model,
            entity_id,
        );

        if let Some(phys_obj) = maybe_phys_obj {
            self.id_to_physics.insert(entity_id, phys_obj);
        }
    }

    /// Set entity position, rotation, and scale
    pub fn set_entity_position_rotation(
        &mut self,
        entity_id: EntityId,
        position: Vector3<f32>,
        rotation: Quaternion<f32>,
        scale: Vector3<f32>,
    ) {
        if let Some(rigid_body_handle) = self.id_to_physics.get(&entity_id) {
            self.physics
                .set_position_rotation(*rigid_body_handle, position, rotation);
        } else {
            let translation_matrix = Matrix4::from_translation(position);
            let rotation_matrix = Matrix4::<f32>::from(rotation);
            let scale_matrix = Matrix4::from_nonuniform_scale(scale.x, scale.y, scale.z);
            let xform = translation_matrix * rotation_matrix * scale_matrix;

            let v_entities = self.world.borrow::<EntitiesView>().unwrap();
            let mut v_transform = self
                .world
                .borrow::<ViewMut<RuntimePropTransform>>()
                .unwrap();

            let mut v_prop_position = self.world.borrow::<ViewMut<PropPosition>>().unwrap();

            v_entities.add_component(entity_id, &mut v_transform, RuntimePropTransform(xform));
            v_entities.add_component(
                entity_id,
                &mut v_prop_position,
                PropPosition {
                    position,
                    rotation,
                    cell: 0,
                },
            );
        }
    }

    /// Remove an entity from all systems
    pub fn remove_entity(&mut self, entity_id: EntityId) {
        // TODO: gui - remove entity
        self.hit_boxes.remove_entity(
            entity_id,
            &mut self.world,
            &mut self.script_world,
            &mut self.physics,
            &mut self.id_to_physics,
        );

        self.script_world.remove_entity(entity_id);
        self.id_to_bitmap.remove(&entity_id);
        self.id_to_model.remove(&entity_id);
        self.id_to_scene_object.remove(&entity_id);
        self.id_to_physics.remove(&entity_id);
        self.physics.remove(entity_id);

        self.world.delete_entity(entity_id);
    }

    /// Create a simple test entity with physics and rendering (for debug scenes)
    pub fn create_test_entity(
        &mut self,
        position: Vector3<f32>,
        rotation: Quaternion<f32>,
        size: f32,
        color: Vector3<f32>,
    ) -> EntityId {
        // Create entity with basic components
        let entity = self.world.add_entity((
            RuntimePropTransform(Matrix4::from_translation(position)),
            PropPosition {
                position,
                rotation,
                cell: 0,
            },
        ));

        // Create visual representation
        let material = engine::scene::color_material::create(color);
        let cube_scene_obj = SceneObject::new(material, Box::new(engine::scene::cube::create()));

        // Store the scene object for rendering
        self.id_to_scene_object.insert(entity, cube_scene_obj);

        // Add physics representation
        let cube_handle = self.physics.add_dynamic(
            entity,
            position,
            rotation,
            Vector3::new(0.0, 0.0, 0.0),
            crate::physics::PhysicsShape::Cuboid(Vector3::new(size, size, size)),
            crate::physics::CollisionGroup::entity(),
            false,
            crate::physics::DynamicPhysicsOptions::default(),
        );

        // Store physics handle for synchronization
        self.id_to_physics.insert(entity, cube_handle);

        entity
    }

    /// Synchronize scene object positions with physics world
    fn synchronize_physics_positions(&mut self) {
        let mut v_transform = self
            .world
            .borrow::<ViewMut<RuntimePropTransform>>()
            .unwrap();
        let mut v_prop_position = self.world.borrow::<ViewMut<PropPosition>>().unwrap();
        let v_entities = self.world.borrow::<EntitiesView>().unwrap();

        for (entity_id, handle) in &self.id_to_physics {
            if let Some(position) = self.physics.get_position(*handle) {
                let rotation = self.physics.get_rotation(*handle).unwrap_or_else(|| {
                    Quaternion::new(1.0, 0.0, 0.0, 0.0)
                });

                // For test entities, use unit scale (no PropScale component needed)
                let scale = vec3(1.0, 1.0, 1.0);

                let scale_xform =
                    Matrix4::from_nonuniform_scale(scale.x, scale.y, scale.z);
                let translation_xform = Matrix4::from_translation(position);
                let rotation_xform = Matrix4::from(rotation);
                let xform = translation_xform * rotation_xform * scale_xform;

                v_entities.add_component(
                    *entity_id,
                    &mut v_prop_position,
                    PropPosition {
                        position,
                        rotation,
                        cell: 0,
                    },
                );
                v_entities.add_component(*entity_id, &mut v_transform, RuntimePropTransform(xform));
            }
        }
    }

    pub fn create_entity_with_position(
        &mut self,
        _asset_cache: &mut AssetCache,
        _template_id: i32,
        _position: Point3<f32>,
        _orientation: Quaternion<f32>,
        _root_transform: Matrix4<f32>,
        _additional_options: CreateEntityOptions,
    ) -> EntityCreationInfo {
        // Stub implementation for MissionCore - debug scenes typically don't spawn complex entities
        // This method is called by VirtualHand effects but debug scenes can work without it
        println!("MissionCore: create_entity_with_position stub called with template_id {}", _template_id);
        EntityCreationInfo {
            entity_id: self.world.add_entity(()),
            model: None,
            bitmap_animation: None,
            rigid_body: None,
            scripts: Vec::new(),
        }
    }

    pub fn update_avatar_hands(
        &mut self,
        asset_cache: &mut AssetCache,
        player_pos: Vector3<f32>,
        player_rotation: Quaternion<f32>,
        input_context: &InputContext,
    ) {
        let (right_hand, mut right_hand_msgs) = VirtualHand::update(
            &self.right_hand,
            &self.physics,
            &self.world,
            player_pos,
            player_rotation,
            &input_context.right_hand,
        );
        self.right_hand = right_hand;

        let (left_hand, mut left_hand_msgs) = VirtualHand::update(
            &self.left_hand,
            &self.physics,
            &self.world,
            player_pos,
            player_rotation,
            &input_context.left_hand,
        );
        self.left_hand = left_hand;

        left_hand_msgs.append(&mut right_hand_msgs);

        for msg in left_hand_msgs {
            match msg {
                VirtualHandEffect::OutMessage { message } => self.script_world.dispatch(message),
                VirtualHandEffect::ApplyForce {
                    entity_id,
                    force,
                    torque,
                } => {
                    if let Some(rigid_body_handle) = self.id_to_physics.get(&entity_id) {
                        self.physics.apply_torque(*rigid_body_handle, torque);
                        self.physics.apply_force(*rigid_body_handle, force)
                    };
                }
                VirtualHandEffect::SetPositionRotation {
                    entity_id,
                    position,
                    rotation,
                    scale,
                } => {
                    self.set_entity_position_rotation(entity_id, position, rotation, scale);
                }
                VirtualHandEffect::SpawnEntity {
                    template_id,
                    position,
                    rotation,
                } => {
                    self.create_entity_with_position(
                        asset_cache,
                        template_id,
                        vec3_to_point3(position),
                        rotation,
                        Matrix4::identity(),
                        CreateEntityOptions::default(),
                    );
                }
                VirtualHandEffect::HoldItem { entity_id } => {
                    self.make_un_physical(entity_id);
                    self.script_world.dispatch(Message {
                        payload: MessagePayload::Hold,
                        to: entity_id,
                    });
                }
                VirtualHandEffect::DropItem { entity_id } => {
                    self.make_physical(entity_id);

                    self.script_world.dispatch(Message {
                        payload: MessagePayload::Drop,
                        to: entity_id,
                    });
                }
            }
        }
    }
}

impl GameScene for MissionCore {
    fn update(
        &mut self,
        time: &Time,
        input_context: &InputContext,
        asset_cache: &mut AssetCache,
        game_options: &GameOptions,
        command_effects: Vec<Effect>,
    ) -> Vec<Effect> {
        // Update time in world
        let _ = self.world.remove_unique::<Time>();
        self.world.add_unique(time.clone());

        let mut effects = command_effects;

        // Update teleport system and add effects (only if experimental flag enabled)
        if game_options.experimental_features.contains("teleport") {
            let teleport_effects = self.teleport_system.update(input_context);
            effects.extend(teleport_effects);
        }

        // Player movement logic (similar to Mission::update)
        let delta_time = time.elapsed.as_secs_f32();
        let player = {
            let player_info = self.world.borrow::<UniqueView<PlayerInfo>>().unwrap();
            player_info.clone()
        };

        let rot_speed = 2.0;
        let additional_rotation = Quaternion::from_axis_angle(
            vec3(0.0, 1.0, 0.0),
            Rad(input_context.left_hand.thumbstick.x * delta_time * rot_speed),
        );

        let new_rotation = player.rotation * additional_rotation;

        let dir = new_rotation * input_context.head.rotation;
        let move_thumbstick_value = input_context.right_hand.thumbstick;
        let forward = dir.rotate_vector(vec3(
            -delta_time * move_thumbstick_value.x * 25. / SCALE_FACTOR,
            0.0,
            -delta_time * move_thumbstick_value.y * 25. / SCALE_FACTOR,
        ));

        let up_value = input_context.left_hand.thumbstick.y / SCALE_FACTOR;

        let (new_character_pos, _collision_events) = {
            self.physics.update(
                forward + vec3(0.0, up_value, 0.0),
                &mut self.player_handle,
            )
        };

        // Clear forces
        self.physics.clear_forces();

        // Update player info
        let mut player_info = self.world.borrow::<UniqueViewMut<PlayerInfo>>().unwrap();
        player_info.pos = new_character_pos;
        player_info.rotation = new_rotation;
        drop(player_info);

        // Update VR hands (exact same logic as Mission::update_avatar_hands)
        self.update_avatar_hands(asset_cache, new_character_pos, new_rotation, input_context);

        effects
    }

    fn render(
        &mut self,
        _asset_cache: &mut AssetCache,
        options: &GameOptions,
    ) -> (Vec<SceneObject>, Vector3<f32>, Quaternion<f32>) {
        // Synchronize physics positions first
        self.synchronize_physics_positions();

        // Start with base scene objects
        let mut scene = self.scene_objects.clone();

        // Render scene objects with their physics-synchronized transforms
        let v_transform = self.world.borrow::<shipyard::View<RuntimePropTransform>>().unwrap();
        for (entity_id, scene_obj) in &self.id_to_scene_object {
            if let Ok(transform) = v_transform.get(*entity_id) {
                let mut transformed_obj = scene_obj.clone();
                transformed_obj.set_transform(transform.0);
                scene.push(transformed_obj);
            }
        }

        // Add VR hand rendering
        scene.append(&mut self.left_hand.render());
        scene.append(&mut self.right_hand.render());

        // Add debug physics rendering if enabled
        if options.debug_physics {
            let debug_render = &self.physics.debug_render();
            scene.extend(debug_render.clone());
        }

        // Return scene objects and player camera position/rotation
        let player_info = self.world.borrow::<UniqueView<PlayerInfo>>().unwrap();
        let pos = player_info.pos;
        let rot = player_info.rotation;
        drop(player_info);

        (scene, pos, rot)
    }

    fn render_per_eye(
        &mut self,
        _asset_cache: &mut AssetCache,
        _view: Matrix4<f32>,
        _projection: Matrix4<f32>,
        _screen_size: Vector2<f32>,
        _options: &GameOptions,
    ) -> Vec<SceneObject> {
        // Basic implementation - no per-eye specific rendering yet
        Vec::new()
    }

    fn finish_render(
        &mut self,
        _asset_cache: &mut AssetCache,
        _view: Matrix4<f32>,
        _projection: Matrix4<f32>,
        _screen_size: Vector2<f32>,
    ) {
        // Basic implementation - no finalization needed yet
    }

    fn handle_effects(
        &mut self,
        effects: Vec<Effect>,
        _global_context: &GlobalContext,
        _game_options: &GameOptions,
        _asset_cache: &mut AssetCache,
        _audio_context: &mut AudioContext<EntityId, String>,
    ) -> Vec<GlobalEffect> {
        // Basic implementation - just ignore effects for now
        let _ = effects;
        Vec::new()
    }

    fn get_hand_spotlights(&self, _options: &GameOptions) -> Vec<SpotLight> {
        // Basic implementation - no hand spotlights
        Vec::new()
    }

    fn world(&self) -> &World {
        &self.world
    }

    fn scene_name(&self) -> &str {
        &self.scene_name
    }

    fn queue_entity_trigger(&mut self, entity_name: String) {
        self.pending_entity_triggers.push(entity_name);
    }
}