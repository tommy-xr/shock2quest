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
    scenes::debug_common::{
        DebugSceneBuildOptions, DebugSceneBuilder, DebugSceneFloor, DebugSceneHooks,
        HookedDebugScene,
    },
};

const FLOOR_COLOR: Vector3<f32> = Vector3::new(0.15, 0.15, 0.20);
const FLOOR_SIZE: Vector3<f32> = Vector3::new(120.0, 0.5, 120.0);
const GLOVE_POSITION: Point3<f32> = point3(0.0, 3.0, 1.0);
const GLOVE_SCALE: f32 = 1.0;

struct GloveHooks {
    glove_template: Vec<SceneObject>,
    glove_model: Rc<GlbModel>,
}

impl DebugSceneHooks for GloveHooks {
    fn after_render(
        &mut self,
        _core: &mut MissionCore,
        scene_objects: &mut Vec<SceneObject>,
        _camera_position: &mut Vector3<f32>,
        _camera_rotation: &mut Quaternion<f32>,
        _asset_cache: &mut AssetCache,
        _options: &GameOptions,
    ) {
        let transform =
            Matrix4::from_translation(vec3(GLOVE_POSITION.x, GLOVE_POSITION.y, GLOVE_POSITION.z))
                * Matrix4::from_angle_y(Deg(0.0))
                * Matrix4::from_scale(GLOVE_SCALE);

        scene_objects.extend(clone_with_transform(&self.glove_template, transform));

        let original_transform =
            Matrix4::from_translation(vec3(GLOVE_POSITION.x, GLOVE_POSITION.y, GLOVE_POSITION.z))
                * Matrix4::from_scale(GLOVE_SCALE);
        let mut original_model_clone = self.glove_model.as_ref().clone();
        let original_debug_cubes =
            create_skeleton_debug_cubes(&mut original_model_clone, original_transform, 0.1, None);
        scene_objects.extend(original_debug_cubes);
    }
}

fn load_glove_template(asset_cache: &mut AssetCache) -> Vec<SceneObject> {
    let model = asset_cache.get(&GLB_MODELS_IMPORTER, "vr_glove_model.glb");
    let mut scene_objects = model.clone_scene_objects();

    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        asset_cache.get::<_, engine::texture::Texture, _>(&TEXTURE_IMPORTER, "vr_glove_color.jpg")
    })) {
        Ok(external_texture) => {
            let texture_rc = external_texture as Rc<dyn engine::texture::TextureTrait>;

            for scene_object in scene_objects.iter_mut() {
                let new_material = SkinnedMaterial::create(texture_rc.clone(), 1.0, 0.0);
                *scene_object.material.borrow_mut() = new_material;
            }

            for scene_object in scene_objects.iter_mut() {
                scene_object.set_transform(Matrix4::identity());
            }
        }
        Err(_) => {}
    }

    scene_objects
}

fn create_skeleton_debug_cubes(
    glb_model: &mut GlbModel,
    transform: Matrix4<f32>,
    cube_size: f32,
    highlight_node: Option<usize>,
) -> Vec<SceneObject> {
    let mut debug_cubes = Vec::new();

    for node_index in 0..glb_model.skeleton().nodes().len() {
        if let Some(global_transform) = glb_model.get_global_transform(node_index) {
            let bone_position = global_transform.w.truncate();

            let cube_color = if Some(node_index) == highlight_node {
                vec3(1.0, 0.0, 0.0)
            } else {
                vec3(1.0, 1.0, 1.0)
            };

            let cube_material = color_material::create(cube_color);
            let mut bone_cube =
                SceneObject::new(cube_material, Box::new(engine::scene::cube::create()));

            let bone_cube_transform = transform
                * Matrix4::from_translation(bone_position * 1.0)
                * Matrix4::from_scale(cube_size * 0.2);

            bone_cube.set_transform(bone_cube_transform);
            debug_cubes.push(bone_cube);
        }
    }

    debug_cubes
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
            .with_floor(DebugSceneFloor::world_units(FLOOR_SIZE, FLOOR_COLOR))
            .with_spawn_location(SpawnLocation::PositionRotation(
                vec3(0.0, 2.5, 0.0),
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
        let glove_template = load_glove_template(asset_cache);

        info!(
            "Created debug gloves scene with {} glove nodes in template",
            glove_template.len()
        );

        let hooks = GloveHooks {
            glove_template,
            glove_model,
        };

        Box::new(HookedDebugScene::new(core, hooks))
    }
}
