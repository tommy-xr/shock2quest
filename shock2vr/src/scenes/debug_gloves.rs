use std::collections::HashMap;
use std::rc::Rc;

use cgmath::{
    Deg, Matrix4, Point3, Quaternion, Rotation3, SquareMatrix, Vector2, Vector3, point3, vec3,
};
use dark::{
    glb_model::GlbModel,
    importers::GLB_MODELS_IMPORTER,
    mission::{SongParams, room_database::RoomDatabase},
    ss2_entity_info::SystemShock2EntityInfo,
};
use engine::{
    assets::asset_cache::AssetCache,
    audio::AudioContext,
    scene::{SceneObject, color_material, light::SpotLight},
};
use rapier3d::prelude::{Collider, ColliderBuilder};
use shipyard::EntityId;
use tracing::info;

use crate::{
    GameOptions,
    game_scene::GameScene,
    input_context::InputContext,
    mission::{
        AbstractMission, AlwaysVisible, GlobalContext, SpawnLocation,
        entity_populator::empty_entity_populator::EmptyEntityPopulator, mission_core::MissionCore,
    },
    quest_info::QuestInfo,
    save_load::HeldItemSaveData,
    // scenes::hand_pose::*,
    scripts::{Effect, GlobalEffect},
    time::Time,
};

const FLOOR_COLOR: Vector3<f32> = Vector3::new(0.15, 0.15, 0.20);
const FLOOR_SIZE: Vector3<f32> = Vector3::new(120.0, 0.5, 120.0);
const GLOVE_POSITION: Point3<f32> = point3(0.0, 3.0, 1.0);
const GLOVE_SCALE: f32 = 1.0;

// TODO: Bring back as we add more gloves tot he scene
// const SECOND_GLOVE_OFFSET_X: f32 = 0.5;

/// Debug scene that displays the VR glove model with replaced textures
/// in front of the player for testing texture loading.
pub struct DebugGlovesScene {
    core: MissionCore,
    glove_template: Vec<SceneObject>,
    glove_model: Rc<GlbModel>,
}

impl DebugGlovesScene {
    pub fn new(
        global_context: &GlobalContext,
        game_options: &GameOptions,
        asset_cache: &mut AssetCache,
        audio_context: &mut AudioContext<EntityId, String>,
    ) -> Self {
        let abstract_mission = Self::create_debug_mission();

        let core = MissionCore::load(
            "debug_gloves".to_string(),
            abstract_mission,
            asset_cache,
            audio_context,
            global_context,
            SpawnLocation::PositionRotation(
                vec3(0.0, 2.5, 0.0),
                Quaternion::from_angle_y(Deg(90.0)),
            ),
            QuestInfo::new(),
            Box::new(EmptyEntityPopulator {}),
            HeldItemSaveData::empty(),
            game_options,
        );

        // Load the VR glove model once and instantiate it as needed.
        let glove_model = asset_cache.get(&GLB_MODELS_IMPORTER, "vr_glove_model.glb");
        let glove_template = Self::load_glove_template(asset_cache);

        info!(
            "Created debug gloves scene with {} glove nodes in template",
            glove_template.len()
        );

        Self {
            core,
            glove_template,
            glove_model,
        }
    }

