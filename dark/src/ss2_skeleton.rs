// ss2_skeleton.rs
// Helper class to work with skeletons in AI meshes

use rpds as immutable;
use std::collections::HashMap;
use tracing::warn;

use cgmath::{Deg, Matrix4, Quaternion, SquareMatrix, Vector2, Vector3};
use engine::{
    assets::asset_cache::AssetCache,
    scene::{SceneObject, VertexPosition, color_material, cube, lines_mesh},
    util,
};

use crate::{
    SCALE_FACTOR,
    importers::FONT_IMPORTER,
    motion::{AnimationClip, JointId},
    ss2_cal_loader::SystemShock2Cal,
};

#[derive(Debug, Clone)]
pub struct Skeleton {
    bones: Vec<Bone>,
    #[allow(dead_code)]
    animation_transforms: HashMap<JointId, Matrix4<f32>>,
    global_transforms: HashMap<JointId, Matrix4<f32>>,
}

#[derive(Debug, Clone)]
pub struct Bone {
    pub joint_id: JointId,
    pub parent_id: Option<JointId>,
    pub local_transform: Matrix4<f32>,
}

#[derive(Debug, Clone)]
pub struct JointRestTransform {
    pub translation: Vector3<f32>,
    pub rotation: Quaternion<f32>,
    pub scale: Vector3<f32>,
    pub local_matrix: Matrix4<f32>,
    pub local_inverse: Matrix4<f32>,
    pub inverse_bind: Matrix4<f32>,
}

impl Skeleton {
    pub fn bone_count(&self) -> usize {
        self.bones.len()
    }

    pub fn bones(&self) -> &[Bone] {
        &self.bones
    }

    pub fn get_transforms(&self) -> [Matrix4<f32>; 40] {
        let mut transforms = [Matrix4::identity(); 40];
        for (joint_id, global_transform) in self.global_transforms.iter() {
            if joint_id >= &40 {
                break;
            }
            transforms[*joint_id as usize] = *global_transform;
        }
        transforms
    }

    pub fn world_transforms(&self) -> [Matrix4<f32>; 40] {
        let mut transforms = [Matrix4::identity(); 40];
        for bone in &self.bones {
            if bone.joint_id < 40 {
                if let Some(global) = self.global_transforms.get(&bone.joint_id) {
                    transforms[bone.joint_id as usize] = *global;
                }
            }
        }
        transforms
    }

    pub fn global_transform(&self, joint_id: &JointId) -> Matrix4<f32> {
        let _joint_offset = *joint_id as f32;

        let default_matrix = Matrix4::identity();
        self.global_transforms
            .get(joint_id)
            .copied()
            .unwrap_or(default_matrix)
    }

    pub fn empty() -> Skeleton {
        Skeleton {
            bones: Vec::new(),
            animation_transforms: HashMap::new(),
            global_transforms: HashMap::new(),
        }
    }

    pub fn create_from_bones(bones: Vec<Bone>) -> Skeleton {
        // Build global transform map
        let animation_transforms = HashMap::new();
        let mut global_transforms = HashMap::new();
        for bone in &bones {
            let _ignored = calc_and_cache_global_transform(
                bone.joint_id,
                &animation_transforms,
                &mut global_transforms,
                &bones,
                Matrix4::identity(),
            );
        }

        Skeleton {
            bones,
            animation_transforms,
            global_transforms,
        }
    }

    pub fn set_joint_transforms(
        base_skeleton: &Skeleton,
        joint_transforms: &HashMap<JointId, Matrix4<f32>>,
    ) -> Skeleton {
        let bones = base_skeleton.bones.clone();
        let animation_transforms = joint_transforms.clone();
        let mut global_transforms = HashMap::new();

        for bone in &bones {
            let _ignored = calc_and_cache_global_transform(
                bone.joint_id,
                &animation_transforms,
                &mut global_transforms,
                &bones,
                Matrix4::identity(),
            );
        }

        Skeleton {
            bones,
            animation_transforms,
            global_transforms,
        }
    }

