use std::collections::HashMap;
use std::rc::Rc;

use cgmath::{
    Deg, Matrix4, Point3, Quaternion, Rotation3, SquareMatrix, Vector2, Vector3, point3, vec3,
};
use dark::{
    SCALE_FACTOR,
    importers::GLB_MODELS_IMPORTER,
    mission::{SongParams, room_database::RoomDatabase},
    model::Model,
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
    scripts::{Effect, GlobalEffect},
    time::Time,
};

const FLOOR_COLOR: Vector3<f32> = Vector3::new(0.15, 0.15, 0.20);
const FLOOR_SIZE: Vector3<f32> = Vector3::new(120.0, 0.5, 120.0);
const GLOVE_POSITION: Point3<f32> = point3(0.0, 6.0 / SCALE_FACTOR, 2.0 / SCALE_FACTOR);
const GLOVE_SCALE: f32 = 2.0 / SCALE_FACTOR;

/// Debug scene that displays the VR glove model with replaced textures
/// in front of the player for testing texture loading.
pub struct DebugGlovesScene {
    core: MissionCore,
    glove_template: Vec<SceneObject>,
    glove_model: Rc<Model>,
    static_glove_objects: Vec<SceneObject>,
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
                vec3(0.0, 5.0 / SCALE_FACTOR, -5.0 / SCALE_FACTOR),
                Quaternion::from_angle_y(Deg(0.0)),
            ),
            QuestInfo::new(),
            Box::new(EmptyEntityPopulator {}),
            HeldItemSaveData::empty(),
            game_options,
        );

        // Load the VR glove model once and instantiate it as needed.
        let glove_model = asset_cache.get(&GLB_MODELS_IMPORTER, "vr_glove_model.glb");
        let glove_template = Self::load_glove_template(asset_cache);
        let static_glove_objects = Self::create_static_glove_objects(&glove_template);

        info!(
            "Created debug gloves scene with {} glove nodes in template",
            glove_template.len()
        );

        Self {
            core,
            glove_template,
            glove_model,
            static_glove_objects,
        }
    }

    fn load_glove_template(asset_cache: &mut AssetCache) -> Vec<SceneObject> {
        use dark::importers::TEXTURE_IMPORTER;
        use engine::scene::SkinnedMaterial;

        println!("Loading VR glove model for debug scene...");

        // Load the GLB model
        let model = asset_cache.get(&GLB_MODELS_IMPORTER, "vr_glove_model.glb");
        let mut scene_objects = model.clone_scene_objects();

        println!("Loaded VR glove model with {} objects", scene_objects.len());

        // Replace textures with external vr_glove_color.jpg
        println!("Loading external texture: vr_glove_color.jpg");

        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            asset_cache
                .get::<_, engine::texture::Texture, _>(&TEXTURE_IMPORTER, "vr_glove_color.jpg")
        })) {
            Ok(external_texture) => {
                println!(
                    "Successfully loaded external texture: {}x{}",
                    external_texture.width(),
                    external_texture.height()
                );

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

                println!("Successfully replaced textures on glove template");
            }
            Err(_) => {
                println!("Failed to load external texture: vr_glove_color.jpg");
            }
        }

        scene_objects
    }

    fn create_static_glove_objects(template: &[SceneObject]) -> Vec<SceneObject> {
        let transform =
            Matrix4::from_translation(vec3(GLOVE_POSITION.x, GLOVE_POSITION.y, GLOVE_POSITION.z))
                * Matrix4::from_angle_y(Deg(0.0))
                * Matrix4::from_scale(GLOVE_SCALE);

        Self::clone_with_transform(template, transform)
    }

    fn hand_transform(position: Vector3<f32>, rotation: Quaternion<f32>) -> Matrix4<f32> {
        Matrix4::from_translation(position)
            * Matrix4::from(rotation)
            * Matrix4::from_scale(GLOVE_SCALE)
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

    fn hand_glove_objects(&self) -> Vec<SceneObject> {
        let mut gloves = Vec::new();

        let left_transform = Self::hand_transform(
            self.core.left_hand.get_position(),
            self.core.left_hand.get_rotation(),
        );
        gloves.extend(Self::clone_with_transform(
            &self.glove_template,
            left_transform,
        ));

        let right_transform = Self::hand_transform(
            self.core.right_hand.get_position(),
            self.core.right_hand.get_rotation(),
        );
        gloves.extend(Self::clone_with_transform(
            &self.glove_template,
            right_transform,
        ));

        gloves
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

        // Add the static and per-hand glove objects to the scene
        scene_objects.extend(self.static_glove_objects.clone());
        scene_objects.extend(self.hand_glove_objects());

        // Add custom bone visualization for the static glove
        if let Some(skeleton) = self.glove_model.skeleton() {
            let static_transform = Matrix4::from_translation(vec3(
                GLOVE_POSITION.x,
                GLOVE_POSITION.y,
                GLOVE_POSITION.z,
            )) * Matrix4::from_scale(GLOVE_SCALE);

            let world_transforms = skeleton.world_transforms();

            // Create a cube for each bone position
            for (_bone_index, bone_transform) in world_transforms.iter().enumerate() {
                // Skip identity transforms (unused bones)
                if bone_transform != &Matrix4::identity() {
                    let bone_position = bone_transform.w.truncate();

                    // Create cube at bone position
                    let cube_material = color_material::create(Vector3::new(1.0, 0.5, 0.0)); // Orange color
                    let mut bone_cube =
                        SceneObject::new(cube_material, Box::new(engine::scene::cube::create()));

                    // Scale cube small and position it at the bone location (moved up 1 unit)
                    let cube_size = 0.02 / SCALE_FACTOR; // Small cube
                    let bone_cube_transform = static_transform
                        * Matrix4::from_translation(
                            bone_position + vec3(0.0, 1.0 / SCALE_FACTOR, 0.0),
                        )
                        * Matrix4::from_scale(cube_size);

                    bone_cube.set_transform(bone_cube_transform);
                    scene_objects.push(bone_cube);
                }
            }
        }

        (scene_objects, camera_position, camera_rotation)
    }

    fn render_per_eye(
        &mut self,
        asset_cache: &mut AssetCache,
        view: Matrix4<f32>,
        projection: Matrix4<f32>,
        screen_size: Vector2<f32>,
        options: &GameOptions,
    ) -> Vec<SceneObject> {
        let mut scene_objects =
            self.core
                .render_per_eye(asset_cache, view, projection, screen_size, options);

        // Add the glove objects to the per-eye render as well
        scene_objects.extend(self.static_glove_objects.clone());
        scene_objects.extend(self.hand_glove_objects());

        // Add custom bone visualization for the static glove
        if let Some(skeleton) = self.glove_model.skeleton() {
            let static_transform = Matrix4::from_translation(vec3(
                GLOVE_POSITION.x,
                GLOVE_POSITION.y,
                GLOVE_POSITION.z,
            )) * Matrix4::from_scale(GLOVE_SCALE);

            let world_transforms = skeleton.world_transforms();

            // Create a cube for each bone position
            for (_bone_index, bone_transform) in world_transforms.iter().enumerate() {
                // Skip identity transforms (unused bones)
                if bone_transform != &Matrix4::identity() {
                    let bone_position = bone_transform.w.truncate();

                    // Create cube at bone position
                    let cube_material = color_material::create(Vector3::new(1.0, 0.5, 0.0)); // Orange color
                    let mut bone_cube =
                        SceneObject::new(cube_material, Box::new(engine::scene::cube::create()));

                    // Scale cube small and position it at the bone location (moved up 1 unit)
                    let cube_size = 0.02 / SCALE_FACTOR; // Small cube
                    let bone_cube_transform = static_transform
                        * Matrix4::from_translation(
                            bone_position + vec3(0.0, 1.0 / SCALE_FACTOR, 0.0),
                        )
                        * Matrix4::from_scale(cube_size);

                    bone_cube.set_transform(bone_cube_transform);
                    scene_objects.push(bone_cube);
                }
            }
        }

        scene_objects
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
