//! Skeleton-only playback of one motion clip: a mesh supplies the joint
//! topology, the clip drives it, and only the bone lines are drawn - so a
//! `.mc` can be inspected without a mesh getting in the way.

use super::{
    ToolScene,
    render_helpers::{create_axes_gizmo, world_joint_transforms},
};
use cgmath::Vector3;
use dark::importers::{ANIMATION_CLIP_IMPORTER, MODELS_IMPORTER};
use dark::motion::AnimationPlayer;
use engine::assets::asset_cache::AssetCache;
use engine::scene::Scene;
use std::rc::Rc;
use std::time::Duration;

pub struct SkeletonViewerScene {
    model: Rc<dark::model::Model>,
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
        // Loop the clip natively: re-queueing on completion would cross-fade an
        // unauthored blend into every loop seam.
        let animation_player = AnimationPlayer::from_animation(clip);
        Ok(SkeletonViewerScene {
            model,
            animation_player,
        })
    }

    /// World-space positions of the joints the skeleton actually uses. The
    /// palette has 40 slots and unused ones stay identity, so indexing by bone
    /// keeps the origin out of the bounds below.
    fn joint_positions(&self) -> Vec<Vector3<f32>> {
        let world_joints = world_joint_transforms(&self.model, &self.animation_player);
        let Some(skeleton) = self.model.skeleton() else {
            return Vec::new();
        };
        skeleton
            .bones()
            .iter()
            .filter_map(|bone| world_joints.get(bone.joint_id as usize))
            .map(|world| Vector3::new(world.w.x, world.w.y, world.w.z))
            .collect()
    }

    /// Center and radius of the current pose's joint cloud. AI meshes report no
    /// bounding box, so this is what a caller frames the camera on. Sampled at
    /// the current pose only - framing happens at t=0.
    pub fn pose_bounds(&self) -> (Vector3<f32>, f32) {
        let positions = self.joint_positions();
        if positions.is_empty() {
            return (Vector3::new(0.0, 0.0, 0.0), 1.0);
        }
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
        let (player, _flags, _events, _velocity) =
            AnimationPlayer::update(&self.animation_player, Duration::from_secs_f32(delta_time));
        self.animation_player = player;
    }

    /// Bone lines and a unit axes gizmo for scale - no ground plane, which at
    /// creature scale outshines the skeleton it is meant to sit under.
    fn render(&self, asset_cache: &mut AssetCache) -> Scene {
        let mut objects = create_axes_gizmo(asset_cache);

        let world_joints = world_joint_transforms(&self.model, &self.animation_player);
        objects.append(&mut self.model.draw_debug_skeleton(&world_joints));

        Scene::from_objects(objects)
    }
}
