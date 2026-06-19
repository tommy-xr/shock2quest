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
//! in the fit/joint mapping. (In practice the joint matrices are orthonormal, so
//! red and green coincide - rotation extraction is *not* the bug.) Cycle poses
//! with the `DebugHitboxCyclePose` input action (HTTP-triggerable via the debug
//! runtime) to stress runtime placement.
//!
//! It also overlays how the ragdoll wires up each parent->child joint:
//!
//! - **blue**    - the bone segment (parent joint origin -> child joint origin).
//! - **yellow**  - the impulse-joint anchor `add_ragdoll` actually uses: the
//!   *parent* joint origin (frame1 translation is zero). This is the pivot the
//!   child limb rotates about.
//! - **magenta** - the closest/contact points between the parent and child fitted
//!   shapes (`parry::query::contact`): where the two limb boxes actually meet.
//!
//! The yellow anchor sitting far from the magenta contact (e.g. the hip anchors
//! ~29 cm above/behind the thigh sockets, both pinned to the same bone-8 origin)
//! is the "joint positioned wrong" signal: the limb pivots about a point well
//! away from where it visually connects to its parent.

use cgmath::{Matrix4, Point3, Quaternion, Rotation, Vector3, vec3};
use dark::{SCALE_FACTOR, hit_box::HitBoxShape, properties::PropTemplateId, ss2_skeleton::Bone};
use engine::{
    assets::asset_cache::AssetCache,
    audio::AudioContext,
    scene::{SceneObject, VertexPosition, color_material, lines_mesh},
};
use rapier3d::{
    na::{Point3 as NaPoint3, Translation3},
    parry::query,
    prelude::{Isometry, SharedShape},
};
use shipyard::{EntityId, Get, IntoIter, IntoWithId, View};

use crate::{
    GameOptions,
    game_scene::GameScene,
    mission::{
        GlobalContext, SpawnLocation, entity_creator::CreateEntityOptions,
        mission_core::MissionCore,
    },
    physics::util::quat_to_nquat,
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
/// Blue: the parent->child "bone" segments, as the ragdoll wires up joints.
const JOINT_BONE_COLOR: Vector3<f32> = Vector3::new(0.35, 0.5, 1.0);
/// Yellow: where `RagDollManager::add_ragdoll` anchors each impulse joint (the
/// parent joint origin). This is the pivot the child limb rotates about.
const JOINT_ANCHOR_COLOR: Vector3<f32> = Vector3::new(1.0, 0.9, 0.1);
/// Magenta: the closest/contact points between the parent and child fitted shapes
/// (`parry::query::contact`) - i.e. where the two limb boxes actually meet, the
/// candidate "natural" joint location.
const JOINT_CONTACT_COLOR: Vector3<f32> = Vector3::new(1.0, 0.2, 1.0);

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

        // Joint overlay: how the ragdoll wires up each parent->child joint, drawn
        // statically from the live pose (no physics). Lets us see joint placement
        // (and compare the current anchor vs. where the limb boxes actually meet)
        // across poses, separate from any runtime solver behavior.
        if let Some((bones, _bind_world)) = model.ragdoll_source() {
            scene_objects.extend(draw_joint_debug(&shapes, &bones, &world_direct));
        }
    }
}

