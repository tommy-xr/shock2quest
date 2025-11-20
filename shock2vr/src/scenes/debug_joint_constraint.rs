use cgmath::{Quaternion, Vector3, vec3};
use dark::SCALE_FACTOR;
use engine::{assets::asset_cache::AssetCache, audio::AudioContext};
use rapier3d::{
    na::Point3 as RapierPoint,
    prelude::{
        GenericJointBuilder, ImpulseJointHandle, Isometry, JointAxesMask, RigidBodyHandle,
        SharedShape,
    },
};
use shipyard::EntityId;

use crate::{
    GameOptions,
    game_scene::GameScene,
    input_context::InputContext,
    mission::{GlobalContext, SpawnLocation, mission_core::MissionCore},
    physics::CollisionGroup,
    scenes::debug_common::{
        DebugSceneBuildOptions, DebugSceneBuilder, DebugSceneHooks, HookedDebugScene,
    },
    time::Time,
};

const BOX_COUNT: usize = 5;
const BOX_SIZE: Vector3<f32> = Vector3::new(1.5, 0.7, 0.7);
const BOX_SPACING: f32 = 2.8;
const BOX_START_HEIGHT: f32 = 4.0;

const UPWARD_FORCE: f32 = 40.0;
const LATERAL_FORCE: f32 = 25.0;
const IMPULSE_STRENGTH: f32 = 15.0;

pub struct DebugJointConstraintScene;

impl DebugJointConstraintScene {
    pub fn new(
        global_context: &GlobalContext,
        game_options: &GameOptions,
        asset_cache: &mut AssetCache,
        audio_context: &mut AudioContext<EntityId, String>,
    ) -> Box<dyn GameScene> {
        let builder = DebugSceneBuilder::new("debug_joint_constraint")
            .with_default_floor()
            .with_spawn_location(SpawnLocation::PositionRotation(
                vec3(0.0, 5.0 / SCALE_FACTOR, -6.0 / SCALE_FACTOR),
                Quaternion::new(1.0, 0.0, 0.0, 0.0),
            ));

        let build_options = DebugSceneBuildOptions {
            global_context,
            game_options,
            asset_cache,
            audio_context,
        };

        let mut core = builder.build_core(build_options);
        let mut hooks = JointConstraintHooks::default();
        hooks.initialize(&mut core);
        Box::new(HookedDebugScene::new(core, hooks))
    }
}

#[derive(Default)]
struct JointConstraintHooks {
    body_handles: Vec<RigidBodyHandle>,
    #[allow(dead_code)]
    joint_handles: Vec<ImpulseJointHandle>,
    last_right_impulse: bool,
    last_left_impulse: bool,
}

impl JointConstraintHooks {
    fn initialize(&mut self, core: &mut MissionCore) {
        let (body_handles, joint_handles) = Self::spawn_joint_chain(core);
        self.body_handles = body_handles;
        self.joint_handles = joint_handles;

        println!(
            "[debug_joint_constraint] Controls:\n\
             - Hold Left Trigger (mouse button 1 while holding Q) to push the left box upward.\n\
             - Hold Right Trigger (mouse button 1 while holding E) to push the center box upward.\n\
             - Use controller grips / mouse button 2 to push the outer boxes sideways (left grip pulls left, right grip pushes right).\n\
             - Tap the A button / mouse button 3 on either hand to fire an upward impulse on that side."
        );
    }
}

impl DebugSceneHooks for JointConstraintHooks {
    fn before_update(
        &mut self,
        core: &mut MissionCore,
        _time: &Time,
        input_context: &InputContext,
        _asset_cache: &mut AssetCache,
        _game_options: &GameOptions,
    ) {
        self.handle_debug_input(core, input_context);
    }
}

impl JointConstraintHooks {
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

    fn handle_debug_input(&mut self, core: &mut MissionCore, input_context: &InputContext) {
        if self.body_handles.len() < 3 {
            return;
        }

        let left_idx = 0;
        let mid_idx = self.body_handles.len() / 2;
        let right_idx = self.body_handles.len() - 1;

        let up_force = vec3(0.0, UPWARD_FORCE / SCALE_FACTOR, 0.0);
        let lateral_force = vec3(LATERAL_FORCE / SCALE_FACTOR, 0.0, 0.0);

        if input_context.left_hand.trigger_value > 0.05 {
            core.physics
                .apply_force(self.body_handles[left_idx], up_force);
        }

        if input_context.right_hand.trigger_value > 0.05 {
            core.physics
                .apply_force(self.body_handles[mid_idx], up_force);
        }

        if input_context.left_hand.squeeze_value > 0.05 {
            core.physics
                .apply_force(self.body_handles[left_idx], -lateral_force);
        }

        if input_context.right_hand.squeeze_value > 0.05 {
            core.physics
                .apply_force(self.body_handles[right_idx], lateral_force);
        }

        let right_impulse_pressed =
            input_context.right_hand.a_value > 0.5 && !self.last_right_impulse;
        if right_impulse_pressed {
            core.physics.apply_impulse(
                self.body_handles[right_idx],
                vec3(0.0, IMPULSE_STRENGTH / SCALE_FACTOR, 0.0),
            );
        }

        let left_impulse_pressed = input_context.left_hand.a_value > 0.5 && !self.last_left_impulse;
        if left_impulse_pressed {
            core.physics.apply_impulse(
                self.body_handles[left_idx],
                vec3(0.0, IMPULSE_STRENGTH / SCALE_FACTOR, 0.0),
            );
        }

        self.last_right_impulse = input_context.right_hand.a_value > 0.5;
        self.last_left_impulse = input_context.left_hand.a_value > 0.5;
    }
}
