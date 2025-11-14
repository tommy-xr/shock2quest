use std::collections::HashMap;
use std::rc::Rc;

use cgmath::{
    Deg, Matrix4, Point3, Quaternion, Rotation3, SquareMatrix, Vector2, Vector3, point3, vec3,
};
use dark::{
    glb_model::GlbModel,
    glb_skeleton::GlbSkeleton,
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
    scenes::hand_pose::joint_indices,
    scenes::hand_pose::*,
    scripts::{Effect, GlobalEffect},
    time::Time,
};

const FLOOR_COLOR: Vector3<f32> = Vector3::new(0.15, 0.15, 0.20);
const FLOOR_SIZE: Vector3<f32> = Vector3::new(120.0, 0.5, 120.0);
const GLOVE_POSITION: Point3<f32> = point3(0.0, 3.0, 1.0);
const GLOVE_SCALE: f32 = 1.0;
const SECOND_GLOVE_OFFSET_X: f32 = 0.5;
const POSE_GLOVE_VERTICAL_OFFSET: f32 = -0.25;

/// Debug scene that displays the VR glove model with replaced textures
/// in front of the player for testing texture loading.
pub struct DebugGlovesScene {
    core: MissionCore,
    glove_template: Vec<SceneObject>,
    glove_model: Rc<GlbModel>,
    glove_skeleton: GlbSkeleton,
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
        let glove_skeleton = glove_model.skeleton().clone();

        info!(
            "Created debug gloves scene with {} glove nodes in template",
            glove_template.len()
        );

