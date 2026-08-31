use std::{collections::HashMap, rc::Rc};

use crate::{
    hit_box::{HitBoxShape, fit_hit_box_shapes},
    motion::{AnimationClip, AnimationPlayer},
    ss2_bin_ai_loader::{self, SystemShock2AIMesh},
    ss2_bin_obj_loader::{self, SystemShock2ObjectMesh, Vhot},
    ss2_skeleton::{self, AnimationInfo, Bone, Skeleton},
};
use cgmath::{Matrix4, SquareMatrix, Vector2};
use collision::Aabb3;
use engine::{
    assets::asset_cache::AssetCache,
    scene::{FrontFaceWinding, MAX_SKINNED_JOINTS, SKINNING_PALETTE_SIZE, SceneObject},
};

/// The opposite winding: what a mirrored copy of a mesh presents to the GPU.
fn flip_winding(winding: FrontFaceWinding) -> FrontFaceWinding {
    match winding {
        FrontFaceWinding::Clockwise => FrontFaceWinding::CounterClockwise,
        FrontFaceWinding::CounterClockwise => FrontFaceWinding::Clockwise,
    }
}

/// One LGMD sub-object: a named part of a static `.bin` (the pieces an in-game
/// tweq rotates or translates - a gun slide, a door leaf) and its pivot in
/// model space. Empty for LGMM/AI and GLB models, which articulate via joints.
#[derive(Clone, Debug)]
pub struct SubObject {
    pub name: String,
    pub transform: Matrix4<f32>,
}

#[derive(Clone)]
pub struct StaticModel {
    scene_objects: Vec<SceneObject>,
    bounding_box: Aabb3<f32>,
    vhots: Vec<Vhot>,
    sub_objects: Vec<SubObject>,
}

impl StaticModel {
    fn to_scene_objects(&self) -> &Vec<SceneObject> {
        &self.scene_objects
    }

    #[allow(dead_code)]
    fn pose(&mut self, _animation_clip: &AnimationClip) {}

    fn transform(model: &StaticModel, transform: Matrix4<f32>) -> StaticModel {
        let new_scene_objects = model
            .scene_objects
            .iter()
            .map(|m| {
                let mut new_obj = m.clone();
                new_obj.set_transform(transform);
                new_obj
            })
            .collect::<Vec<SceneObject>>();

        StaticModel {
            scene_objects: new_scene_objects,
            bounding_box: model.bounding_box,
            vhots: model.vhots.clone(),
            sub_objects: model.sub_objects.clone(),
        }
    }
}

#[derive(Clone)]
pub struct AnimatedModel {
    skeleton: Rc<Skeleton>,
    scene_objects: Vec<SceneObject>,
    hit_boxes: Rc<HashMap<u32, Aabb3<f32>>>,
    hit_box_shapes: Rc<HashMap<u32, HitBoxShape>>,
    vhots: Vec<Vhot>,
    sub_objects: Vec<SubObject>,
    /// Set only for a `PMNM` mesh, whose vertices are in bind-pose model space
    /// rather than joint-local space. Holds `bind_inverse[j] * bind_correction`,
    /// so the posed palette becomes `pose[j] * bind[j]`.
    bind: Option<Rc<[Matrix4<f32>; MAX_SKINNED_JOINTS]>>,
}

/// Build the render palette, undoing the bind pose first when the geometry needs
/// it. `expand_skinning_palette`'s stretchy parent frames are a vanilla-LGMM
/// concept, so a bind-space mesh takes the plain per-joint product.
fn build_palette(
    pose: &[Matrix4<f32>; MAX_SKINNED_JOINTS],
    skeleton: &Skeleton,
    bind: Option<&Rc<[Matrix4<f32>; MAX_SKINNED_JOINTS]>>,
) -> [Matrix4<f32>; SKINNING_PALETTE_SIZE] {
    match bind {
        None => Skeleton::expand_skinning_palette(pose, skeleton),
        Some(bind) => {
            let mut combined = [Matrix4::identity(); MAX_SKINNED_JOINTS];
            for j in 0..MAX_SKINNED_JOINTS {
                combined[j] = pose[j] * bind[j];
            }
            let mut palette = [Matrix4::identity(); SKINNING_PALETTE_SIZE];
            palette[..MAX_SKINNED_JOINTS].copy_from_slice(&combined);
            palette[MAX_SKINNED_JOINTS..].copy_from_slice(&combined);
            palette
        }
    }
}

