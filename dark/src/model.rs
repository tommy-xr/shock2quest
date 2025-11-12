use std::{collections::HashMap, rc::Rc};

use crate::{
    motion::{AnimationClip, AnimationPlayer},
    ss2_bin_ai_loader::{self, SystemShock2AIMesh},
    ss2_bin_obj_loader::{self, SystemShock2ObjectMesh, Vhot},
    ss2_skeleton::{self, AnimationInfo, Bone, Skeleton},
};
use cgmath::{Matrix4, SquareMatrix, Vector2};
use collision::Aabb3;
use engine::{assets::asset_cache::AssetCache, scene::SceneObject};

#[derive(Clone)]
pub struct StaticModel {
    scene_objects: Vec<SceneObject>,
    bounding_box: Aabb3<f32>,
    vhots: Vec<Vhot>,
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
        }
    }
}

#[derive(Clone)]
pub struct AnimatedModel {
    skeleton: Rc<Skeleton>,
    scene_objects: Vec<SceneObject>,
    hit_boxes: Rc<HashMap<u32, Aabb3<f32>>>,
    vhots: Vec<Vhot>,
}

impl AnimatedModel {
    fn to_scene_objects(&self) -> &Vec<SceneObject> {
        &self.scene_objects
    }

    fn to_animated_scene_objects(&self, player: &AnimationPlayer) -> Vec<SceneObject> {
        let skinning_data = player.get_transforms(&self.skeleton);

        self.scene_objects
            .iter()
            .map(|m| {
                let mut new_obj = m.clone();
                new_obj.set_skinning_data(skinning_data);
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
            }),
            &rpds::HashTrieMap::new(),
        );
        let new_data = animated_skeleton.get_transforms();

        let new_scene_objects = self
            .scene_objects
            .iter()
            .map(|m| {
                let mut new_obj = m.clone();
                new_obj.set_skinning_data(new_data);
                new_obj
            })
            .collect::<Vec<SceneObject>>();

        AnimatedModel {
            //mesh: self.mesh.clone(),
            skeleton: self.skeleton.clone(),
            scene_objects: new_scene_objects,
            hit_boxes: self.hit_boxes.clone(),
            vhots: self.vhots.clone(),
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
            vhots: model.vhots.clone(),
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

        if skeleton.bone_count() > 1 {
            let hit_boxes = HashMap::new();
            Model {
                transform: Matrix4::identity(),
                inner: InnerModel::Animated(AnimatedModel {
                    skeleton: Rc::new(skeleton),
                    scene_objects,
                    hit_boxes: Rc::new(hit_boxes),
                    vhots: static_mesh.vhots.clone(),
                }),
            }
        } else {
            Model {
                transform: Matrix4::identity(),
                inner: InnerModel::Static(StaticModel {
                    scene_objects,
                    bounding_box,
                    vhots: static_mesh.vhots.clone(),
                }),
            }
        }
    }

    pub fn from_ai_bin(
        ai_mesh: SystemShock2AIMesh,
        skeleton: Rc<Skeleton>,
        asset_cache: &mut AssetCache,
    ) -> Model {
        let (scene_objects, hit_boxes) =
            ss2_bin_ai_loader::to_scene_objects(&ai_mesh, &skeleton, asset_cache);
        Model {
            transform: Matrix4::identity(),
            inner: InnerModel::Animated(AnimatedModel {
                //mesh: ai_mesh,
                skeleton,
                scene_objects,
                hit_boxes: Rc::new(hit_boxes),
                vhots: vec![],
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
                    vhots: vec![],
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
