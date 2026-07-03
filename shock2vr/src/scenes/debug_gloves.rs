use std::rc::Rc;

use cgmath::{Deg, Matrix4, Point3, Quaternion, Rotation3, SquareMatrix, Vector3, point3, vec3};
use dark::{
    glb_model::GlbModel,
    importers::{GLB_MODELS_IMPORTER, TEXTURE_IMPORTER},
};
use engine::{
    assets::asset_cache::AssetCache,
    audio::AudioContext,
    scene::{SceneObject, SkinnedMaterial, color_material},
};
use shipyard::EntityId;
use tracing::info;

use crate::{
    GameOptions,
    game_scene::GameScene,
    mission::{GlobalContext, SpawnLocation, mission_core::MissionCore},
    scenes::{
        debug_common::{
            DebugSceneBuildOptions, DebugSceneBuilder, DebugSceneHooks, HookedDebugScene,
        },
        hand_pose::{self, HandPoseRetarget, Pose},
    },
};

const GLOVE_POSITION: Point3<f32> = point3(0.0, 3.3, 1.0);
const GLOVE_SPACING: f32 = 0.28;
const ROW_SPACING: f32 = 0.45;

/// A glove with a pose baked into its skinning matrices, plus debug cubes at
/// the posed joint positions. Both are in model space; `after_render` places
/// them in the world.
struct PosedGlove {
    objects: Vec<SceneObject>,
    debug_cubes: Vec<SceneObject>,
}

struct GloveHooks {
    /// Rows of gloves, top to bottom. Row 1 (reference poses) reads bind,
    /// open, point, fist left to right; row 2 (blends) reads open->fist at
    /// 25/50/75%, then an index-only half-pull (trigger).
    rows: Vec<Vec<PosedGlove>>,
    /// Everything needed to re-pose the animated glove each frame
    animation: GloveAnimation,
}

/// A glove cycling open->fist over time, rendered at the end of the blend row.
struct GloveAnimation {
    model: Rc<GlbModel>,
    retarget: HandPoseRetarget,
    texture: Option<Rc<dyn engine::texture::TextureTrait>>,
    open: Pose,
    fist: Pose,
    total_time: f32,
}

impl GloveAnimation {
    const CYCLE_SECONDS: f32 = 3.0;

    fn current_glove(&self) -> PosedGlove {
        let phase = self.total_time * std::f32::consts::TAU / Self::CYCLE_SECONDS;
        let amount = 0.5 - 0.5 * phase.cos();
        let pose = self.open.blend(&self.fist, amount);
        build_posed_glove(
            &self.model,
            &self.retarget,
            Some(&pose),
            self.texture.as_ref(),
        )
    }
}

impl DebugSceneHooks for GloveHooks {
    fn before_update(
        &mut self,
        _core: &mut MissionCore,
        time: &crate::time::Time,
        _input_context: &crate::input_context::InputContext,
        _asset_cache: &mut AssetCache,
        _game_options: &GameOptions,
    ) {
        self.animation.total_time = time.total.as_secs_f32();
    }

    fn after_render(
        &mut self,
        _core: &mut MissionCore,
        scene_objects: &mut Vec<SceneObject>,
        _camera_position: &mut Vector3<f32>,
        _camera_rotation: &mut Quaternion<f32>,
        _asset_cache: &mut AssetCache,
        _options: &GameOptions,
    ) {
        // The animated glove joins the end of the blend row (row 1)
        let animated = self.animation.current_glove();
        for (row_index, row) in self.rows.iter().enumerate() {
            let animated_glove = (row_index == 1).then_some(&animated);
            let count = (row.len() + animated_glove.map_or(0, |_| 1)) as f32;
            let y = GLOVE_POSITION.y - row_index as f32 * ROW_SPACING;
            for (index, glove) in row.iter().chain(animated_glove).enumerate() {
                // Negated so each row reads left to right on screen (the
                // camera looks along -X toward the gloves)
                let offset = ((count - 1.0) / 2.0 - index as f32) * GLOVE_SPACING;
                // Fingers point up, palm toward the camera
                let world =
                    Matrix4::from_translation(vec3(GLOVE_POSITION.x + offset, y, GLOVE_POSITION.z))
                        * Matrix4::from_angle_y(Deg(90.0))
                        * Matrix4::from_angle_x(Deg(-90.0));
                scene_objects.extend(clone_with_transform(&glove.objects, world));
                scene_objects.extend(glove.debug_cubes.iter().map(|cube| {
                    let mut clone = cube.clone();
                    clone.set_transform(world * cube.get_transform());
                    clone
                }));
            }
        }
    }
}