    fn load_glove_template(asset_cache: &mut AssetCache) -> Vec<SceneObject> {
        use dark::importers::TEXTURE_IMPORTER;
        use engine::scene::SkinnedMaterial;

        // Load the GLB model
        let model = asset_cache.get(&GLB_MODELS_IMPORTER, "vr_glove_model.glb");
        let mut scene_objects = model.clone_scene_objects();

        // Replace textures with external vr_glove_color.jpg

        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            asset_cache
                .get::<_, engine::texture::Texture, _>(&TEXTURE_IMPORTER, "vr_glove_color.jpg")
        })) {
            Ok(external_texture) => {
                let texture_rc = external_texture as Rc<dyn engine::texture::TextureTrait>;

                // Replace materials in all scene objects
                for scene_object in scene_objects.iter_mut() {
                    // Create new material with the external texture
                    let new_material = SkinnedMaterial::create(texture_rc.clone(), 1.0, 0.0);

                    // Replace the material
                    *scene_object.material.borrow_mut() = new_material;
                }

                // Reset base transforms so instances can position themselves.
                for scene_object in scene_objects.iter_mut() {
                    scene_object.set_transform(Matrix4::identity());
                }
            }
            Err(_) => {
                // Failed to load external texture - use default materials
            }
        }

        scene_objects
    }

    // TODO: Will bring back when we test posing again
    // fn create_posed_glove(glb_model: &Rc<GlbModel>) -> GlbModel {
    //     // Clone the GLB model so we can modify it
    //     let mut posed_model = (**glb_model).clone();

    //     // Apply the pointing pose to demonstrate the corrected joint indices
    //     let pointing_pose = point_right_hand();

    //     // Apply rotations to each joint using the corrected joint mapping
    //     for (pose_index, rotation) in pointing_pose.bone_rotations.iter().enumerate() {
    //         // Use pose_index as joint_index directly since the updated hand_pose.rs
    //         // should have the correct mapping

    //         // TODO: Set joint transform
    //     }

    //     posed_model
    // }

    fn create_skeleton_debug_cubes(
        glb_model: &mut GlbModel,
        transform: Matrix4<f32>,
        cube_size: f32,
        highlight_node: Option<usize>,
    ) -> Vec<SceneObject> {
        let mut debug_cubes = Vec::new();

        // Create debug cubes for each node using the model's current animation state
        for node_index in 0..glb_model.skeleton().nodes().len() {
            if let Some(global_transform) = glb_model.get_global_transform(node_index) {
                let bone_position = global_transform.w.truncate();

                // Color scheme: highlight special node, otherwise use index-based colors
                let cube_color = if Some(node_index) == highlight_node {
                    vec3(1.0, 0.0, 0.0) // red for highlighted bone
                } else {
                    vec3(1.0, 1.0, 1.0) // white for non highlight bones
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

    fn create_debug_mission() -> AbstractMission {
        let scene_objects = Self::create_floor_scene_objects();
        let physics_geometry = Self::create_floor_physics();
        let entity_info = SystemShock2EntityInfo::empty();
        let obj_map = HashMap::new();

        AbstractMission {
            scene_objects,
            song_params: SongParams {
                song: String::new(),
            },
            room_db: RoomDatabase { rooms: Vec::new() },
            physics_geometry: Some(physics_geometry),
            spatial_data: None,
            entity_info,
            obj_map,
            visibility_engine: Box::new(AlwaysVisible),
        }
    }

    fn create_floor_scene_objects() -> Vec<SceneObject> {
        let floor_size_scaled = vec3(FLOOR_SIZE.x, FLOOR_SIZE.y, FLOOR_SIZE.z);

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
        let floor_size_scaled = vec3(FLOOR_SIZE.x / 2.0, FLOOR_SIZE.y / 2.0, FLOOR_SIZE.z / 2.0);

        ColliderBuilder::cuboid(
            floor_size_scaled.x,
            floor_size_scaled.y,
            floor_size_scaled.z,
        )
        .build()
    }
}

impl GameScene for DebugGlovesScene {
    fn update(
        &mut self,
        time: &Time,
        input_context: &InputContext,
        asset_cache: &mut AssetCache,
        game_options: &GameOptions,
        command_effects: Vec<Effect>,
    ) -> Vec<Effect> {
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
        let (mut scene_objects, camera_position, camera_rotation) =
            self.core.render(asset_cache, options);

        let transform =
            Matrix4::from_translation(vec3(GLOVE_POSITION.x, GLOVE_POSITION.y, GLOVE_POSITION.z))
                * Matrix4::from_angle_y(Deg(0.0))
                * Matrix4::from_scale(GLOVE_SCALE);

        let original_glove = Self::clone_with_transform(&self.glove_template, transform);

        // Add the static and posed glove objects to the scene
        scene_objects.extend(original_glove);

        // Create posed model for debug visualization
        // TODO: Test this out
        // let mut posed_model = Self::create_posed_glove(&self.glove_model);
        // scene_objects.extend(posed_model.to_scene_objects_with_skinning());

        // Add debug cubes for original glove (using original model)
        let original_transform =
            Matrix4::from_translation(vec3(GLOVE_POSITION.x, GLOVE_POSITION.y, GLOVE_POSITION.z))
                * Matrix4::from_scale(GLOVE_SCALE);
        let mut original_model_clone = self.glove_model.as_ref().clone();
        let original_debug_cubes = Self::create_skeleton_debug_cubes(
            &mut original_model_clone,
            original_transform,
            0.1,
            None, // No highlighting
        );
        scene_objects.extend(original_debug_cubes);

        // Add debug cubes for posed glove (using posed model with transforms applied)
        // let posed_transform = Matrix4::from_translation(vec3(
        //     GLOVE_POSITION.x + SECOND_GLOVE_OFFSET_X,
        //     GLOVE_POSITION.y,
        //     GLOVE_POSITION.z,
        // ));
        // let posed_debug_cubes = Self::create_skeleton_debug_cubes(
        //     &mut posed_model, // Now using mutable reference
        //     posed_transform,
        //     0.12, // Slightly larger
        //     None,
        // );
        // scene_objects.extend(posed_debug_cubes);
        (scene_objects, camera_position, camera_rotation)
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
