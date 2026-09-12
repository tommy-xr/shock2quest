use std::{collections::HashMap, rc::Rc};

use crate::{
    hit_box::{HitBoxShape, fit_hit_box_shapes},
    motion::{AnimationClip, AnimationPlayer},
    ss2_bin_ai_loader::{self, SystemShock2AIMesh},
    ss2_bin_obj_loader::{self, SystemShock2ObjectMesh, Vhot},
    ss2_skeleton::{self, AnimationInfo, Bone, Skeleton},
};
use cgmath::{Matrix4, SquareMatrix, Transform, Vector2};
use collision::{Aabb, Aabb3};
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

/// Enclose the joint's geometry with a cylinder and rounded ends. The long
/// axis comes from the mesh, so this also handles terminal parts without a child.
fn object_part_capsule(bounds: Aabb3<f32>, padding: f32) -> HitBoxShape {
    use cgmath::EuclideanSpace;
    let half = bounds.dim() * 0.5;
    let axis = if half.x >= half.y && half.x >= half.z {
        0
    } else if half.y >= half.z {
        1
    } else {
        2
    };
    let mut a = bounds.center().to_vec();
    let mut b = a;
    a[axis] -= half[axis];
    b[axis] += half[axis];
    let radial_sq: f32 = (0..3)
        .filter(|&i| i != axis)
        .map(|i| half[i] * half[i])
        .sum();
    HitBoxShape::Capsule {
        a,
        b,
        radius: radial_sq.sqrt() + padding.max(0.0),
    }
}

/// One LGMD sub-object: a named part of a static `.bin` (the pieces an in-game
/// tweq rotates or translates - a gun slide, a door leaf) and its pivot in
/// model space. Empty for LGMM/AI and GLB models, which articulate via joints.
#[derive(Clone, Debug)]
pub struct SubObject {
    pub name: String,
    pub transform: Matrix4<f32>,
    pub articulation: u8,
    pub parameter: i32,
    /// Geometry bounds in this joint's local frame; absent for empty pivots.
    pub local_bounds: Option<Aabb3<f32>>,
}

impl SubObject {
    /// LGMD articulates around/along local Dark X after the authored pivot
    /// transform. Dark X maps to -X in the runtime. Parameters are degrees
    /// for hinges and Dark distance units for sliders.
    pub fn parameter_transform(&self, values: &[f32]) -> Option<Matrix4<f32>> {
        let value = *values.get(usize::try_from(self.parameter).ok()?)?;
        if !value.is_finite() {
            return None;
        }
        match self.articulation {
            1 => Some(Matrix4::from_angle_x(cgmath::Deg(-value))),
            2 => Some(Matrix4::from_translation(cgmath::vec3(
                -value / crate::SCALE_FACTOR,
                0.0,
                0.0,
            ))),
            _ => None,
        }
    }
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
    /// Model-space bounds of the pose baked into `scene_objects`, set when a
    /// model is statically posed (`animate`/`pose`). Posing bakes only the
    /// skinning palette - the skeleton itself stays at rest - so without this
    /// there is no record of the pose the model is actually drawn in, and a
    /// corpse lying flat would be bounded by the standing rest skeleton.
    posed_bounds: Option<Aabb3<f32>>,
    object_articulation: Option<std::sync::Arc<crate::object_articulation::ObjectArticulation>>,
    /// Authored LGMD rest bounds, kept separate from posed skeletal bounds.
    object_bounds: Option<Aabb3<f32>>,
}

