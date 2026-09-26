use cgmath::{EuclideanSpace, Matrix4, Vector3};
use dark::{importers::TEXTURE_IMPORTER, model::Model, motion::AnimationPlayer};
use engine::assets::asset_cache::AssetCache;
use engine::scene::{
    Scene, SceneObject, VertexPosition, basic_material, color_material, create_plane_with_uv_scale,
    cube, lines_mesh,
};

/// Color of the fitted hit-box wireframe overlay (bright green).
const HIT_BOX_OVERLAY_COLOR: Vector3<f32> = Vector3::new(0.2, 1.0, 0.3);

/// Articulation overlay: LGMD sub-object pivots (magenta) and vhots (yellow).
const SUB_OBJECT_COLOR: Vector3<f32> = Vector3::new(1.0, 0.2, 0.85);
const VHOT_COLOR: Vector3<f32> = Vector3::new(1.0, 0.85, 0.1);
const SUB_OBJECT_MARKER_SIZE: f32 = 0.05;
const VHOT_MARKER_SIZE: f32 = 0.035;

/// How much of the mesh shows through under an overlay (0 = opaque).
const OVERLAY_GHOST_TRANSPARENCY: f32 = 0.35;

/// Markers for a static `.bin`'s articulation points: one cube per sub-object
/// pivot (the parts tweqs rotate/translate) and one per vhot (attachment
/// points - muzzle, light, particle origins). Empty for LGMM/GLB models.
pub fn articulation_overlay(model: &Model) -> Vec<SceneObject> {
    let model_transform = model.get_transform();
    let marker = |color: Vector3<f32>, at: Matrix4<f32>, size: f32| {
        let mut obj = SceneObject::new(color_material::create(color), Box::new(cube::create()));
        obj.set_transform(model_transform * at * Matrix4::from_scale(size));
        obj
    };

    let mut objects = model
        .sub_objects()
        .iter()
        .map(|sub_object| {
            // Pivot only: the sub-object's rotation would skew the cube.
            let at = Matrix4::from_translation(sub_object.transform.w.truncate());
            marker(SUB_OBJECT_COLOR, at, SUB_OBJECT_MARKER_SIZE)
        })
        .collect::<Vec<SceneObject>>();

    objects.extend(model.vhots().iter().map(|vhot| {
        let at = Matrix4::from_translation(vhot.point.to_vec());
        marker(VHOT_COLOR, at, VHOT_MARKER_SIZE)
    }));

    objects
}

/// The model's joint palette in world space: each joint transform with the
/// model transform applied, as the debug skeleton and hit-box overlays want it.
pub fn world_joint_transforms(model: &Model, player: &AnimationPlayer) -> Vec<Matrix4<f32>> {
    let model_transform = model.get_transform();
    model
        .get_joint_transforms(player)
        .iter()
        .map(|joint| model_transform * *joint)
        .collect()
}

/// Compose a scene for a model, optionally overlaying debug skeletons, the
/// fitted per-joint hit-box shapes (`dark::hit_box`), and/or the LGMD
/// articulation markers. The hit-box overlay renders exactly what
/// `fit_hit_box_shapes` produced, transformed by the live joint transforms - so
/// it tracks the animated mesh and reveals fit/mapping issues independent of the
/// physics ragdoll.
///
/// `decorations` (ground plane, axes gizmo) are kept apart from the model's own
/// objects so that ghosting under an overlay touches only the mesh.
pub fn build_model_scene_with_debug_skeletons(
    model: &Model,
    animation_player: Option<&AnimationPlayer>,
    mut model_objects: Vec<SceneObject>,
    mut decorations: Vec<SceneObject>,
    debug_skeletons: bool,
    debug_hit_boxes: bool,
    debug_articulation: bool,
) -> Scene {
    let mut overlay = Vec::new();

    if (debug_skeletons || debug_hit_boxes) && model.is_animated() {
        if let Some(player) = animation_player {
            let world_joints = world_joint_transforms(model, player);

            if debug_skeletons {
                overlay.append(&mut model.draw_debug_skeleton(&world_joints));
            }

            if debug_hit_boxes {
                overlay.append(&mut dark::hit_box::draw_debug_hit_box_shapes(
                    &model.hit_box_shapes(),
                    &world_joints,
                    HIT_BOX_OVERLAY_COLOR,
                ));
            }
        }
    }

    if debug_articulation {
        overlay.append(&mut articulation_overlay(model));
    }

    // Ghost the model (and only the model - not the grid) so overlay geometry
    // buried inside the mesh still reads.
    if !overlay.is_empty() {
        model_objects.iter_mut().for_each(|obj| {
            obj.set_depth_write(false);
            obj.set_transparency(Some(OVERLAY_GHOST_TRANSPARENCY));
        });
    }

    model_objects.append(&mut decorations);
    model_objects.append(&mut overlay);
    Scene::from_objects(model_objects)
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