impl AnimatedModel {
    fn to_scene_objects(&self) -> &Vec<SceneObject> {
        &self.scene_objects
    }

    fn to_animated_scene_objects(&self, player: &AnimationPlayer) -> Vec<SceneObject> {
        let pose = player.get_transforms(&self.skeleton);
        let palette = build_palette(&pose, &self.skeleton, self.bind.as_ref());

        self.scene_objects
            .iter()
            .map(|m| {
                let mut new_obj = m.clone();
                new_obj.set_skinning_palette(palette);
                new_obj
            })
            .collect::<Vec<SceneObject>>()
    }

    fn animate(&self, animation_clip: &AnimationClip, frame: u32) -> AnimatedModel {
        let animated_skeleton = ss2_skeleton::animate(
            &self.skeleton,
            Some(AnimationInfo {
                animation_clip: &animation_clip,
                frame,
                fraction: 0.0,
                wrap: false,
                cancel_root_motion: false,
            }),
            &rpds::HashTrieMap::new(),
        );
        let new_data = build_palette(
            &animated_skeleton.get_transforms(),
            &animated_skeleton,
            self.bind.as_ref(),
        );

        let new_scene_objects = self
            .scene_objects
            .iter()
            .map(|m| {
                let mut new_obj = m.clone();
                new_obj.set_skinning_palette(new_data);
                new_obj
            })
            .collect::<Vec<SceneObject>>();

        AnimatedModel {
            //mesh: self.mesh.clone(),
            skeleton: self.skeleton.clone(),
            scene_objects: new_scene_objects,
            hit_boxes: self.hit_boxes.clone(),
            hit_box_shapes: self.hit_box_shapes.clone(),
            vhots: self.vhots.clone(),
            sub_objects: self.sub_objects.clone(),
            bind: self.bind.clone(),
        }
    }

    fn transform(model: &AnimatedModel, transform: Matrix4<f32>) -> AnimatedModel {
        let new_scene_objects = model
            .scene_objects
            .iter()
            .map(|m| {
                let mut new_obj = m.clone();
                new_obj.set_transform(transform);
                new_obj
            })
            .collect::<Vec<SceneObject>>();

        AnimatedModel {
            //mesh: model.mesh.clone(),
            skeleton: model.skeleton.clone(),
            scene_objects: new_scene_objects,
            hit_boxes: model.hit_boxes.clone(),
            hit_box_shapes: model.hit_box_shapes.clone(),
            vhots: model.vhots.clone(),
            sub_objects: model.sub_objects.clone(),
            bind: model.bind.clone(),
        }
    }

    fn pose(&self, animation_clip: &AnimationClip) -> AnimatedModel {
        self.animate(animation_clip, 1)
    }
}

#[derive(Clone)]
pub enum InnerModel {
    Static(StaticModel),
    Animated(AnimatedModel),
}

#[derive(Clone)]
pub struct Model {
    inner: InnerModel,
    transform: Matrix4<f32>,
}