/// Bake `pose` (or the bind pose, for `None`) into a copy of the glove model
/// and return its render objects plus per-joint debug cubes.
fn build_posed_glove(
    model: &GlbModel,
    retarget: &HandPoseRetarget,
    pose: Option<&Pose>,
    texture: Option<&Rc<dyn engine::texture::TextureTrait>>,
) -> PosedGlove {
    let mut posed_model = model.clone();
    if let Some(pose) = pose {
        retarget.apply(pose, &mut posed_model);
    }

    let mut objects = posed_model.to_scene_objects_with_skinning();
    for object in objects.iter_mut() {
        if let Some(texture) = texture {
            *object.material.borrow_mut() = SkinnedMaterial::create(texture.clone(), 1.0, 0.0);
        }
        object.set_transform(Matrix4::identity());
    }

    let debug_cubes = create_skeleton_debug_cubes(&mut posed_model, 0.005);

    PosedGlove {
        objects,
        debug_cubes,
    }
}

/// Small white cubes at every skin joint's posed position (model space).
fn create_skeleton_debug_cubes(glb_model: &mut GlbModel, cube_size: f32) -> Vec<SceneObject> {
    let mut debug_cubes = Vec::new();

    for joint in 0..glb_model.skeleton().joint_count() {
        let Some(node_index) = glb_model.skeleton().node_index_for_joint(joint) else {
            continue;
        };
        if let Some(global_transform) = glb_model.get_global_transform(node_index) {
            let bone_position = global_transform.w.truncate();

            let cube_material = color_material::create(vec3(1.0, 1.0, 1.0));
            let mut bone_cube =
                SceneObject::new(cube_material, Box::new(engine::scene::cube::create()));
            bone_cube.set_transform(
                Matrix4::from_translation(bone_position) * Matrix4::from_scale(cube_size),
            );
            debug_cubes.push(bone_cube);
        }
    }

    debug_cubes
}

fn load_glove_texture(
    asset_cache: &mut AssetCache,
) -> Option<Rc<dyn engine::texture::TextureTrait>> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        asset_cache.get::<_, engine::texture::Texture, _>(&TEXTURE_IMPORTER, "vr_glove_color.jpg")
    }))
    .ok()
    .map(|texture| texture as Rc<dyn engine::texture::TextureTrait>)
}

fn clone_with_transform(template: &[SceneObject], transform: Matrix4<f32>) -> Vec<SceneObject> {
    template
        .iter()
        .map(|object| {
            let mut clone = object.clone();
            clone.set_transform(transform);
            clone
        })
        .collect()
}

pub struct DebugGlovesScene;

impl DebugGlovesScene {
    pub fn new(
        global_context: &GlobalContext,
        game_options: &GameOptions,
        asset_cache: &mut AssetCache,
        audio_context: &mut AudioContext<EntityId, String>,
    ) -> Box<dyn GameScene> {
        let builder = DebugSceneBuilder::new("debug_gloves")
            .with_default_floor()
            // Far enough back that the whole two-row line-up fits the FOV
            .with_spawn_location(SpawnLocation::PositionRotation(
                vec3(0.0, 2.5, -0.8),
                Quaternion::from_angle_y(Deg(90.0)),
            ));

        let build_options = DebugSceneBuildOptions {
            global_context,
            game_options,
            asset_cache,
            audio_context,
        };

        let core = builder.build_core(build_options);

        let glove_model = asset_cache.get(&GLB_MODELS_IMPORTER, "vr_glove_model.glb");
        let texture = load_glove_texture(asset_cache);
        let retarget = HandPoseRetarget::for_right_glove(glove_model.skeleton());

        let open = hand_pose::open_right_hand();
        let fist = hand_pose::fist_right_hand();

        // Row 1: the reference poses
        let reference_poses: [Option<Pose>; 4] = [
            None, // bind pose
            Some(open.clone()),
            Some(hand_pose::point_right_hand()),
            Some(fist.clone()),
        ];
        // Row 2: intermediate states - open->fist blends, then a trigger
        // half-pull (only the index finger curls)
        let blend_poses: [Option<Pose>; 4] = [
            Some(open.blend(&fist, 0.25)),
            Some(open.blend(&fist, 0.5)),
            Some(open.blend(&fist, 0.75)),
            Some(open.blend_per_finger(
                &fist,
                &hand_pose::FingerAmounts {
                    index: 0.5,
                    ..Default::default()
                },
            )),
        ];

        let rows = [reference_poses, blend_poses]
            .iter()
            .map(|row| {
                row.iter()
                    .map(|pose| {
                        build_posed_glove(&glove_model, &retarget, pose.as_ref(), texture.as_ref())
                    })
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();

        info!(
            "Created debug gloves scene: row 1 bind/open/point/fist, row 2 open->fist blends + trigger half-pull + animated blend"
        );

        let animation = GloveAnimation {
            model: glove_model,
            retarget,
            texture,
            open,
            fist,
            total_time: 0.0,
        };

        let hooks = GloveHooks { rows, animation };

        Box::new(HookedDebugScene::new(core, hooks))
    }
}
