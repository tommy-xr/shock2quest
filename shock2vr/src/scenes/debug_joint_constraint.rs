use std::collections::HashMap;

use cgmath::{vec3, Matrix4, Point3, Quaternion, Vector2, Vector3};
use dark::{
    mission::{room_database::RoomDatabase, SongParams},
    ss2_entity_info::SystemShock2EntityInfo,
    SCALE_FACTOR,
};
use engine::{
    assets::asset_cache::AssetCache,
    audio::AudioContext,
    scene::{color_material, light::SpotLight, SceneObject},
};
use rapier3d::{
    na::Point3 as RapierPoint,
    prelude::{
        Collider, ColliderBuilder, GenericJointBuilder, ImpulseJointHandle, Isometry,
        JointAxesMask, RigidBodyHandle, SharedShape,
    },
};
use shipyard::EntityId;

use crate::{
    game_scene::GameScene,
    input_context::InputContext,
    mission::{
        entity_populator::empty_entity_populator::EmptyEntityPopulator, mission_core::MissionCore,
        AbstractMission, AlwaysVisible, GlobalContext, SpawnLocation,
    },
    physics::CollisionGroup,
    quest_info::QuestInfo,
    save_load::HeldItemSaveData,
    scripts::{Effect, GlobalEffect},
    time::Time,
    GameOptions,
};

const FLOOR_COLOR: Vector3<f32> = Vector3::new(0.12, 0.12, 0.18);
const FLOOR_SIZE: Vector3<f32> = Vector3::new(60.0, 0.5, 60.0);

const BOX_COUNT: usize = 5;
const BOX_SIZE: Vector3<f32> = Vector3::new(1.5, 0.7, 0.7);
const BOX_SPACING: f32 = 2.8;
const BOX_START_HEIGHT: f32 = 4.0;

const UPWARD_FORCE: f32 = 40.0;
const LATERAL_FORCE: f32 = 25.0;
const IMPULSE_STRENGTH: f32 = 15.0;

pub struct DebugJointConstraintScene {
    core: MissionCore,
    body_handles: Vec<RigidBodyHandle>,
    #[allow(dead_code)]
    joint_handles: Vec<ImpulseJointHandle>,
    last_right_impulse: bool,
    last_left_impulse: bool,
}

impl DebugJointConstraintScene {
    pub fn new(
        global_context: &GlobalContext,
        game_options: &GameOptions,
        asset_cache: &mut AssetCache,
        audio_context: &mut AudioContext<EntityId, String>,
    ) -> Self {
        let abstract_mission = Self::create_debug_mission();

        let mut core = MissionCore::load(
            "debug_joint_constraint".to_string(),
            abstract_mission,
            asset_cache,
            audio_context,
            global_context,
            SpawnLocation::PositionRotation(
                vec3(0.0, 5.0 / SCALE_FACTOR, -6.0 / SCALE_FACTOR),
                Quaternion::new(1.0, 0.0, 0.0, 0.0),
            ),
            QuestInfo::new(),
            Box::new(EmptyEntityPopulator {}),
            HeldItemSaveData::empty(),
            game_options,
        );

        let (body_handles, joint_handles) = Self::spawn_joint_chain(&mut core);

        println!(
            "[debug_joint_constraint] Controls:\n\
             - Hold Left Trigger (mouse button 1 while holding Q) to push the left box upward.\n\
             - Hold Right Trigger (mouse button 1 while holding E) to push the center box upward.\n\
             - Use controller grips / mouse button 2 to push the outer boxes sideways (left grip pulls left, right grip pushes right).\n\
             - Tap the A button / mouse button 3 on either hand to fire an upward impulse on that side."
        );

        Self {
            core,
            body_handles,
            joint_handles,
            last_right_impulse: false,
            last_left_impulse: false,
        }
    }

    fn create_debug_mission() -> AbstractMission {
        AbstractMission {
            scene_objects: Self::create_floor_scene_objects(),
            song_params: SongParams {
                song: String::new(),
            },
            room_db: RoomDatabase { rooms: Vec::new() },
            physics_geometry: Some(Self::create_floor_physics()),
            spatial_data: None,
            entity_info: SystemShock2EntityInfo::empty(),
            obj_map: HashMap::new(),
            visibility_engine: Box::new(AlwaysVisible),
        }
    }

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