impl Model {
    pub fn from_obj_bin(
        static_mesh: SystemShock2ObjectMesh,
        asset_cache: &mut AssetCache,
    ) -> Model {
        let (scene_objects, skeleton) =
            ss2_bin_obj_loader::to_scene_objects(&static_mesh, asset_cache);
        let bounding_box = static_mesh.bounding_box;

        // Sub-object pivots, in model space: the skeleton the obj loader just
        // built from the sub-object tree is exactly that hierarchy resolved.
        let sub_objects = static_mesh
            .sub_objects
            .iter()
            .enumerate()
            .map(|(index, sub_object)| SubObject {
                name: sub_object.name.clone(),
                transform: skeleton.global_transform(&(index as u32)),
            })
            .collect::<Vec<SubObject>>();

        if skeleton.bone_count() > 1 {
            let hit_boxes = HashMap::new();
            Model {
                transform: Matrix4::identity(),
                inner: InnerModel::Animated(AnimatedModel {
                    skeleton: Rc::new(skeleton),
                    scene_objects,
                    hit_boxes: Rc::new(hit_boxes),
                    hit_box_shapes: Rc::new(HashMap::new()),
                    vhots: static_mesh.vhots.clone(),
                    sub_objects,
                    bind: None,
                }),
            }
        } else {
            Model {
                transform: Matrix4::identity(),
                inner: InnerModel::Static(StaticModel {
                    scene_objects,
                    bounding_box,
                    vhots: static_mesh.vhots.clone(),
                    sub_objects,
                }),
            }
        }
    }

    pub fn from_ai_bin(
        ai_mesh: SystemShock2AIMesh,
        skeleton: Rc<Skeleton>,
        pmnm: Option<crate::ss2_bin_pmnm::PmnmMesh>,
        asset_cache: &mut AssetCache,
    ) -> Model {
        // Render the high-detail chunk when we have one, and in that case skip
        // building the original mesh's scene objects entirely - they would be
        // discarded, and building them uploads a vertex buffer and decodes every
        // original texture per creature. Hitboxes still come from the original
        // mesh, so damage locations and ragdoll *physics* fitting are untouched.
        // Ragdoll *rendering* is not automatic: it poses `clone_scene_objects()`
        // from physics rather than from an `AnimationPlayer`, so it reads
        // `bind_matrices()` and folds the same product in - see
        // `RagDoll::renderables`.
        let high_detail = pmnm.and_then(|pmnm| {
            let objects = ss2_bin_ai_loader::pmnm_to_scene_objects(&pmnm, asset_cache);
            (!objects.is_empty()).then_some(objects)
        });

        let (scene_objects, hit_boxes, bind) = match high_detail {
            None => {
                let (objects, hit_boxes) =
                    ss2_bin_ai_loader::to_scene_objects(&ai_mesh, &skeleton, asset_cache);
                (objects, hit_boxes, None)
            }
            Some(objects) => {
                let (_, hit_boxes) = ss2_bin_ai_loader::to_vertices(&ai_mesh, &skeleton);
                let matrices = Rc::new(ss2_bin_ai_loader::pmnm_bind_matrices(&skeleton));
                // Bake the rest palette so an un-animated draw is correct too,
                // not just the animated paths.
                let rest = build_palette(&skeleton.get_transforms(), &skeleton, Some(&matrices));
                let objects = objects
                    .into_iter()
                    .map(|mut o| {
                        o.set_skinning_palette(rest);
                        o
                    })
                    .collect();
                (objects, hit_boxes, Some(matrices))
            }
        };
        let hit_box_shapes = fit_hit_box_shapes(&ai_mesh, &skeleton);
        Model {
            transform: Matrix4::identity(),
            inner: InnerModel::Animated(AnimatedModel {
                //mesh: ai_mesh,
                skeleton,
                scene_objects,
                hit_boxes: Rc::new(hit_boxes),
                hit_box_shapes: Rc::new(hit_box_shapes),
                vhots: vec![],
                sub_objects: vec![],
                bind,
            }),
        }
    }

