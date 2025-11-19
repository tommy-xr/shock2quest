use cgmath::{Matrix4, Vector3};
use dark::{importers::TEXTURE_IMPORTER, model::Model, motion::AnimationPlayer};
use engine::assets::asset_cache::AssetCache;
use engine::scene::{
    Scene, SceneObject, VertexPosition, basic_material, color_material, create_plane_with_uv_scale,
    cube, lines_mesh,
};

/// Compose a scene for a model, optionally overlaying debug skeletons
pub fn build_model_scene_with_debug_skeletons(
    model: &Model,
    animation_player: Option<&AnimationPlayer>,
    mut objects: Vec<SceneObject>,
    debug_skeletons: bool,
) -> Scene {
    if debug_skeletons && model.is_animated() {
        if let Some(player) = animation_player {
            objects.iter_mut().for_each(|obj| {
                obj.set_depth_write(false);
                obj.set_skinned_transparency(Some(0.35));
            });

            let joint_transforms = model.get_joint_transforms(player);
            let model_transform = model.get_transform();
            let world_joints: Vec<Matrix4<f32>> = joint_transforms
                .iter()
                .map(|joint| model_transform * *joint)
                .collect();

            let mut debug_skeleton = model.draw_debug_skeleton(&world_joints);
            objects.append(&mut debug_skeleton);
        }
    }

    Scene::from_objects(objects)
}

/// Create a ground plane SceneObject with grid texture and proper scaling
pub fn create_ground_plane(asset_cache: &mut AssetCache) -> SceneObject {
    // Load grid texture and create material with 100% emissivity and 50% transparency
    let grid_texture = asset_cache.get(&TEXTURE_IMPORTER, "grid.png");
    let texture_trait: std::rc::Rc<dyn engine::texture::TextureTrait> = grid_texture;
    let ground_material = basic_material::create(texture_trait, 1.0, 0.5);

    // Create plane with smaller UV scale (10.0 instead of default 100.0)
    let ground_plane =
        SceneObject::new(ground_material, Box::new(create_plane_with_uv_scale(10.0)));

    // Scale the ground plane to be larger (10x10 units)
    let scale_transform = Matrix4::from_scale(10.0);
    let mut ground_plane_scaled = ground_plane;
    ground_plane_scaled.set_transform(scale_transform);

    ground_plane_scaled
}

/// Create axes gizmo with a small yellow cube at origin and RGB-colored axis lines
pub fn create_axes_gizmo(_asset_cache: &mut AssetCache) -> Vec<SceneObject> {
    // Small vertical offset to prevent axes from intersecting with ground plane
    const AXES_VERTICAL_OFFSET: f32 = 0.1;

    let mut gizmo_objects = Vec::new();

    // Create small yellow cube at origin (slightly elevated)
    let yellow_material = color_material::create(Vector3::new(1.0, 1.0, 0.0)); // Yellow
    let origin_cube = SceneObject::new(yellow_material, Box::new(cube::create()));

    // Scale and position the cube
    let cube_transform = Matrix4::from_scale(0.05)
        * Matrix4::from_translation(Vector3::new(0.0, AXES_VERTICAL_OFFSET, 0.0));
    let mut positioned_cube = origin_cube;
    positioned_cube.set_transform(cube_transform);
    gizmo_objects.push(positioned_cube);

    // Create X-axis line (red) - elevated
    let x_axis_vertices = vec![
        VertexPosition {
            position: Vector3::new(0.0, AXES_VERTICAL_OFFSET, 0.0),
        }, // Elevated origin
        VertexPosition {
            position: Vector3::new(1.0, AXES_VERTICAL_OFFSET, 0.0),
        }, // Elevated +X direction
    ];
    let x_axis_geometry = lines_mesh::create(x_axis_vertices);
    let red_material = color_material::create(Vector3::new(1.0, 0.0, 0.0)); // Red
    let x_axis_line = SceneObject::new(red_material, Box::new(x_axis_geometry));
    gizmo_objects.push(x_axis_line);

    // Create Y-axis line (green) - starts elevated
    let y_axis_vertices = vec![
        VertexPosition {
            position: Vector3::new(0.0, AXES_VERTICAL_OFFSET, 0.0),
        }, // Elevated origin
        VertexPosition {
            position: Vector3::new(0.0, 1.0 + AXES_VERTICAL_OFFSET, 0.0),
        }, // +Y direction from elevated origin
    ];
    let y_axis_geometry = lines_mesh::create(y_axis_vertices);
    let green_material = color_material::create(Vector3::new(0.0, 1.0, 0.0)); // Green
    let y_axis_line = SceneObject::new(green_material, Box::new(y_axis_geometry));
    gizmo_objects.push(y_axis_line);

    // Create Z-axis line (blue) - elevated
    let z_axis_vertices = vec![
        VertexPosition {
            position: Vector3::new(0.0, AXES_VERTICAL_OFFSET, 0.0),
        }, // Elevated origin
        VertexPosition {
            position: Vector3::new(0.0, AXES_VERTICAL_OFFSET, 1.0),
        }, // Elevated +Z direction
    ];
    let z_axis_geometry = lines_mesh::create(z_axis_vertices);
    let blue_material = color_material::create(Vector3::new(0.0, 0.0, 1.0)); // Blue
    let z_axis_line = SceneObject::new(blue_material, Box::new(z_axis_geometry));
    gizmo_objects.push(z_axis_line);

    gizmo_objects
}
