// ss2_skeleton.rs
// Helper class to work with skeletons in AI meshes

use std::collections::HashMap;

use cgmath::{vec3, Deg, Matrix4, SquareMatrix};

use crate::{
    motion::{AnimationClip, JointId},
    ss2_cal_loader::SystemShock2Cal,
    SCALE_FACTOR,
};

#[derive(Debug, Clone)]
pub struct Skeleton {
    bones: Vec<Bone>,
    animation_transforms: HashMap<JointId, Matrix4<f32>>,
    global_transforms: HashMap<JointId, Matrix4<f32>>,
}

#[derive(Debug, Clone)]
struct Bone {
    joint_id: JointId,
    parent_id: Option<JointId>,
    local_transform: Matrix4<f32>,
}

impl Skeleton {
    pub fn get_transforms(&self) -> [Matrix4<f32>; 40] {
        let mut transforms = [Matrix4::identity(); 40];
        for (joint_id, transform) in self.global_transforms.iter() {
            if joint_id >= &40 {
                break;
            }
            transforms[*joint_id as usize] = *transform;
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
}

pub fn create(cal: SystemShock2Cal) -> Skeleton {
    // Create bones from torsos
    let mut bones = Vec::new();
    for i in 0..cal.num_torsos {
        let torso = &cal.torsos[i as usize];

        let parent_id = if torso.parent >= 0 {
            Some(torso.parent as JointId)
        } else {
            None
        };

        let torso_bone = Bone {
            joint_id: torso.joint,
            parent_id,
            local_transform: Matrix4::from_angle_y(Deg(90.0)),
        };
        // Push root bone
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

    // Build global transform map
    let animation_transforms = HashMap::new();
    let mut global_transforms = HashMap::new();
    for bone in &bones {
        let _ignored = calc_and_cache_global_transform(
            bone.joint_id,
            &animation_transforms,
            &mut global_transforms,
            &bones,
        );
    }

    Skeleton {
        bones,
        animation_transforms,
        global_transforms,
    }
}

fn calc_and_cache_global_transform(
    bone: JointId,
    animation_transforms: &HashMap<JointId, Matrix4<f32>>,
    global_transforms: &mut HashMap<JointId, Matrix4<f32>>,
    bones: &Vec<Bone>,
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
                None => Matrix4::identity(),
                Some(parent_id) => calc_and_cache_global_transform(
                    parent_id,
                    animation_transforms,
                    global_transforms,
                    bones,
                ),
            };

            let global_transform = parent_transform * local_transform * animation_transform;
            global_transforms.insert(local_bone.joint_id, global_transform);
            global_transform
        }
    }
}

pub fn animate(base_skeleton: &Skeleton, animation_clip: &AnimationClip, frame: u32) -> Skeleton {
    let bones = base_skeleton.bones.clone();

    let normalized_frame = frame % animation_clip.num_frames;

    let animations = &animation_clip.joint_to_frame;
    let mut animation_transforms = HashMap::new();
    for key in animations {
        let (joint, frames) = key;
        animation_transforms.insert(*joint, frames[normalized_frame as usize]);
    }

    let mut global_transforms = HashMap::new();

    for bone in &bones {
        let _ignored = calc_and_cache_global_transform(
            bone.joint_id,
            &animation_transforms,
            &mut global_transforms,
            &bones,
        );
    }

    Skeleton {
        bones,
        animation_transforms,
        global_transforms,
    }
}