        Self {
            core,
            glove_template,
            glove_skeleton,
            glove_model,
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

    fn create_posed_glove(glb_model: &Rc<GlbModel>) -> GlbModel {
        // Clone the GLB model so we can modify it
        let mut posed_model = (**glb_model).clone();

        // Apply the pointing pose to demonstrate the corrected joint indices
        let pointing_pose = point_right_hand();

        println!(
            "Applying pointing pose with {} rotations and {} positions",
            pointing_pose.bone_rotations.len(),
            pointing_pose.bone_positions.len()
        );

        // Define mapping from pose array index to actual GLB node index
        // Based on skeleton structure, we need to map all 31 pose indices to nodes
        let pose_to_node_mapping = [
            joint_indices::ROOT,                // 0  -> Node 2
            joint_indices::WRIST,               // 1  -> Node 3
            joint_indices::THUMB_METACARPAL,    // 2  -> Node 4
            joint_indices::THUMB_PROXIMAL,      // 3  -> Node 5
            joint_indices::THUMB_INTERMEDIATE,  // 4  -> Node 6
            joint_indices::THUMB_TIP,           // 5  -> Node 7 (finger_thumb_r_end)
            joint_indices::INDEX_METACARPAL,    // 6  -> Node 8
            joint_indices::INDEX_PROXIMAL,      // 7  -> Node 9
            joint_indices::INDEX_INTERMEDIATE,  // 8  -> Node 10
            joint_indices::INDEX_DISTAL,        // 9  -> Node 11
            joint_indices::INDEX_TIP,           // 10 -> Node 12
            joint_indices::MIDDLE_METACARPAL,   // 11 -> Node 13
            joint_indices::MIDDLE_PROXIMAL,     // 12 -> Node 14
            joint_indices::MIDDLE_INTERMEDIATE, // 13 -> Node 15
            joint_indices::MIDDLE_DISTAL,       // 14 -> Node 16
            joint_indices::MIDDLE_TIP,          // 15 -> Node 17
            joint_indices::RING_METACARPAL,     // 16 -> Node 18
            joint_indices::RING_PROXIMAL,       // 17 -> Node 19
            joint_indices::RING_INTERMEDIATE,   // 18 -> Node 20
            joint_indices::RING_DISTAL,         // 19 -> Node 21
            joint_indices::RING_TIP,            // 20 -> Node 22
            joint_indices::PINKY_METACARPAL,    // 21 -> Node 23
            joint_indices::PINKY_PROXIMAL,      // 22 -> Node 24
            joint_indices::PINKY_INTERMEDIATE,  // 23 -> Node 25
            joint_indices::PINKY_DISTAL,        // 24 -> Node 26
            joint_indices::PINKY_TIP,           // 25 -> Node 27
            28,                                 // finger_thumb_r_aux          // 26 -> Node 28
            29,                                 // finger_index_r_aux          // 27 -> Node 29
            30,                                 // finger_middle_r_aux         // 28 -> Node 30
            31,                                 // finger_ring_r_aux           // 29 -> Node 31
            32,                                 // finger_pinky_r_aux          // 30 -> Node 32
        ];

        // Apply minimal test rotations to just a few joints to debug the transform issue
        println!("DEBUGGING: Applying minimal rotations to test joints");

        // Test: Apply small rotations to a few key joints to build up gradually

        // 1. Index finger metacarpal - bend at base
        // if let Some(original_transform) = posed_model.get_node_transform(joint_indices::INDEX_METACARPAL) {
        //     let bend_rotation = Matrix4::from_angle_z(cgmath::Deg(20.0));
        //     posed_model.set_node_transform(joint_indices::INDEX_METACARPAL, original_transform * bend_rotation);
        //     println!("Applied 20-degree Z rotation to INDEX_METACARPAL");
        // }

        // 2. Index finger proximal - use joint index instead of node index
        // Based on joint mapping: Joint 7 should be finger_index_0_r (INDEX_PROXIMAL)
        let index_proximal_joint = 6; // Joint index, not node index

        println!(
            "Testing INDEX_PROXIMAL using joint index: {}",
            index_proximal_joint
        );

        if let Some(original_transform) = posed_model.get_joint_transform(index_proximal_joint) {
            // Try negative Z rotation (opposite direction)
            let bend_rotation = Matrix4::from_angle_z(cgmath::Deg(-30.0));
            posed_model
                .set_joint_transform(index_proximal_joint, original_transform * bend_rotation);
            println!(
                "Applied -30-degree Z rotation to INDEX_PROXIMAL (joint {}) (opposite direction)",
                index_proximal_joint
            );
        }

        // 3. Middle finger metacarpal - use joint index
        // Based on joint mapping: Joint 11 should be finger_middle_meta_r (MIDDLE_METACARPAL)
        // let middle_metacarpal_joint = 11; // Joint index, not node index

        // println!("Testing MIDDLE_METACARPAL using joint index: {}", middle_metacarpal_joint);

        // if let Some(original_transform) = posed_model.get_joint_transform(middle_metacarpal_joint) {
        //     let bend_rotation = Matrix4::from_angle_z(cgmath::Deg(15.0));
        //     posed_model.set_joint_transform(middle_metacarpal_joint, original_transform * bend_rotation);
        //     println!("Applied 15-degree Z rotation to MIDDLE_METACARPAL (joint {})", middle_metacarpal_joint);
        // }

        /*
        // Apply rotations to each joint, composing with original transforms
        for (pose_index, rotation) in pointing_pose.bone_rotations.iter().enumerate() {
            if let Some(&node_index) = pose_to_node_mapping.get(pose_index) {
                // CRITICAL: Skip nodes 0, 1, 2 as they control mesh and coordinate system
                if node_index <= 2 {
                    println!("Skipping system node {} to preserve mesh coordinate space", node_index);
                    continue;
                }

                // Get the original transform for this node
                if let Some(original_transform) = posed_model.get_node_transform(node_index) {
                    // Only apply non-identity rotations
                    if rotation.s != 1.0
                        || rotation.v.x != 0.0
                        || rotation.v.y != 0.0
                        || rotation.v.z != 0.0
                    {
                        let rotation_matrix = Matrix4::from(*rotation);

                        println!(
                            "Node {}: Original transform: {:?}",
                            node_index, original_transform
                        );
                        println!("Node {}: Pose rotation: {:?}", node_index, rotation);

                        // Try a simpler approach: just apply a small test rotation to the original transform
                        // to see if the issue is with the rotation composition or the pose data
                        let test_rotation = Matrix4::from_angle_y(cgmath::Deg(15.0 * pose_index as f32));
                        let composed_transform = original_transform * test_rotation;

                        posed_model.set_node_transform(node_index, composed_transform);
                        println!(
                            "Applied test rotation to node {} (pose index {})",
                            node_index, pose_index
                        );
                    }
                }
            }
        }
        */

        posed_model
    }

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

    fn create_skeleton_connection_lines(
        glb_model: &mut GlbModel,
        transform: Matrix4<f32>,
    ) -> Vec<SceneObject> {
        let mut lines = Vec::new();

        // Get skeleton data first
        let node_count = glb_model.skeleton().nodes().len();
        let mut parent_child_pairs = Vec::new();

        println!("=== Skeleton Structure ===");
        for node_index in 0..node_count {
            let node = &glb_model.skeleton().nodes()[node_index];
            println!(
                "Node {}: name={:?}, parent={:?}",
                node_index, node.name, node.parent_index
            );

            if let Some(parent_index) = node.parent_index {
                parent_child_pairs.push((parent_index, node_index));
            }
        }
        println!("=== End Skeleton Structure ===");

        // Now get transforms for each parent-child pair
        for (parent_index, child_index) in parent_child_pairs {
            if let (Some(child_transform), Some(parent_transform)) = (
                glb_model.get_global_transform(child_index),
                glb_model.get_global_transform(parent_index),
            ) {
                let child_pos = child_transform.w.truncate();
                let parent_pos = parent_transform.w.truncate();

                // Debug: print the connection being drawn
                // println!(
                //     "Drawing line: parent {} -> child {} (positions: {:?} -> {:?})",
                //     parent_index, child_index, parent_pos, child_pos
                // );

                // Apply the same coordinate scaling as the cubes
                let scaled_parent_pos = parent_pos * 1.0; // Same as cube positioning
                let scaled_child_pos = child_pos * 1.0;

                // Create a line from parent to child
                let line_object = Self::create_line_between_points(
                    scaled_parent_pos,
                    scaled_child_pos,
                    transform,
                    vec3(0.0, 1.0, 0.0), // Green lines for skeleton connections
                );
                lines.push(line_object);
            }
        }

        lines
    }

    fn create_line_between_points(
        start: Vector3<f32>,
        end: Vector3<f32>,
        transform: Matrix4<f32>,
        color: Vector3<f32>,
    ) -> SceneObject {
        use cgmath::InnerSpace;

        // Calculate line properties
        let direction = end - start;
        let length = direction.magnitude();
        let center = start + direction * 0.5;

        // Create a thin cylinder to represent the line
        let line_material = color_material::create(color);
        let mut line_object = SceneObject::new(
            line_material,
            Box::new(engine::scene::cube::create()), // Using cube as a thin line
        );

        // Calculate rotation to align with the direction vector
        let up = vec3(0.0, 1.0, 0.0);
        let rotation = if direction.magnitude() > 0.001 {
            let normalized_dir = direction.normalize();
            // Simple rotation - could be improved for arbitrary orientations
            if (normalized_dir.cross(up)).magnitude() > 0.001 {
                let axis = normalized_dir.cross(up).normalize();
                let angle = normalized_dir.dot(up).acos();
                Matrix4::from_axis_angle(axis, cgmath::Rad(angle))
            } else {
                Matrix4::identity()
            }
        } else {
            Matrix4::identity()
        };

        // Transform: position at center, rotate to align with direction, scale to line dimensions
        let line_transform = transform
            * Matrix4::from_translation(center)
            * rotation
            * Matrix4::from_nonuniform_scale(0.005, length, 0.005); // Thin line

        line_object.set_transform(line_transform);
        line_object
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

        // let posed_glove = Self::create_posed_glove_objects(&self.glove_model);

        // let static_glove_objects =
        //     Self::create_static_glove_objects(&self.glove_template, &self.glove_skeleton);

        // Add the static and posed glove objects to the scene
        scene_objects.extend(original_glove);

        // Create posed model for debug visualization
        let mut posed_model = Self::create_posed_glove(&self.glove_model);
        scene_objects.extend(posed_model.to_scene_objects_with_skinning());

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
        let posed_transform = Matrix4::from_translation(vec3(
            GLOVE_POSITION.x + SECOND_GLOVE_OFFSET_X,
            GLOVE_POSITION.y,
            GLOVE_POSITION.z,
        ));
        let posed_debug_cubes = Self::create_skeleton_debug_cubes(
            &mut posed_model, // Now using mutable reference
            posed_transform,
            0.12, // Slightly larger
            None,
            // Some(test_node_index), // Highlight the test node
        );
        scene_objects.extend(posed_debug_cubes);

        // Add skeleton connection lines for both skeletons
        let original_skeleton_lines =
            Self::create_skeleton_connection_lines(&mut original_model_clone, original_transform);
        scene_objects.extend(original_skeleton_lines);

        let posed_skeleton_lines =
            Self::create_skeleton_connection_lines(&mut posed_model, posed_transform);
        scene_objects.extend(posed_skeleton_lines);

        // panic!("render single frame to limit output");
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
