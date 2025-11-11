use cgmath::Matrix4;
use dark::{model::Model, motion::AnimationPlayer};
use engine::scene::{Scene, SceneObject};

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