    fn create_floor_physics() -> Collider {
        let floor_size_scaled = vec3(
            FLOOR_SIZE.x / SCALE_FACTOR / 2.0,
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

    fn spawn_joint_chain(
        core: &mut MissionCore,
    ) -> (Vec<RigidBodyHandle>, Vec<ImpulseJointHandle>) {
        let mut body_handles = Vec::new();
        let mut joint_handles = Vec::new();

        let half_size = vec3(
            BOX_SIZE.x / SCALE_FACTOR / 2.0,
            BOX_SIZE.y / SCALE_FACTOR / 2.0,
            BOX_SIZE.z / SCALE_FACTOR / 2.0,
        );

        let spacing = BOX_SPACING / SCALE_FACTOR;
        let start_height = BOX_START_HEIGHT / SCALE_FACTOR;
        let joint_extension = ((BOX_SPACING - BOX_SIZE.x).max(0.0) / 2.0) / SCALE_FACTOR;

        let chain_offset = (BOX_COUNT as f32 - 1.0) * 0.5;
        for i in 0..BOX_COUNT {
            let offset = (i as f32 - chain_offset) * spacing;
            let position = vec3(offset, start_height, 0.0);
            let isometry = Isometry::translation(position.x, position.y, position.z);

            let handle = core.physics.create_dynamic_body(isometry, None);
            let shape = SharedShape::cuboid(half_size.x, half_size.y, half_size.z);
            core.physics
                .attach_collider(handle, shape, 3.0, CollisionGroup::selectable());
            body_handles.push(handle);
        }

        for window in body_handles.windows(2) {
            if let [parent, child] = window {
                let joint = GenericJointBuilder::new(JointAxesMask::LOCKED_SPHERICAL_AXES)
                    .local_anchor1(RapierPoint::new(half_size.x + joint_extension, 0.0, 0.0))
                    .local_anchor2(RapierPoint::new(-half_size.x - joint_extension, 0.0, 0.0))
                    .build();
                let handle = core.physics.create_impulse_joint(*parent, *child, joint);
                joint_handles.push(handle);
            }
        }

        (body_handles, joint_handles)
    }

    fn handle_debug_input(&mut self, input_context: &InputContext) {
        if self.body_handles.len() < 3 {
            return;
        }

        let left_idx = 0;
        let mid_idx = self.body_handles.len() / 2;
        let right_idx = self.body_handles.len() - 1;

        let up_force = vec3(0.0, UPWARD_FORCE / SCALE_FACTOR, 0.0);
        let lateral_force = vec3(LATERAL_FORCE / SCALE_FACTOR, 0.0, 0.0);

        if input_context.left_hand.trigger_value > 0.05 {
            self.core
                .physics
                .apply_force(self.body_handles[left_idx], up_force);
        }

        if input_context.right_hand.trigger_value > 0.05 {
            self.core
                .physics
                .apply_force(self.body_handles[mid_idx], up_force);
        }

        if input_context.left_hand.squeeze_value > 0.05 {
            self.core
                .physics
                .apply_force(self.body_handles[left_idx], -lateral_force);
        }

        if input_context.right_hand.squeeze_value > 0.05 {
            self.core
                .physics
                .apply_force(self.body_handles[right_idx], lateral_force);
        }

        let right_impulse_pressed =
            input_context.right_hand.a_value > 0.5 && !self.last_right_impulse;
        if right_impulse_pressed {
            self.core.physics.apply_impulse(
                self.body_handles[right_idx],
                vec3(0.0, IMPULSE_STRENGTH / SCALE_FACTOR, 0.0),
            );
        }

        let left_impulse_pressed = input_context.left_hand.a_value > 0.5 && !self.last_left_impulse;
        if left_impulse_pressed {
            self.core.physics.apply_impulse(
                self.body_handles[left_idx],
                vec3(0.0, IMPULSE_STRENGTH / SCALE_FACTOR, 0.0),
            );
        }

        self.last_right_impulse = input_context.right_hand.a_value > 0.5;
        self.last_left_impulse = input_context.left_hand.a_value > 0.5;
    }
}

impl GameScene for DebugJointConstraintScene {
    fn update(
        &mut self,
        time: &Time,
        input_context: &InputContext,
        asset_cache: &mut AssetCache,
        game_options: &GameOptions,
        command_effects: Vec<Effect>,
    ) -> Vec<Effect> {
        self.handle_debug_input(input_context);

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
        self.core.handle_effects(
            effects,
            global_context,
            game_options,
            asset_cache,
            audio_context,
        )
    }

    fn get_hand_spotlights(&self, options: &GameOptions) -> Vec<SpotLight> {
        self.core.get_hand_spotlights(options)
    }

    fn world(&self) -> &shipyard::World {
        self.core.world()
    }

    fn scene_name(&self) -> &str {
        self.core.scene_name()
    }

    fn queue_entity_trigger(&mut self, entity_name: String) {
        self.core.queue_entity_trigger(entity_name)
    }
}
