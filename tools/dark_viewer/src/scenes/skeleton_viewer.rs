//! Skeleton-only playback of one motion clip: a mesh supplies the joint
//! topology, the clip drives it, and only the bone lines are drawn - so a
//! `.mc` can be inspected without a mesh getting in the way.

use super::{ToolScene, render_helpers::create_axes_gizmo};
use cgmath::{Matrix4, Vector3};
use dark::importers::{ANIMATION_CLIP_IMPORTER, MODELS_IMPORTER};
use dark::motion::{AnimationClip, AnimationEvent, AnimationPlayer};
use engine::assets::asset_cache::AssetCache;
use engine::scene::Scene;
use std::rc::Rc;
use std::time::Duration;

pub struct SkeletonViewerScene {
    model: Rc<dark::model::Model>,
    clip: Rc<AnimationClip>,
    animation_player: AnimationPlayer,
}

impl SkeletonViewerScene {
    /// `clip_name` is the asset name the clip importer expects (`<name>_.mc`).
    pub fn new(
        mesh_key: &str,
        clip_name: &str,
        asset_cache: &mut AssetCache,
    ) -> Result<Self, String> {
        let model = asset_cache.get(&MODELS_IMPORTER, mesh_key);
        if !model.is_animated() {
            return Err(format!("'{mesh_key}' has no skeleton to pose"));
        }
        let clip = asset_cache
            .get_opt(&ANIMATION_CLIP_IMPORTER, clip_name)
            .ok_or_else(|| format!("could not load animation clip '{clip_name}'"))?;
        let animation_player =
            AnimationPlayer::queue_animation(&AnimationPlayer::empty(), clip.clone());
        Ok(SkeletonViewerScene {
            model,
            clip,
            animation_player,
        })
    }

    /// World-space joint positions for the current pose.
    fn joint_positions(&self) -> Vec<Vector3<f32>> {
        let model_transform = self.model.get_transform();
        self.model
            .get_joint_transforms(&self.animation_player)
            .iter()
            .map(|joint| {
                let world = model_transform * *joint;
                Vector3::new(world.w.x, world.w.y, world.w.z)
            })
            .collect()
    }

    /// Center and radius of the current pose's joint cloud. AI meshes report no
    /// bounding box, so this is what a caller frames the camera on.
    pub fn pose_bounds(&self) -> (Vector3<f32>, f32) {
        let positions = self.joint_positions();
        let mut min = positions[0];
        let mut max = positions[0];
        for p in &positions {
            min = Vector3::new(min.x.min(p.x), min.y.min(p.y), min.z.min(p.z));
            max = Vector3::new(max.x.max(p.x), max.y.max(p.y), max.z.max(p.z));
        }
        let center = (min + max) / 2.0;
        let extent = max - min;
        let radius = (extent.x * extent.x + extent.y * extent.y + extent.z * extent.z).sqrt() / 2.0;
        (center, radius)
    }
}

impl ToolScene for SkeletonViewerScene {
    fn update(&mut self, delta_time: f32) {
        let (player, _flags, events, _velocity) =
            AnimationPlayer::update(&self.animation_player, Duration::from_secs_f32(delta_time));
        self.animation_player = player;
        // Re-queue on completion so the clip loops.
        if events
            .iter()
            .any(|event| matches!(event, AnimationEvent::Completed))
        {
            self.animation_player =
                AnimationPlayer::queue_animation(&self.animation_player, self.clip.clone());
        }
    }

    /// Bone lines and a unit axes gizmo for scale - no ground plane, which at
    /// creature scale outshines the skeleton it is meant to sit under.
    fn render(&self, asset_cache: &mut AssetCache) -> Scene {
        let mut objects = create_axes_gizmo(asset_cache);

        let model_transform = self.model.get_transform();
        let world_joints: Vec<Matrix4<f32>> = self
            .model
            .get_joint_transforms(&self.animation_player)
            .iter()
            .map(|joint| model_transform * *joint)
            .collect();
        objects.append(&mut self.model.draw_debug_skeleton(&world_joints));

        Scene::from_objects(objects)
    }
}