    pub fn from_glb(
        scene_objects: Vec<SceneObject>,
        bounding_box: Aabb3<f32>,
        skeleton: Option<Skeleton>,
    ) -> Model {
        if let Some(skeleton) = skeleton {
            println!(
                "Creating animated GLB model with {} bones",
                skeleton.bone_count()
            );
            // Animated model
            let hit_boxes = HashMap::new();
            Model {
                transform: Matrix4::identity(),
                inner: InnerModel::Animated(AnimatedModel {
                    skeleton: Rc::new(skeleton),
                    scene_objects,
                    hit_boxes: Rc::new(hit_boxes),
                    hit_box_shapes: Rc::new(HashMap::new()),
                    vhots: vec![],
                    sub_objects: vec![],
                    bind: None,
                }),
            }
        } else {
            println!("Creating static GLB model (no skeleton)");
            // Static model
            Model {
                transform: Matrix4::identity(),
                inner: InnerModel::Static(StaticModel {
                    scene_objects,
                    bounding_box,
                    vhots: vec![],
                    sub_objects: vec![],
                }),
            }
        }
    }

    pub fn to_scene_objects(&self) -> &Vec<SceneObject> {
        match &self.inner {
            InnerModel::Animated(animated_model) => animated_model.to_scene_objects(),
            InnerModel::Static(static_model) => static_model.to_scene_objects(),
        }
    }

    /// The per-joint bind-pose undo for a `PMNM` mesh, if this model uses one.
    ///
    /// Anything that poses `clone_scene_objects()` itself - the ragdoll, which
    /// drives joints from physics rather than from an `AnimationPlayer` - must
    /// fold this in, or a bind-space mesh renders exploded.
    pub fn bind_matrices(&self) -> Option<Rc<[Matrix4<f32>; MAX_SKINNED_JOINTS]>> {
        match &self.inner {
            InnerModel::Animated(animated_model) => animated_model.bind.clone(),
            InnerModel::Static(_) => None,
        }
    }

    pub fn clone_scene_objects(&self) -> Vec<SceneObject> {
        match &self.inner {
            InnerModel::Animated(animated_model) => animated_model.to_scene_objects().clone(),
            InnerModel::Static(static_model) => static_model.to_scene_objects().clone(),
        }
    }

    pub fn vhots(&self) -> Vec<Vhot> {
        match &self.inner {
            InnerModel::Animated(animated_model) => animated_model.vhots.clone(),
            InnerModel::Static(static_model) => static_model.vhots.clone(),
        }
    }

    /// The LGMD sub-object pivots (see [`SubObject`]). Empty unless this model
    /// came from an object `.bin`.
    pub fn sub_objects(&self) -> &[SubObject] {
        match &self.inner {
            InnerModel::Animated(animated_model) => &animated_model.sub_objects,
            InnerModel::Static(static_model) => &static_model.sub_objects,
        }
    }

    /// Per-joint fitted collision shapes (capsule-toward-child / box), the shared
    /// source of truth for the ragdoll and damage hitboxes. Empty for static or
    /// non-AI-bin models.
    pub fn hit_box_shapes(&self) -> Rc<HashMap<u32, HitBoxShape>> {
        match &self.inner {
            InnerModel::Animated(animated_model) => animated_model.hit_box_shapes.clone(),
            InnerModel::Static(_) => Rc::new(HashMap::new()),
        }
    }

    pub fn get_hit_boxes(&self) -> Rc<HashMap<u32, Aabb3<f32>>> {
        match &self.inner {
            InnerModel::Animated(animated_model) => animated_model.hit_boxes.clone(),
            InnerModel::Static(static_model) => {
                let mut hit_boxes = HashMap::new();
                hit_boxes.insert(0, static_model.bounding_box);
                Rc::new(hit_boxes)
            }
        }
    }

    pub fn to_animated_scene_objects(&self, player: &AnimationPlayer) -> Vec<SceneObject> {
        match &self.inner {
            InnerModel::Animated(animated_model) => {
                animated_model.to_animated_scene_objects(player)
            }
            InnerModel::Static(static_model) => static_model.to_scene_objects().clone(),
        }
    }

    pub fn bounding_box(&self) -> Option<Aabb3<f32>> {
        match &self.inner {
            InnerModel::Animated(_animated_model) => None,
            InnerModel::Static(static_model) => Some(static_model.bounding_box),
        }
    }