    pub fn debug_draw(&self, global_transforms: &[Matrix4<f32>]) -> Vec<SceneObject> {
        if global_transforms.is_empty() || self.bones.is_empty() {
            return Vec::new();
        }

        let mut debug_objects = Vec::new();
        let mut line_vertices = Vec::new();

        let joint_template = SceneObject::new(
            color_material::create(Vector3::new(0.95, 0.6, 0.2)),
            Box::new(cube::create()),
        );

        for bone in &self.bones {
            let joint_idx = bone.joint_id as usize;
            if joint_idx >= global_transforms.len() {
                continue;
            }

            let joint_position = translation_from_matrix(&global_transforms[joint_idx]);

            let mut joint_obj = joint_template.clone();
            let joint_transform = Matrix4::from_translation(joint_position)
                * Matrix4::from_nonuniform_scale(0.05, 0.05, 0.05);
            joint_obj.set_transform(joint_transform);
            debug_objects.push(joint_obj);

            if let Some(parent_id) = bone.parent_id {
                let parent_idx = parent_id as usize;
                if parent_idx < global_transforms.len() {
                    let parent_position = translation_from_matrix(&global_transforms[parent_idx]);
                    line_vertices.push(VertexPosition {
                        position: parent_position,
                    });
                    line_vertices.push(VertexPosition {
                        position: joint_position,
                    });
                }
            }
        }

        if !line_vertices.is_empty() {
            let lines_mat = color_material::create(Vector3::new(0.2, 0.8, 1.0));
            let line_obj = SceneObject::new(lines_mat, Box::new(lines_mesh::create(line_vertices)));
            debug_objects.push(line_obj);
        }

        debug_objects
    }

    pub fn debug_draw_with_text(
        &self,
        global_transforms: &[Matrix4<f32>],
        asset_cache: &mut AssetCache,
        view: Matrix4<f32>,
        projection: Matrix4<f32>,
        screen_size: Vector2<f32>,
    ) -> Vec<SceneObject> {
        if global_transforms.is_empty() || self.bones.is_empty() {
            return Vec::new();
        }

        let mut debug_objects = Vec::new();
        let mut line_vertices = Vec::new();

        let joint_template = SceneObject::new(
            color_material::create(Vector3::new(0.95, 0.6, 0.2)),
            Box::new(cube::create()),
        );

        let font = asset_cache.get(&FONT_IMPORTER, "mainfont.fon");

        for bone in &self.bones {
            let joint_idx = bone.joint_id as usize;
            if joint_idx >= global_transforms.len() {
                continue;
            }

            let joint_position = translation_from_matrix(&global_transforms[joint_idx]);

            let mut joint_obj = joint_template.clone();
            let joint_transform = Matrix4::from_translation(joint_position)
                * Matrix4::from_nonuniform_scale(0.05, 0.05, 0.05);
            joint_obj.set_transform(joint_transform);
            debug_objects.push(joint_obj);

            // Project joint position to screen space for text overlay
            let screen_pos = util::project(
                view,
                projection,
                joint_position,
                screen_size.x,
                screen_size.y,
            );

            // Create text object with joint ID
            let joint_id_text = SceneObject::screen_space_text(
                &bone.joint_id.to_string(),
                font.clone(),
                12.0,                // font size
                1.0,                 // transparency
                screen_pos.x + 10.0, // offset to the right of joint
                screen_pos.y - 5.0,  // offset slightly above joint
            );
            debug_objects.push(joint_id_text);

            if let Some(parent_id) = bone.parent_id {
                let parent_idx = parent_id as usize;
                if parent_idx < global_transforms.len() {
                    let parent_position = translation_from_matrix(&global_transforms[parent_idx]);
                    line_vertices.push(VertexPosition {
                        position: parent_position,
                    });
                    line_vertices.push(VertexPosition {
                        position: joint_position,
                    });
                }
            }
        }

        if !line_vertices.is_empty() {
            let lines_mat = color_material::create(Vector3::new(0.2, 0.8, 1.0));
            let line_obj = SceneObject::new(lines_mat, Box::new(lines_mesh::create(line_vertices)));
            debug_objects.push(line_obj);
        }

        debug_objects
    }
}