/// Build the per-joint debug overlay (bones, current anchors, shape contact
/// points) from the live posed joint world matrices.
fn draw_joint_debug(
    shapes: &std::collections::HashMap<u32, HitBoxShape>,
    bones: &[Bone],
    world: &[Matrix4<f32>],
) -> Vec<SceneObject> {
    let mut bone_verts: Vec<VertexPosition> = Vec::new();
    let mut anchor_verts: Vec<VertexPosition> = Vec::new();
    let mut contact_verts: Vec<VertexPosition> = Vec::new();

    for bone in bones {
        let Some(parent_id) = bone.parent_id else {
            continue;
        };
        let (c, p) = (bone.joint_id as usize, parent_id as usize);
        if c >= world.len() || p >= world.len() {
            continue;
        }

        let cpos = point_to_vec(get_position_from_matrix(&world[c]));
        let ppos = point_to_vec(get_position_from_matrix(&world[p]));

        // Bone segment + a small tick at the child joint origin.
        push_segment(&mut bone_verts, ppos, cpos);
        push_cross(&mut bone_verts, cpos, 0.02);

        // Current ragdoll anchor: the parent joint origin (frame1 translation is
        // zero in add_ragdoll, so the pivot sits here).
        push_cross(&mut anchor_verts, ppos, 0.07);

        // Where the two fitted shapes actually meet (closest/contact points).
        if let (Some(child_shape), Some(parent_shape)) =
            (shapes.get(&(bone.joint_id)), shapes.get(&(parent_id)))
        {
            let (pg, pi) = parry_shape(parent_shape, &world[p]);
            let (cg, ci) = parry_shape(child_shape, &world[c]);
            if let Ok(Some(ct)) = query::contact(&pi, &*pg, &ci, &*cg, 0.5) {
                let pt1 = Vector3::new(ct.point1.x, ct.point1.y, ct.point1.z);
                let pt2 = Vector3::new(ct.point2.x, ct.point2.y, ct.point2.z);
                push_cross(&mut contact_verts, pt1, 0.05);
                push_cross(&mut contact_verts, pt2, 0.05);
                push_segment(&mut contact_verts, pt1, pt2);
            }
        }
    }

    let mut out = Vec::new();
    for (verts, color) in [
        (bone_verts, JOINT_BONE_COLOR),
        (anchor_verts, JOINT_ANCHOR_COLOR),
        (contact_verts, JOINT_CONTACT_COLOR),
    ] {
        if !verts.is_empty() {
            out.push(SceneObject::new(
                color_material::create(color),
                Box::new(lines_mesh::create(verts)),
            ));
        }
    }
    out
}

/// Build a parry shape + world isometry for a fitted `HitBoxShape`, matching how
/// the ragdoll places its collider (body origin at the joint, cuboid center
/// folded into the transform).
fn parry_shape(shape: &HitBoxShape, world: &Matrix4<f32>) -> (SharedShape, Isometry<f32>) {
    let pos = get_position_from_matrix(world);
    let rot = get_rotation_from_matrix(world);
    let nrot = quat_to_nquat(rot);
    match shape {
        HitBoxShape::Capsule { a, b, radius } => (
            SharedShape::capsule(
                NaPoint3::new(a.x, a.y, a.z),
                NaPoint3::new(b.x, b.y, b.z),
                *radius,
            ),
            Isometry::from_parts(Translation3::new(pos.x, pos.y, pos.z), nrot),
        ),
        HitBoxShape::Cuboid {
            half_extents,
            center,
        } => {
            let cw = rot.rotate_vector(*center);
            (
                SharedShape::cuboid(
                    half_extents.x.max(0.01),
                    half_extents.y.max(0.01),
                    half_extents.z.max(0.01),
                ),
                Isometry::from_parts(
                    Translation3::new(pos.x + cw.x, pos.y + cw.y, pos.z + cw.z),
                    nrot,
                ),
            )
        }
    }
}

fn point_to_vec(p: Point3<f32>) -> Vector3<f32> {
    Vector3::new(p.x, p.y, p.z)
}

fn push_segment(verts: &mut Vec<VertexPosition>, a: Vector3<f32>, b: Vector3<f32>) {
    verts.push(VertexPosition { position: a });
    verts.push(VertexPosition { position: b });
}

/// A small 3-axis cross centered at `p` (six vertices / three segments).
fn push_cross(verts: &mut Vec<VertexPosition>, p: Vector3<f32>, s: f32) {
    for axis in [Vector3::unit_x(), Vector3::unit_y(), Vector3::unit_z()] {
        verts.push(VertexPosition {
            position: p - axis * s,
        });
        verts.push(VertexPosition {
            position: p + axis * s,
        });
    }
}
