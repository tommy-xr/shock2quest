//! Debug scene for inspecting per-joint creature hitboxes across animation poses.
//!
//! Spawns a creature and overlays, every frame, two wireframes of the *same*
//! fitted `HitBoxShape`s (`dark::hit_box`), transformed two different ways:
//!
//! - **green** - by the full joint world matrix (`root * joint`), exactly how the
//!   mesh is skinned and how the damage hitboxes are placed. This is the source
//!   of truth and tracks the animated mesh.
//! - **red** - by the *decomposed* `(position, get_rotation_from_matrix)` the
//!   ragdoll uses when it spawns a body per joint (see `RagDollManager::add_ragdoll`).
//!   `get_rotation_from_matrix` reads the raw 3x3 with no orthonormalization, so
//!   any scale/shear in the joint matrix makes red diverge from green.
//!
//! Where red and green coincide, the ragdoll conversion is faithful; where they
//! diverge (e.g. the thighs), the bug is in the hitbox->ragdoll conversion, not
//! in the fit/joint mapping. Cycle poses with the `DebugHitboxCyclePose` input
//! action (HTTP-triggerable via the debug runtime) to stress runtime placement.

use cgmath::{Matrix4, Point3, Quaternion, Vector3, vec3};
use dark::{SCALE_FACTOR, properties::PropTemplateId};
use engine::{assets::asset_cache::AssetCache, audio::AudioContext};
use shipyard::{EntityId, Get, IntoIter, IntoWithId, View};

use crate::{
    GameOptions,
    game_scene::GameScene,
    mission::{
        GlobalContext, SpawnLocation, entity_creator::CreateEntityOptions,
        mission_core::MissionCore,
    },
    runtime_props::{RuntimePropJointTransforms, RuntimePropTransform},
    scenes::debug_common::{
        DebugSceneBuildOptions, DebugSceneBuilder, DebugSceneHooks, HookedDebugScene,
    },
    scripts::Effect,
    util::{get_position_from_matrix, get_rotation_from_matrix},
};

/// Template spawned for inspection. -397 = grunt og-pipe (pipe hybrid), the same
/// humanoid the ragdoll debug scene uses.
const CREATURE_TEMPLATE_ID: i32 = -397;

/// Green: fitted shapes placed by the full joint matrix (matches the mesh).
const SHAPE_COLOR_DIRECT: Vector3<f32> = Vector3::new(0.2, 1.0, 0.3);
/// Red: fitted shapes placed the way the ragdoll decomposes the joint matrix.
const SHAPE_COLOR_RAGDOLL: Vector3<f32> = Vector3::new(1.0, 0.25, 0.2);

/// Where the creature spawns - straight ahead of the default player spawn.
fn focus_point() -> Point3<f32> {
    Point3::new(-5.0, 5.0 / SCALE_FACTOR, 0.0)
}

pub struct DebugHitboxScene;

impl DebugHitboxScene {
    pub fn new(
        global_context: &GlobalContext,
        game_options: &GameOptions,
        asset_cache: &mut AssetCache,
        audio_context: &mut AudioContext<EntityId, String>,
    ) -> Box<dyn GameScene> {
        let builder = DebugSceneBuilder::new("debug_hitbox")
            .with_default_floor()
            .with_spawn_location(SpawnLocation::PositionRotation(
                vec3(0.0, 5.0 / SCALE_FACTOR, 0.0),
                Quaternion::new(1.0, 0.0, 0.0, 0.0),
            ));

        let build_options = DebugSceneBuildOptions {
            global_context,
            game_options,
            asset_cache,
            audio_context,
        };

        let core = builder.build_core(build_options);
        Box::new(HookedDebugScene::new(core, HitboxHooks::new()))
    }
}

struct HitboxHooks {
    spawned: bool,
}