    /// Bias every scene object's depth slightly toward the camera. Set at
    /// model preparation for flat decal-like meshes placed coplanar with
    /// world geometry, so they win the depth test instead of z-fighting it.
    pub fn set_depth_bias(&mut self, enabled: bool) {
        let objs = match &mut self.inner {
            InnerModel::Static(m) => &mut m.scene_objects,
            InnerModel::Animated(m) => &mut m.scene_objects,
        };
        for obj in objs {
            obj.set_depth_bias(enabled);
        }
    }

    pub fn get_transform(&self) -> Matrix4<f32> {
        self.transform
    }

    pub fn animate(&self, animation_clip: &AnimationClip, frame: u32) -> Model {
        match &self.inner {
            InnerModel::Static(_) => self.clone(),
            InnerModel::Animated(animated_model) => Model {
                transform: self.transform,
                inner: InnerModel::Animated(animated_model.clone().animate(animation_clip, frame)),
            },
        }
    }

    pub fn get_joint_transforms(&self, animation_player: &AnimationPlayer) -> [Matrix4<f32>; 40] {
        match &self.inner {
            InnerModel::Static(_) => [Matrix4::identity(); 40],
            InnerModel::Animated(animated_model) => {
                animation_player.get_transforms(&animated_model.skeleton)
                // let mut output = [Matrix4::identity(); 40];

                // let mut idx = 0;
                // for joint in &animated_model.mesh.joint_map {
                //     println!("[debug] mapping {} to {}", idx, joint.joint);
                //     output[idx] = initial[joint.joint as usize];
                //     idx = idx + 1;
                // }

                // output
            }
        }
    }

    pub fn is_animated(&self) -> bool {
        match &self.inner {
            InnerModel::Static(_) => false,
            InnerModel::Animated(_) => true,
        }
    }

    pub fn draw_debug_skeleton(&self, global_transforms: &[Matrix4<f32>]) -> Vec<SceneObject> {
        match &self.inner {
            InnerModel::Animated(animated_model) => {
                animated_model.skeleton.debug_draw(global_transforms)
            }
            InnerModel::Static(_) => Vec::new(),
        }
    }

    pub fn draw_debug_skeleton_with_text(
        &self,
        global_transforms: &[Matrix4<f32>],
        asset_cache: &mut AssetCache,
        view: Matrix4<f32>,
        projection: Matrix4<f32>,
        screen_size: Vector2<f32>,
    ) -> Vec<SceneObject> {
        match &self.inner {
            InnerModel::Animated(animated_model) => animated_model.skeleton.debug_draw_with_text(
                global_transforms,
                asset_cache,
                view,
                projection,
                screen_size,
            ),
            InnerModel::Static(_) => Vec::new(),
        }
    }

    pub fn ragdoll_source(&self) -> Option<(Vec<Bone>, Vec<Matrix4<f32>>)> {
        match &self.inner {
            InnerModel::Animated(animated_model) => {
                let bones = animated_model.skeleton.bones().to_vec();
                let world = animated_model
                    .skeleton
                    .world_transforms()
                    .into_iter()
                    .collect::<Vec<_>>();
                Some((bones, world))
            }
            InnerModel::Static(_) => None,
        }
    }

    pub fn can_create_rag_doll(&self) -> bool {
        matches!(self.inner, InnerModel::Animated(_))
    }

    pub fn skeleton(&self) -> Option<&Skeleton> {
        match &self.inner {
            InnerModel::Animated(animated_model) => Some(&animated_model.skeleton),
            InnerModel::Static(_) => None,
        }
    }