pub fn create(cal: SystemShock2Cal) -> Skeleton {
    // Validate CAL file structure
    if cal.torsos.is_empty() {
        eprintln!("Warning: CAL file has no torsos");
        return Skeleton::empty();
    }

    // First torso should be root (parent == -1)
    if cal.torsos[0].parent != -1 {
        warn!(
            "First torso doesn't have parent == -1, got parent = {}",
            cal.torsos[0].parent
        );
    }

    // Create bones from torsos
    let mut bones = Vec::new();
    for i in 0..cal.num_torsos {
        let torso = &cal.torsos[i as usize];

        // Fix: torso.parent is a torso array index, not a joint ID
        let parent_id = if torso.parent == -1 {
            // Root torso has no parent
            None
        } else if torso.parent >= 0 && (torso.parent as usize) < cal.torsos.len() {
            // Parent is index into torsos array - get that torso's joint ID
            Some(cal.torsos[torso.parent as usize].joint)
        } else {
            warn!(
                "Invalid torso parent index {} for torso {}, treating as root",
                torso.parent, i
            );
            None
        };

        let torso_bone = Bone {
            joint_id: torso.joint,
            parent_id,
            local_transform: Matrix4::from_angle_y(Deg(90.0)),
        };
        // Push torso root bone
        bones.push(torso_bone);

        // Iterate through and push torso joints
        for joint_idx in 0..torso.fixed_count {
            let joint_id = torso.fixed_joints[joint_idx as usize] as JointId;
            let local_position = torso.fixed_joint_offset[joint_idx as usize] / SCALE_FACTOR;
            let parent_id = Some(torso.joint as JointId);
            bones.push(Bone {
                joint_id,
                local_transform: Matrix4::from_translation(local_position),
                parent_id,
            })
        }
    }

    // Create bones from joints
    for i in 0..cal.num_limbs {
        let limb = &cal.limbs[i as usize];

        let mut parent_id = limb.attachment_joint as JointId;
        for s in 0..limb.num_segments {
            let seg = limb.segments[s as usize] as JointId;
            let seg_length = limb.segment_lengths[s as usize] / SCALE_FACTOR;
            let seg_dir = &limb.segment_directions[s as usize];

            let joint_id = seg;
            let local_position = seg_dir * seg_length;
            bones.push(Bone {
                joint_id,
                local_transform: Matrix4::from_translation(local_position),
                parent_id: Some(parent_id),
            });
            parent_id = seg;
        }
    }

    Skeleton::create_from_bones(bones)
}

fn calc_and_cache_global_transform(
    bone: JointId,
    animation_transforms: &HashMap<JointId, Matrix4<f32>>,
    global_transforms: &mut HashMap<JointId, Matrix4<f32>>,
    bones: &Vec<Bone>,
    root_transform: Matrix4<f32>,
) -> Matrix4<f32> {
    match global_transforms.get(&bone) {
        Some(xform) => *xform,
        None => {
            let local_bone = bones.iter().find(|b| b.joint_id == bone).unwrap();
            let local_transform = local_bone.local_transform;

            let animation_transform = match animation_transforms.get(&bone) {
                None => Matrix4::identity(),
                Some(m) => *m,
            };

            let parent_transform = match local_bone.parent_id {
                None => root_transform,
                Some(parent_id) => calc_and_cache_global_transform(
                    parent_id,
                    animation_transforms,
                    global_transforms,
                    bones,
                    root_transform,
                ),
            };

            let global_transform = parent_transform * local_transform * animation_transform;
            global_transforms.insert(local_bone.joint_id, global_transform);
            global_transform
        }
    }
}

pub struct AnimationInfo<'a> {
    pub animation_clip: &'a AnimationClip,
    pub frame: u32,
}

pub fn animate(
    base_skeleton: &Skeleton,
    animation_info: Option<AnimationInfo>,
    additional_joint_transforms: &immutable::HashTrieMap<u32, Matrix4<f32>>,
) -> Skeleton {
    let bones = base_skeleton.bones.clone();

    let mut animation_transforms = HashMap::new();

    let root_transform = if let Some(AnimationInfo {
        animation_clip,
        frame,
    }) = animation_info
    {
        let normalized_frame = frame % animation_clip.num_frames;
        let animations = &animation_clip.joint_to_frame;
        for key in animations {
            let (joint, frames) = key;
            animation_transforms.insert(*joint, frames[normalized_frame as usize]);
        }

        // Get the root transform for this frame
        if animation_clip.root_transforms.is_empty() {
            Matrix4::identity()
        } else {
            let root_transform = animation_clip.root_transforms[normalized_frame as usize];
            root_transform
        }
    } else {
        Matrix4::identity()
    };

    // Have joint transforms completely override animation transforms
    // TODO: Are there cases where joint transforms need to be used in the context of an animation transform? Maybe head rotation?
    for (joint, transform) in additional_joint_transforms {
        animation_transforms.insert(*joint, *transform);
    }

    let mut global_transforms = HashMap::new();

    for bone in &bones {
        let _ignored = calc_and_cache_global_transform(
            bone.joint_id,
            &animation_transforms,
            &mut global_transforms,
            &bones,
            root_transform,
        );
    }

    Skeleton {
        bones,
        animation_transforms,
        global_transforms,
    }
}

fn translation_from_matrix(matrix: &Matrix4<f32>) -> Vector3<f32> {
    matrix.w.truncate()
}
