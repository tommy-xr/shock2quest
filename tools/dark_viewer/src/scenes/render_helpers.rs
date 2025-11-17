use cgmath::Matrix4;
use dark::{importers::TEXTURE_IMPORTER, model::Model, motion::AnimationPlayer};
use engine::assets::asset_cache::AssetCache;
use engine::scene::{Scene, SceneObject, basic_material, create_plane_with_uv_scale};

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
