use std::collections::HashMap;
use std::rc::Rc;

use cgmath::{
    Deg, InnerSpace, Matrix4, Point3, Quaternion, Rotation3, SquareMatrix, Vector2, Vector3,
    point3, vec3,
};
use dark::{
    SCALE_FACTOR,
    importers::GLB_MODELS_IMPORTER,
    mission::{SongParams, room_database::RoomDatabase},
    model::Model,
    ss2_entity_info::SystemShock2EntityInfo,
    ss2_skeleton::Skeleton,
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
    hand_pose,
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
const SECOND_GLOVE_OFFSET_X: f32 = 0.75 / SCALE_FACTOR;
const DEBUG_RENDER_BONE_COUNT: usize = hand_pose::AUX_BONE_START_INDEX;
const MAX_DEBUG_BONE_INDEX: usize = DEBUG_RENDER_BONE_COUNT - 1;
const POSE_GLOVE_VERTICAL_OFFSET: f32 = -1.5 / SCALE_FACTOR;

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
        let static_glove_objects =
            Self::create_static_glove_objects(&glove_template, glove_model.skeleton());

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

    fn create_static_glove_objects(
        template: &[SceneObject],
        skeleton: Option<&Skeleton>,
    ) -> Vec<SceneObject> {
        let transform =
            Matrix4::from_translation(vec3(GLOVE_POSITION.x, GLOVE_POSITION.y, GLOVE_POSITION.z))
                * Matrix4::from_angle_y(Deg(0.0))
                * Matrix4::from_scale(GLOVE_SCALE);

        let mut objects = Self::clone_with_transform(template, transform);

        if let Some(skeleton) = skeleton {
            objects.extend(Self::create_manual_skinning_glove_objects(
                template, skeleton,
            ));
        }

        objects
    }

    fn create_manual_skinning_glove_objects(
        template: &[SceneObject],
        skeleton: &Skeleton,
    ) -> Vec<SceneObject> {
        // Nudge the manual-skin glove next to the debug cubes for visual comparison.
        let manual_transform = Matrix4::from_translation(vec3(
            GLOVE_POSITION.x + SECOND_GLOVE_OFFSET_X,
            GLOVE_POSITION.y,
            GLOVE_POSITION.z,
        )) * Matrix4::from_angle_y(Deg(0.0))
            * Matrix4::from_scale(1.0);

        let skinning_data = Self::manual_skinning_data(skeleton);

        template
            .iter()
            .map(|object| {
                let mut clone = object.clone();
                clone.set_transform(manual_transform);
                clone.set_skinning_data(skinning_data);
                clone
            })
            .collect()
    }

    fn manual_skinning_data(skeleton: &Skeleton) -> [Matrix4<f32>; 40] {
        let world_transforms = skeleton.world_transforms();
        let mut skinning_data = [Matrix4::identity(); 40];

        for (joint_index, world_transform) in world_transforms.iter().enumerate() {
            if world_transform == &Matrix4::identity() {
                continue;
            }

            let inverse_bind = skeleton
                .rest_transform(joint_index as u32)
                .map(|rest| rest.inverse_bind)
                .unwrap_or_else(Matrix4::identity);

            skinning_data[joint_index] = *world_transform * inverse_bind;
        }

        skinning_data
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

    fn pointing_pose_offset() -> Vector3<f32> {
        vec3(4.0 / SCALE_FACTOR, 6.0 / SCALE_FACTOR, 2.0 / SCALE_FACTOR)
    }

    fn open_pose_offset() -> Vector3<f32> {
        vec3(8.0 / SCALE_FACTOR, 6.0 / SCALE_FACTOR, 2.0 / SCALE_FACTOR)
    }

    fn glove_translation_for_pose(pose_offset: Vector3<f32>) -> Vector3<f32> {
        pose_offset + vec3(0.0, POSE_GLOVE_VERTICAL_OFFSET, 0.0)
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

    fn create_hand_pose_debug_cubes() -> Vec<SceneObject> {
        let pose = hand_pose::point_right_hand();
        let relationships = hand_pose::joint_relationships();
        let mut objects = Vec::new();

        // Get global positions by transforming through the bone hierarchy
        let global_positions = pose.global_bone_positions();

        // Position the pose cubes to the right of the current debug rendering
        let pose_offset = Self::pointing_pose_offset();
        let pose_scale = 1.0;

        // Create cubes for each bone position
        for (bone_index, bone_position) in global_positions
            .iter()
            .enumerate()
            .take(DEBUG_RENDER_BONE_COUNT)
        {
            let bone_pos_cgmath = *bone_position;

            // Create a small cube for each bone position
            let cube_color = if bone_index == hand_pose::joint_indices::INDEX_PROXIMAL {
                Vector3::new(1.0, 0.0, 0.0) // Red for index proximal joint
            } else {
                Vector3::new(0.0, 1.0, 0.5) // Cyan-green for other bones
            };
            let cube_material = color_material::create(cube_color);
            let mut pose_cube =
                SceneObject::new(cube_material, Box::new(engine::scene::cube::create()));

            // Scale and position the cube
            let cube_size = 0.03 / SCALE_FACTOR; // Slightly larger than bone cubes for visibility
            let cube_transform = Matrix4::from_translation(pose_offset)
                * Matrix4::from_scale(pose_scale)
                * Matrix4::from_translation(bone_pos_cgmath)
                * Matrix4::from_scale(cube_size);

            pose_cube.set_transform(cube_transform);
            objects.push(pose_cube);
        }

        // Create debug lines connecting parent and child bones
        for (&child_index, &parent_index) in &relationships {
            if child_index > MAX_DEBUG_BONE_INDEX || parent_index > MAX_DEBUG_BONE_INDEX {
                continue;
            }

            if child_index < global_positions.len() && parent_index < global_positions.len() {
                let child_pos = global_positions[child_index];
                let parent_pos = global_positions[parent_index];

                // Create a thin cylinder to represent the connection line
                let line_material = color_material::create(Vector3::new(1.0, 1.0, 0.0)); // Yellow lines
                let mut line_object = SceneObject::new(
                    line_material,
                    Box::new(engine::scene::cube::create()), // Using cube as a thin line
                );

                // Calculate the line properties
                let direction = child_pos - parent_pos;
                let length = direction.magnitude();

                if length > 0.0 {
                    let midpoint = parent_pos + direction * 0.5;
                    let normalized_direction = direction / length;

                    // Create transform for the line
                    let line_thickness = 0.005 / SCALE_FACTOR; // Very thin line

                    // Create a rotation matrix to align the line with the bone direction
                    // Default cube extends along Z-axis, so we rotate to align with our direction
                    let up = Vector3::unit_z();
                    let rotation_matrix = if (normalized_direction.dot(up)).abs() < 0.99 {
                        // Safe to use cross product
                        let right = normalized_direction.cross(up).normalize();
                        let actual_up = right.cross(normalized_direction);
                        Matrix4::from_cols(
                            right.extend(0.0),
                            actual_up.extend(0.0),
                            normalized_direction.extend(0.0),
                            Vector3::new(0.0, 0.0, 0.0).extend(1.0),
                        )
                    } else {
                        // Direction is parallel to up vector, use a different approach
                        Matrix4::identity()
                    };

                    let line_transform = Matrix4::from_translation(pose_offset)
                        * Matrix4::from_scale(pose_scale)
                        * Matrix4::from_translation(midpoint)
                        * rotation_matrix
                        * Matrix4::from_nonuniform_scale(line_thickness, line_thickness, length);

                    line_object.set_transform(line_transform);
                    objects.push(line_object);
                }
            }
        }

        objects
    }

    fn create_open_pose_debug_cubes() -> Vec<SceneObject> {
        let pose = hand_pose::open_right_hand();
        let relationships = hand_pose::joint_relationships();
        let mut objects = Vec::new();

        // Get global positions by transforming through the bone hierarchy
        let global_positions = pose.global_bone_positions();

        // Position the open pose cubes even further to the right
        let pose_offset = Self::open_pose_offset();
        let pose_scale = 1.0;

        // Create cubes for each bone position
        for (bone_index, bone_position) in global_positions
            .iter()
            .enumerate()
            .take(DEBUG_RENDER_BONE_COUNT)
        {
            let bone_pos_cgmath = *bone_position;

            // Create a small cube for each bone position
            let cube_color = if bone_index == 0 {
                Vector3::new(1.0, 0.0, 0.0) // Red for wrist (bone index 0)
            } else {
                Vector3::new(0.0, 0.8, 0.8) // Teal for other bones to distinguish from other poses
            };
            let cube_material = color_material::create(cube_color);
            let mut pose_cube =
                SceneObject::new(cube_material, Box::new(engine::scene::cube::create()));

            // Scale and position the cube
            let cube_size = 0.03 / SCALE_FACTOR;
            let cube_transform = Matrix4::from_translation(pose_offset)
                * Matrix4::from_scale(pose_scale)
                * Matrix4::from_translation(bone_pos_cgmath)
                * Matrix4::from_scale(cube_size);

            pose_cube.set_transform(cube_transform);
            objects.push(pose_cube);
        }

        // Create debug lines connecting parent and child bones
        for (&child_index, &parent_index) in &relationships {
            if child_index > MAX_DEBUG_BONE_INDEX || parent_index > MAX_DEBUG_BONE_INDEX {
                continue;
            }

            if child_index < global_positions.len() && parent_index < global_positions.len() {
                let child_pos = global_positions[child_index];
                let parent_pos = global_positions[parent_index];

                // Create a thin cylinder to represent the connection line
                let line_material = color_material::create(Vector3::new(0.0, 1.0, 1.0)); // Cyan lines for open pose
                let mut line_object =
                    SceneObject::new(line_material, Box::new(engine::scene::cube::create()));

                // Calculate the line properties
                let direction = child_pos - parent_pos;
                let length = direction.magnitude();

                if length > 0.0 {
                    let midpoint = parent_pos + direction * 0.5;
                    let normalized_direction = direction / length;

                    // Create transform for the line
                    let line_thickness = 0.005 / SCALE_FACTOR;

                    // Create a rotation matrix to align the line with the bone direction
                    let up = Vector3::unit_z();
                    let rotation_matrix = if (normalized_direction.dot(up)).abs() < 0.99 {
                        let right = normalized_direction.cross(up).normalize();
                        let actual_up = right.cross(normalized_direction);
                        Matrix4::from_cols(
                            right.extend(0.0),
                            actual_up.extend(0.0),
                            normalized_direction.extend(0.0),
                            Vector3::new(0.0, 0.0, 0.0).extend(1.0),
                        )
                    } else {
                        Matrix4::identity()
                    };

                    let line_transform = Matrix4::from_translation(pose_offset)
                        * Matrix4::from_scale(pose_scale)
                        * Matrix4::from_translation(midpoint)
                        * rotation_matrix
                        * Matrix4::from_nonuniform_scale(line_thickness, line_thickness, length);

                    line_object.set_transform(line_transform);
                    objects.push(line_object);
                }
            }
        }

        objects
    }

    fn pose_glove_objects(&self) -> Vec<SceneObject> {
        let mut pose_gloves = Vec::new();
        let skeleton = match self.glove_model.skeleton() {
            Some(skeleton) => skeleton,
            None => return pose_gloves,
        };

        let pointing_pose = hand_pose::point_right_hand();
        pose_gloves.extend(self.create_pose_glove_objects(
            skeleton,
            &pointing_pose,
            Self::pointing_pose_offset(),
        ));

        let open_pose = hand_pose::open_right_hand();
        pose_gloves.extend(self.create_pose_glove_objects(
            skeleton,
            &open_pose,
            Self::open_pose_offset(),
        ));

        pose_gloves
    }

    fn create_pose_glove_objects(
        &self,
        skeleton: &Skeleton,
        pose: &hand_pose::Pose,
        pose_offset: Vector3<f32>,
    ) -> Vec<SceneObject> {
        let glove_translation = Self::glove_translation_for_pose(pose_offset);
        let pose_transform =
            Matrix4::from_translation(glove_translation) * Matrix4::from_scale(GLOVE_SCALE);
        let mut glove_objects = Self::clone_with_transform(&self.glove_template, pose_transform);
        let skinning_data = Self::pose_skinning_data(skeleton, pose);

        for glove_object in glove_objects.iter_mut() {
            glove_object.set_skinning_data(skinning_data);
        }

        glove_objects
    }

    fn pose_skinning_data(skeleton: &Skeleton, pose: &hand_pose::Pose) -> [Matrix4<f32>; 40] {
        let global_transforms = pose.global_bone_transforms();
        let mut skinning_data = [Matrix4::identity(); 40];

        let skeleton_bone_count = skeleton.bone_count().min(40);
        let pose_bone_count = global_transforms.len().min(skeleton_bone_count);

        for bone_index in 0..pose_bone_count {
            let joint_id = bone_index as u32;
            let mut final_transform = global_transforms[bone_index];

            if let Some(rest) = skeleton.rest_transform(joint_id) {
                final_transform = final_transform * rest.inverse_bind;
            } else {
                panic!("no rest pose");
            }

            skinning_data[bone_index] = final_transform;
        }

        skinning_data
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
        scene_objects.extend(self.pose_glove_objects());

        // Add custom bone visualization for the static glove
        if let Some(skeleton) = self.glove_model.skeleton() {
            let static_transform = Matrix4::from_translation(vec3(
                GLOVE_POSITION.x,
                GLOVE_POSITION.y,
                GLOVE_POSITION.z,
            )) * Matrix4::from_scale(GLOVE_SCALE);

            let world_transforms = skeleton.world_transforms();

            // Create a cube for each bone position
            for bone_transform in world_transforms.iter().take(DEBUG_RENDER_BONE_COUNT) {
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

        // Add hand pose debug cubes
        scene_objects.extend(Self::create_hand_pose_debug_cubes());

        // Add open pose debug cubes
        scene_objects.extend(Self::create_open_pose_debug_cubes());

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
        scene_objects.extend(self.pose_glove_objects());

        // Add custom bone visualization for the static glove
        if let Some(skeleton) = self.glove_model.skeleton() {
            let static_transform = Matrix4::from_translation(vec3(
                GLOVE_POSITION.x,
                GLOVE_POSITION.y,
                GLOVE_POSITION.z,
            )) * Matrix4::from_scale(GLOVE_SCALE);

            let world_transforms = skeleton.world_transforms();

            // Create a cube for each bone position
            for bone_transform in world_transforms.iter().take(DEBUG_RENDER_BONE_COUNT) {
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

        // Add hand pose debug cubes
        scene_objects.extend(Self::create_hand_pose_debug_cubes());

        // Add open pose debug cubes
        scene_objects.extend(Self::create_open_pose_debug_cubes());

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