impl HitboxHooks {
    fn new() -> Self {
        println!(
            "[debug_hitbox] Overlays the fitted per-joint hitbox shapes:\n\
             - green = placed by the full joint matrix (matches the mesh)\n\
             - red   = placed the way the ragdoll decomposes the joint matrix\n\
             Trigger the `DebugHitboxCyclePose` input action to cycle animation poses\n\
             (e.g. `curl -XPOST .../v1/input/action -d '{{\"action\":\"DebugHitboxCyclePose\"}}'`)."
        );
        Self { spawned: false }
    }

    fn spawn_creature(
        &mut self,
        core: &mut MissionCore,
        global_context: &GlobalContext,
        game_options: &GameOptions,
        asset_cache: &mut AssetCache,
        audio_context: &mut AudioContext<EntityId, String>,
    ) {
        if self.spawned {
            return;
        }
        let position = focus_point();
        let spawn = Effect::CreateEntity {
            template_id: CREATURE_TEMPLATE_ID,
            position,
            orientation: Quaternion::new(1.0, 0.0, 0.0, 0.0),
            root_transform: Matrix4::from_translation(vec3(position.x, position.y, position.z)),
            options: CreateEntityOptions::default(),
        };
        core.handle_effects(
            vec![spawn],
            global_context,
            game_options,
            asset_cache,
            audio_context,
        );
        self.spawned = true;
    }
}

impl DebugSceneHooks for HitboxHooks {
    fn before_handle_effects(
        &mut self,
        core: &mut MissionCore,
        _effects: &mut Vec<Effect>,
        global_context: &GlobalContext,
        game_options: &GameOptions,
        asset_cache: &mut AssetCache,
        audio_context: &mut AudioContext<EntityId, String>,
    ) {
        self.spawn_creature(
            core,
            global_context,
            game_options,
            asset_cache,
            audio_context,
        );
    }

    fn after_render(
        &mut self,
        core: &mut MissionCore,
        scene_objects: &mut Vec<engine::scene::SceneObject>,
        _camera_position: &mut Vector3<f32>,
        _camera_rotation: &mut Quaternion<f32>,
        _asset_cache: &mut AssetCache,
        _options: &GameOptions,
    ) {
        // Find the spawned creature.
        let entity_id = core.world().run(|v_template: View<PropTemplateId>| {
            v_template
                .iter()
                .with_id()
                .find(|(_, t)| t.template_id == CREATURE_TEMPLATE_ID)
                .map(|(id, _)| id)
        });
        let Some(entity_id) = entity_id else {
            return;
        };

        let Some(model) = core.id_to_model.get(&entity_id) else {
            return;
        };
        let shapes = model.hit_box_shapes();

        // Live root + per-joint world transforms (written each frame by
        // update_animations).
        let transforms = core.world().run(
            |v_transform: View<RuntimePropTransform>,
             v_joints: View<RuntimePropJointTransforms>| {
                match (v_transform.get(entity_id), v_joints.get(entity_id)) {
                    (Ok(transform), Ok(joints)) => Some((transform.0, joints.0)),
                    _ => None,
                }
            },
        );
        let Some((root, joints)) = transforms else {
            return;
        };

        // Green: full joint matrix (how the mesh is skinned / hitboxes placed).
        let world_direct: Vec<Matrix4<f32>> = joints.iter().map(|j| root * *j).collect();
        scene_objects.extend(dark::hit_box::draw_debug_hit_box_shapes(
            &shapes,
            &world_direct,
            SHAPE_COLOR_DIRECT,
        ));

        // Red: position + extracted rotation, exactly as `add_ragdoll` builds the
        // per-joint body isometry. Diverges from green wherever the joint matrix
        // carries scale/shear that `get_rotation_from_matrix` drops.
        let world_ragdoll: Vec<Matrix4<f32>> = world_direct
            .iter()
            .map(|m| {
                let pos = get_position_from_matrix(m);
                let rot = get_rotation_from_matrix(m);
                Matrix4::from_translation(vec3(pos.x, pos.y, pos.z)) * Matrix4::from(rot)
            })
            .collect();
        scene_objects.extend(dark::hit_box::draw_debug_hit_box_shapes(
            &shapes,
            &world_ragdoll,
            SHAPE_COLOR_RAGDOLL,
        ));
    }
}