    /// Compose a model-space transform *inside* the entity transform, on top of
    /// whatever local transform each scene object already carries (the obj
    /// loader gives vhot debug cubes theirs). The render path re-sets the
    /// entity (world) transform every frame; the local one rides along with
    /// the model.
    ///
    /// A *mirroring* transform (negative determinant - e.g. the left-hand VR
    /// wield) reverses the triangle winding the GPU sees, so an object that
    /// culls backfaces would render inside-out. Flip its front-face winding to
    /// match, here rather than at each call site: the transform is what makes
    /// the mesh mirrored, so the winding it implies belongs with it. Objects
    /// that do not cull (the GLB-loaded glove, debug geometry) are unaffected.
    pub fn apply_local_transform(&mut self, local_transform: Matrix4<f32>) {
        let mirrored = local_transform.determinant() < 0.0;
        let scene_objects = match &mut self.inner {
            InnerModel::Static(static_model) => &mut static_model.scene_objects,
            InnerModel::Animated(animated_model) => &mut animated_model.scene_objects,
        };
        for obj in scene_objects {
            obj.set_local_transform(local_transform * obj.local_transform);
            if mirrored {
                obj.set_backface_culling(obj.backface_culling().map(flip_winding));
            }
        }
    }

    pub fn transform(model: &Model, transform: Matrix4<f32>) -> Model {
        match &model.inner {
            InnerModel::Static(static_model) => Model {
                transform: model.transform,
                inner: InnerModel::Static(StaticModel::transform(static_model, transform)),
            },
            InnerModel::Animated(animated_model) => Model {
                transform: model.transform,
                inner: InnerModel::Animated(AnimatedModel::transform(animated_model, transform)),
            },
        }
    }

    pub fn pose(&self, animation_clip: &Rc<AnimationClip>) -> Model {
        match &self.inner {
            InnerModel::Static(_) => self.clone(),
            InnerModel::Animated(animated_model) => Model {
                inner: InnerModel::Animated(animated_model.clone().pose(animation_clip)),
                ..self.clone()
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cgmath::{Point3, vec3};
    use std::cell::RefCell;

    fn culled_static_model() -> Model {
        let material = RefCell::new(engine::scene::color_material::create(vec3(1.0, 1.0, 1.0)));
        let geometry: Rc<Box<dyn engine::scene::Geometry>> =
            Rc::new(Box::new(engine::scene::geometry::EmptyMesh));
        let mut object = SceneObject::create(material, geometry);
        object.set_backface_culling(Some(FrontFaceWinding::Clockwise));

        Model {
            transform: Matrix4::identity(),
            inner: InnerModel::Static(StaticModel {
                scene_objects: vec![object],
                bounding_box: Aabb3::new(Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 1.0, 1.0)),
                vhots: Vec::new(),
                sub_objects: Vec::new(),
            }),
        }
    }

    fn winding(model: &Model) -> Option<FrontFaceWinding> {
        match &model.inner {
            InnerModel::Static(m) => m.scene_objects[0].backface_culling(),
            InnerModel::Animated(m) => m.scene_objects[0].backface_culling(),
        }
    }

    /// A mirroring local transform (the left-hand VR melee wield) reverses the
    /// triangle winding the GPU sees, so the front face has to flip with it or
    /// the mesh renders inside-out - backfaces toward the camera, front faces
    /// culled.
    #[test]
    fn a_mirrored_local_transform_flips_the_front_face_winding() {
        let mut model = culled_static_model();
        model.apply_local_transform(Matrix4::from_nonuniform_scale(-1.0, 1.0, 1.0));

        assert_eq!(winding(&model), Some(FrontFaceWinding::CounterClockwise));
    }

    /// Mirroring twice is not mirrored, and an ordinary (rotate/translate/
    /// uniform-scale) correction must leave the winding exactly as authored.
    #[test]
    fn an_ordinary_local_transform_leaves_the_winding_alone() {
        let mut model = culled_static_model();
        model.apply_local_transform(
            Matrix4::from_translation(vec3(1.0, 2.0, 3.0)) * Matrix4::from_scale(0.5),
        );
        assert_eq!(winding(&model), Some(FrontFaceWinding::Clockwise));

        model.apply_local_transform(Matrix4::from_nonuniform_scale(-1.0, 1.0, 1.0));
        model.apply_local_transform(Matrix4::from_nonuniform_scale(-1.0, 1.0, 1.0));
        assert_eq!(winding(&model), Some(FrontFaceWinding::Clockwise));
    }
}