/// Build the render palette, undoing the bind pose first when the geometry needs
/// it. `expand_skinning_palette`'s stretchy parent frames are a vanilla-LGMM
/// concept, so a bind-space mesh takes the plain per-joint product.
pub(crate) fn build_palette(
    pose: &[Matrix4<f32>; MAX_SKINNED_JOINTS],
    skeleton: &Skeleton,
    bind: Option<&[Matrix4<f32>; MAX_SKINNED_JOINTS]>,
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

/// Union of per-joint vertex AABBs, each carried into model space by that
/// joint's transform. `None` when the mesh has no per-joint boxes at all (a
/// jointed object mesh, a GLB).
///
/// The joint boxes are the coarse per-joint vertex AABBs (they cluster at the
/// joint origins), not a fit of the skinned vertices - enough for the selection
/// volume this feeds, and the same source the damage hitboxes use.
fn joint_box_bounds(
    joints: &[Matrix4<f32>; 40],
    hit_boxes: &HashMap<u32, Aabb3<f32>>,
) -> Option<Aabb3<f32>> {
    let mut bounds: Option<Aabb3<f32>> = None;
    for (joint_id, aabb) in hit_boxes.iter() {
        let Some(transform) = joints.get(*joint_id as usize) else {
            continue;
        };
        for corner in aabb.to_corners() {
            let point = transform.transform_point(corner);
            bounds = Some(match bounds {
                None => Aabb3::new(point, point),
                Some(bounds) => bounds.grow(point),
            });
        }
    }
    bounds
}

impl AnimatedModel {
    /// Model-space bounds of the pose this model is drawn in: the baked pose
    /// for a statically posed model (an authored corpse), the rest pose
    /// otherwise.
    fn bounding_box(&self) -> Option<Aabb3<f32>> {
        self.posed_bounds
            .or_else(|| joint_box_bounds(&self.skeleton.get_transforms(), &self.hit_boxes))
    }

    fn to_scene_objects(&self) -> &Vec<SceneObject> {
        &self.scene_objects
    }

    fn to_animated_scene_objects(&self, player: &AnimationPlayer) -> Vec<SceneObject> {
        self.to_posed_scene_objects(&player.get_transforms(&self.skeleton))
    }

    fn to_posed_scene_objects(&self, pose: &[Matrix4<f32>; 40]) -> Vec<SceneObject> {
        let palette = build_palette(pose, &self.skeleton, self.bind.as_deref());

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
            self.bind.as_deref(),
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
            object_articulation: self.object_articulation.clone(),
            posed_bounds: joint_box_bounds(&animated_skeleton.get_transforms(), &self.hit_boxes),
            object_bounds: self.object_bounds,
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
            posed_bounds: model.posed_bounds,
            object_articulation: model.object_articulation.clone(),
            object_bounds: model.object_bounds,
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
        // Same pairs as `ss2_bin_obj_loader::sub_object_transforms` - keep them
        // in step.
        let local_bounds = ss2_bin_obj_loader::sub_object_bounds(
            &static_mesh,
            &[Matrix4::identity(); MAX_SKINNED_JOINTS],
        );
        let sub_objects = static_mesh
            .sub_objects
            .iter()
            .enumerate()
            .map(|(index, sub_object)| SubObject {
                name: sub_object.name.clone(),
                transform: skeleton.global_transform(&(index as u32)),
                articulation: sub_object.articulation,
                parameter: sub_object.parameter,
                local_bounds: local_bounds[index].1,
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
                    posed_bounds: None,
                    object_articulation: Some(std::sync::Arc::new(
                        ss2_bin_obj_loader::object_articulation(&static_mesh),
                    )),
                    object_bounds: Some(bounding_box),
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
                posed_bounds: None,
                object_articulation: None,
                object_bounds: None,
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
                    posed_bounds: None,
                    object_articulation: None,
                    object_bounds: None,
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

    /// Bake one player pose into the model's scene objects for immutable tool previews.
    /// The skeleton is retained, so gameplay can still drive it with the same player.
    pub fn with_animation_pose(&self, player: &AnimationPlayer) -> Model {
        let mut model = self.clone();
        if let InnerModel::Animated(ref mut animated) = model.inner {
            animated.scene_objects = animated.to_animated_scene_objects(player);
        }
        model
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

    /// Authored LGMD parameter mapping and attachment ownership.
    pub fn object_articulation(
        &self,
    ) -> Option<&std::sync::Arc<crate::object_articulation::ObjectArticulation>> {
        match &self.inner {
            InnerModel::Animated(model) => model.object_articulation.as_ref(),
            InnerModel::Static(_) => None,
        }
    }

    /// The LGMD sub-object pivots (see [`SubObject`]). Empty for other formats.
    pub fn sub_objects(&self) -> &[SubObject] {
        match &self.inner {
            InnerModel::Animated(animated_model) => &animated_model.sub_objects,
            InnerModel::Static(static_model) => &static_model.sub_objects,
        }
    }

    /// Per-joint fitted collision shapes (capsule-toward-child / box), the shared
    /// source of truth for the ragdoll and damage hitboxes. Articulated object
    /// models opt in through `enable_object_joint_hit_boxes`; static models are empty.
    pub fn hit_box_shapes(&self) -> Rc<HashMap<u32, HitBoxShape>> {
        match &self.inner {
            InnerModel::Animated(animated_model) => animated_model.hit_box_shapes.clone(),
            InnerModel::Static(_) => Rc::new(HashMap::new()),
        }
    }

    /// Opt an articulated object into per-part damage geometry. Physics support
    /// remains separate. Empty pivots receive no collider.
    pub fn enable_object_joint_hit_boxes(&mut self, padding: f32) {
        let InnerModel::Animated(model) = &mut self.inner else {
            return;
        };
        if model.object_bounds.is_none() {
            return;
        }
        let bounds: HashMap<_, _> = model
            .sub_objects
            .iter()
            .enumerate()
            .filter_map(|(i, part)| part.local_bounds.map(|bounds| (i as u32, bounds)))
            .collect();
        model.hit_box_shapes = Rc::new(
            bounds
                .iter()
                .map(|(&i, bounds)| (i, object_part_capsule(*bounds, padding)))
                .collect(),
        );
        model.hit_boxes = Rc::new(bounds);
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

    /// Render a saved model-space pose using the same palette construction as
    /// live animation (including inverse bind transforms for remaster models).
    pub fn to_posed_scene_objects(&self, pose: &[Matrix4<f32>; 40]) -> Vec<SceneObject> {
        match &self.inner {
            InnerModel::Animated(model) => model.to_posed_scene_objects(pose),
            InnerModel::Static(model) => model.to_scene_objects().clone(),
        }
    }

    /// Rest bounds for physical support of articulated object actors. This
    /// does not change the legacy interaction bounds of other jointed props.
    pub fn object_model_bounds(&self) -> Option<Aabb3<f32>> {
        match &self.inner {
            InnerModel::Static(model) => Some(model.bounding_box),
            InnerModel::Animated(model) => model.object_bounds,
        }
    }

    pub fn bounding_box(&self) -> Option<Aabb3<f32>> {
        match &self.inner {
            InnerModel::Animated(animated_model) => animated_model.bounding_box(),
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

    /// A skinned corpse or posed prop is one joint chain: its bounds are the
    /// per-joint boxes placed by the rest pose, not the joint-local boxes
    /// themselves.
    fn animated_model(hit_boxes: HashMap<u32, Aabb3<f32>>, bones: Vec<Bone>) -> Model {
        posed_model(hit_boxes, bones, None)
    }

    fn posed_model(
        hit_boxes: HashMap<u32, Aabb3<f32>>,
        bones: Vec<Bone>,
        posed_joints: Option<[Matrix4<f32>; 40]>,
    ) -> Model {
        let posed_bounds = posed_joints.and_then(|joints| joint_box_bounds(&joints, &hit_boxes));
        Model {
            transform: Matrix4::identity(),
            inner: InnerModel::Animated(AnimatedModel {
                skeleton: Rc::new(Skeleton::create_from_bones(bones)),
                scene_objects: Vec::new(),
                hit_boxes: Rc::new(hit_boxes),
                hit_box_shapes: Rc::new(HashMap::new()),
                vhots: Vec::new(),
                sub_objects: Vec::new(),
                bind: None,
                posed_bounds,
                object_articulation: None,
                object_bounds: None,
            }),
        }
    }

    fn unit_box() -> Aabb3<f32> {
        Aabb3::new(Point3::new(-0.5, -0.5, -0.5), Point3::new(0.5, 0.5, 0.5))
    }

    /// Negative-first: an animated model used to report no bounds at all, so
    /// every consumer (the frob/selection collider a corpse is picked by) fell
    /// back to a default-sized box at the object's origin.
    #[test]
    fn an_animated_model_has_the_bounds_of_its_rest_posed_joint_boxes() {
        let bones = vec![
            Bone {
                joint_id: 0,
                parent_id: None,
                local_transform: Matrix4::identity(),
            },
            Bone {
                joint_id: 1,
                parent_id: Some(0),
                local_transform: Matrix4::from_translation(vec3(4.0, 0.0, 0.0)),
            },
        ];
        let hit_boxes = HashMap::from([(0, unit_box()), (1, unit_box())]);

        let bounds = animated_model(hit_boxes, bones)
            .bounding_box()
            .expect("an animated model with joint boxes has bounds");

        // The second joint sits 4 units along +x, so the union spans it -
        // taking the joint-local boxes as-is would give a 1-unit cube.
        assert_eq!(bounds.min, Point3::new(-0.5, -0.5, -0.5));
        assert_eq!(bounds.max, Point3::new(4.5, 0.5, 0.5));
    }

    /// A statically posed model (an authored corpse) is drawn in its baked
    /// pose, not the skeleton's rest pose: its bounds must follow the pose, or
    /// a body lying on the floor is bounded by the standing rest skeleton.
    #[test]
    fn a_posed_model_is_bounded_by_the_pose_it_is_drawn_in() {
        let bones = vec![Bone {
            joint_id: 0,
            parent_id: None,
            local_transform: Matrix4::identity(),
        }];
        // The pose lays the single joint down 5 units along +x.
        let mut posed = [Matrix4::identity(); 40];
        posed[0] = Matrix4::from_translation(vec3(5.0, 0.0, 0.0));

        let bounds = posed_model(HashMap::from([(0, unit_box())]), bones, Some(posed))
            .bounding_box()
            .expect("a posed model with joint boxes has bounds");

        assert_eq!(bounds.min, Point3::new(4.5, -0.5, -0.5));
        assert_eq!(bounds.max, Point3::new(5.5, 0.5, 0.5));
    }

    /// A jointed object mesh (`from_obj_bin`'s animated branch) carries no
    /// per-joint boxes; reporting an empty box would be worse than reporting
    /// nothing, which callers already handle.
    #[test]
    fn an_animated_model_without_joint_boxes_has_no_bounds() {
        assert!(
            animated_model(HashMap::new(), Vec::new())
                .bounding_box()
                .is_none()
        );
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

#[cfg(test)]
mod object_parameter_tests {
    use super::*;
    use cgmath::{SquareMatrix, Transform};
    #[test]
    fn object_capsule_covers_segment_ends_and_adds_radial_padding() {
        use cgmath::point3;
        let HitBoxShape::Capsule { a, b, radius } = object_part_capsule(
            Aabb3::new(point3(-0.4, -0.03, -0.04), point3(0.2, 0.03, 0.04)),
            0.025,
        ) else {
            panic!("joint parts must use capsules")
        };
        assert!((a.x + 0.4).abs() < 1e-6);
        assert!((b.x - 0.2).abs() < 1e-6);
        assert!((radius - 0.075).abs() < 1e-6);
        assert_eq!(a.y, 0.0);
        assert_eq!(b.z, 0.0);
    }

    #[test]
    fn object_parameters_are_not_sub_object_indices_and_rotate_about_dark_x() {
        let part = SubObject {
            name: "hinge".into(),
            transform: Matrix4::identity(),
            articulation: 1,
            parameter: 2,
            local_bounds: None,
        };
        let transform = part.parameter_transform(&[0.0, 0.0, 90.0]).unwrap();
        let point = transform.transform_point(cgmath::point3(0.0, 1.0, 0.0));
        assert!(point.y.abs() < 0.0001);
        assert!((point.z + 1.0).abs() < 0.0001);
        assert!(part.parameter_transform(&[0.0]).is_none());
    }
}
