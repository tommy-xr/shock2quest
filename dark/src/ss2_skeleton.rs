// ss2_skeleton.rs
// Helper class to work with skeletons in AI meshes

use rpds as immutable;
use std::collections::HashMap;
use tracing::warn;

use cgmath::{Deg, InnerSpace, Matrix4, Quaternion, SquareMatrix, Vector2, Vector3};
use engine::{
    assets::asset_cache::AssetCache,
    scene::{
        MAX_SKINNED_JOINTS, SKINNING_PALETTE_SIZE, SceneObject, VertexPosition, color_material,
        cube, lines_mesh,
    },
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

    /// Expand a posed 40-joint palette to the 80-slot render palette: slot
    /// `40 + j` holds joint j's "parent frame" - the parent joint's
    /// orientation anchored at joint j's own position. Stretchy vertices
    /// blend between joint j (weight `w`) and this frame (weight `1 - w`):
    /// at `w = 1` they follow the joint exactly, at `w = 0` the parent's
    /// orientation about the same pivot, so the blend rotates about the
    /// joint rather than tearing between two pivots. Joints without a
    /// parent, and empty slots, fall back to the joint's own transform,
    /// which makes any second-bone reference a no-op.
    pub fn expand_skinning_palette(
        pose: &[Matrix4<f32>; MAX_SKINNED_JOINTS],
        skeleton: &Skeleton,
    ) -> [Matrix4<f32>; SKINNING_PALETTE_SIZE] {
        let mut palette = [Matrix4::identity(); SKINNING_PALETTE_SIZE];
        palette[..MAX_SKINNED_JOINTS].copy_from_slice(pose);
        palette[MAX_SKINNED_JOINTS..].copy_from_slice(pose);
        for bone in skeleton.bones() {
            let j = bone.joint_id as usize;
            if j >= MAX_SKINNED_JOINTS {
                continue;
            }
            if let Some(parent) = bone.parent_id {
                if (parent as usize) < MAX_SKINNED_JOINTS {
                    let mut frame = pose[parent as usize];
                    frame.w = pose[j].w;
                    palette[MAX_SKINNED_JOINTS + j] = frame;
                }
            }
        }
        palette
    }

    /// Per-joint inverse of the **rest-pose** global transform.
    ///
    /// Vanilla LGMM stores vertices already in joint-local space, so nothing
    /// needed this. A `PMNM` chunk stores them in bind-pose model space, so
    /// skinning has to undo the bind first: `pose[j] * bind_inverse[j] * v`.
    /// Call this on the un-animated skeleton.
    pub fn bind_inverse_transforms(&self) -> [Matrix4<f32>; MAX_SKINNED_JOINTS] {
        let mut out = [Matrix4::identity(); MAX_SKINNED_JOINTS];
        for (joint_id, global) in self.global_transforms.iter() {
            let j = *joint_id as usize;
            if j >= MAX_SKINNED_JOINTS {
                continue;
            }
            out[j] = global.invert().unwrap_or_else(Matrix4::identity);
        }
        out
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
    /// Sub-frame progress toward `frame + 1` (0..1). Poses interpolate
    /// between the two keyframes (per-joint rotation slerp, translation
    /// lerp), so playback above the clip's native frame rate advances
    /// smoothly instead of snapping to whole 30fps keyframes.
    pub fraction: f32,
    /// Whether the final keyframe interpolates toward frame 0 (looping
    /// clips). One-shots hold their final keyframe - their seams are the
    /// queue's business.
    pub wrap: bool,
    /// Cancel the clip's root motion: both the per-frame clip root transform
    /// and the root joint's animation transform are replaced with identity, so
    /// the posed skeleton stays anchored at the model origin and only the
    /// relative joints carry the gesture. Used for the first-person viewmodel,
    /// where the entity transform is re-anchored to the camera every frame (the
    /// original engine does this with a virtual "camSynch" motion that bolts
    /// the arm root to the camera).
    pub cancel_root_motion: bool,
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
        fraction,
        wrap,
        cancel_root_motion,
    }) = animation_info
    {
        let normalized_frame = (frame % animation_clip.num_frames) as usize;
        let sample = |frames: &Vec<Matrix4<f32>>| -> Matrix4<f32> {
            let Some(&current) = frames.get(normalized_frame.min(frames.len().saturating_sub(1)))
            else {
                // Empty keyframe track (malformed clip data) - bind pose.
                return Matrix4::identity();
            };
            if fraction <= f32::EPSILON {
                return current;
            }
            let next = if normalized_frame + 1 < frames.len() {
                Some(normalized_frame + 1)
            } else if wrap && frames.len() > 1 {
                Some(0)
            } else {
                None
            };
            match next {
                Some(next) => interpolate_transform(&current, &frames[next], fraction),
                None => current,
            }
        };
        let animations = &animation_clip.joint_to_frame;
        for key in animations {
            let (joint, frames) = key;
            animation_transforms.insert(*joint, sample(frames));
        }

        if cancel_root_motion {
            // Drop the clip's transform for the root joint(s) so only the bind
            // local applies; the per-frame clip root transform below is also
            // skipped. The remaining joints keep only their ROTATION - the
            // gesture is an orientation overlay on the rig's fixed bone
            // lengths, and the clips' per-joint translations carry root-motion
            // style displacement that would stretch the limbs. See
            // `AnimationInfo::cancel_root_motion`.
            for bone in &bones {
                if bone.parent_id.is_none() {
                    animation_transforms.remove(&bone.joint_id);
                }
            }
            for transform in animation_transforms.values_mut() {
                transform.w.x = 0.0;
                transform.w.y = 0.0;
                transform.w.z = 0.0;
            }
            Matrix4::identity()
        } else if animation_clip.root_transforms.is_empty() {
            Matrix4::identity()
        } else {
            sample(&animation_clip.root_transforms)
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

/// Interpolate between two affine keyframes: rotation by quaternion slerp
/// (basis columns normalized first - keyframes are rotation+translation, but
/// stay robust to scale), translation by lerp.
fn interpolate_transform(a: &Matrix4<f32>, b: &Matrix4<f32>, t: f32) -> Matrix4<f32> {
    let rot = |m: &Matrix4<f32>| -> Quaternion<f32> {
        let x = m.x.truncate().normalize();
        let y = m.y.truncate().normalize();
        let z = m.z.truncate().normalize();
        Quaternion::from(cgmath::Matrix3::from_cols(x, y, z)).normalize()
    };
    let blended = rot(a).slerp(rot(b), t).normalize();
    let translation = a.w.truncate() * (1.0 - t) + b.w.truncate() * t;
    let mut out = Matrix4::from(blended);
    out.w.x = translation.x;
    out.w.y = translation.y;
    out.w.z = translation.z;
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::motion::AnimationClip;
    use std::time::Duration;

    /// Two-bone skeleton (root joint 0, child joint 1 offset (1,0,0)) posed by
    /// a one-frame clip whose root joint, child joint, and per-frame root
    /// transform all carry translations.
    fn test_skeleton_and_clip() -> (Skeleton, AnimationClip) {
        let bones = vec![
            Bone {
                joint_id: 0,
                parent_id: None,
                local_transform: Matrix4::identity(),
            },
            Bone {
                joint_id: 1,
                parent_id: Some(0),
                local_transform: Matrix4::from_translation(Vector3::new(1.0, 0.0, 0.0)),
            },
        ];
        let mut joint_to_frame = HashMap::new();
        joint_to_frame.insert(
            0,
            vec![Matrix4::from_translation(Vector3::new(5.0, 0.0, 0.0))],
        );
        joint_to_frame.insert(
            1,
            vec![Matrix4::from_translation(Vector3::new(2.0, 0.0, 0.0))],
        );
        let clip = AnimationClip {
            num_frames: 1,
            time_per_frame: Duration::from_millis(33),
            duration: Duration::from_millis(33),
            blend_length: Duration::ZERO,
            end_rotation: Deg(0.0),
            sliding_velocity: Vector3::new(0.0, 0.0, 0.0),
            translation: Vector3::new(0.0, 0.0, 0.0),
            joint_to_frame,
            root_transforms: vec![Matrix4::from_translation(Vector3::new(0.0, 10.0, 0.0))],
            root_positions: Vec::new(),
            motion_flags: Vec::new(),
            name: None,
        };
        (Skeleton::create_from_bones(bones), clip)
    }

    #[test]
    fn expand_skinning_palette_builds_parent_frames() {
        let (skeleton, _clip) = test_skeleton_and_clip();
        let mut pose = [Matrix4::identity(); 40];
        // Root (joint 0): a recognizable rotation + its own translation.
        pose[0] = Matrix4::from_translation(Vector3::new(5.0, 0.0, 0.0))
            * Matrix4::from_angle_y(Deg(90.0));
        // Child (joint 1): identity rotation, distinct translation.
        pose[1] = Matrix4::from_translation(Vector3::new(6.0, 2.0, 0.0));

        let palette = Skeleton::expand_skinning_palette(&pose, &skeleton);

        // Slots 0..40 are the pose itself.
        assert_eq!(palette[0], pose[0]);
        assert_eq!(palette[1], pose[1]);

        // Joint 1's parent frame: the ROOT's orientation anchored at JOINT 1's
        // position - stretchy weight 0 rotates with the parent about the
        // child pivot.
        let frame = palette[41];
        assert_eq!(
            translation_from_matrix(&frame),
            Vector3::new(6.0, 2.0, 0.0),
            "parent frame must pivot at the child joint"
        );
        assert_eq!(frame.x, pose[0].x, "parent frame carries parent basis x");
        assert_eq!(frame.z, pose[0].z, "parent frame carries parent basis z");

        // Root has no parent; an unused slot has no bone: both fall back to
        // the joint's own transform so a second-bone reference is a no-op.
        assert_eq!(palette[40], pose[0]);
        assert_eq!(palette[45], pose[5]);
    }

    #[test]
    fn animate_applies_clip_root_motion_by_default() {
        let (skeleton, clip) = test_skeleton_and_clip();
        let posed = animate(
            &skeleton,
            Some(AnimationInfo {
                animation_clip: &clip,
                frame: 0,
                fraction: 0.0,
                wrap: false,
                cancel_root_motion: false,
            }),
            &immutable::HashTrieMap::new(),
        );
        let transforms = posed.get_transforms();
        // Root = clip root transform (0,10,0) * root joint anim (5,0,0).
        assert_eq!(
            translation_from_matrix(&transforms[0]),
            Vector3::new(5.0, 10.0, 0.0)
        );
    }

    #[test]
    fn animate_interpolates_between_keyframes_at_fractional_positions() {
        let (skeleton, mut clip) = test_skeleton_and_clip();
        // Two keyframes for the child joint: translation steps (2,0,0) ->
        // (4,0,0); rotation identity -> 90 deg yaw. Root static both frames.
        clip.num_frames = 2;
        clip.joint_to_frame.insert(
            0,
            vec![
                Matrix4::from_translation(Vector3::new(5.0, 0.0, 0.0)),
                Matrix4::from_translation(Vector3::new(5.0, 0.0, 0.0)),
            ],
        );
        clip.joint_to_frame.insert(
            1,
            vec![
                Matrix4::from_translation(Vector3::new(2.0, 0.0, 0.0)),
                Matrix4::from_translation(Vector3::new(4.0, 0.0, 0.0))
                    * Matrix4::from_angle_y(Deg(90.0)),
            ],
        );
        clip.root_transforms = vec![Matrix4::identity(), Matrix4::identity()];

        let posed = animate(
            &skeleton,
            Some(AnimationInfo {
                animation_clip: &clip,
                frame: 0,
                fraction: 0.5,
                wrap: false,
                cancel_root_motion: false,
            }),
            &immutable::HashTrieMap::new(),
        );
        // Joint 1 global = root(5,0,0) * bone local(1,0,0) * anim(interp):
        // translation lerps to (3,0,0) -> global (9,0,0); rotation slerps to
        // a 45 deg yaw.
        let child = posed.get_transforms()[1];
        assert!(
            (translation_from_matrix(&child) - Vector3::new(9.0, 0.0, 0.0)).magnitude() < 1e-5,
            "translation must lerp midway, got {:?}",
            translation_from_matrix(&child)
        );
        let yaw = child.z.x.atan2(child.z.z).to_degrees();
        assert!(
            (yaw - 45.0).abs() < 0.5,
            "rotation must slerp midway (expected 45 deg yaw, got {yaw})"
        );
    }

    #[test]
    fn animate_wraps_interpolation_on_looping_clips() {
        let (skeleton, mut clip) = test_skeleton_and_clip();
        // Child joint: (2,0,0) at frame 0, (4,0,0) at frame 1. Sampling the
        // FINAL frame with fraction 0.5 and wrap on must head back toward
        // frame 0: lerp((4,0,0), (2,0,0), 0.5) = (3,0,0).
        clip.num_frames = 2;
        clip.joint_to_frame
            .insert(0, vec![Matrix4::identity(), Matrix4::identity()]);
        clip.joint_to_frame.insert(
            1,
            vec![
                Matrix4::from_translation(Vector3::new(2.0, 0.0, 0.0)),
                Matrix4::from_translation(Vector3::new(4.0, 0.0, 0.0)),
            ],
        );
        clip.root_transforms = vec![Matrix4::identity(), Matrix4::identity()];

        let sample_child = |wrap: bool| {
            let posed = animate(
                &skeleton,
                Some(AnimationInfo {
                    animation_clip: &clip,
                    frame: 1,
                    fraction: 0.5,
                    wrap,
                    cancel_root_motion: false,
                }),
                &immutable::HashTrieMap::new(),
            );
            translation_from_matrix(&posed.get_transforms()[1])
        };

        // bone local (1,0,0) + anim translation.
        assert!(
            (sample_child(true) - Vector3::new(4.0, 0.0, 0.0)).magnitude() < 1e-5,
            "looping: final frame interpolates toward frame 0"
        );
        assert!(
            (sample_child(false) - Vector3::new(5.0, 0.0, 0.0)).magnitude() < 1e-5,
            "one-shot: final frame holds"
        );
    }

    #[test]
    fn animate_cancel_root_motion_pins_root_and_strips_joint_translations() {
        let (skeleton, clip) = test_skeleton_and_clip();
        let posed = animate(
            &skeleton,
            Some(AnimationInfo {
                animation_clip: &clip,
                frame: 0,
                fraction: 0.0,
                wrap: false,
                cancel_root_motion: true,
            }),
            &immutable::HashTrieMap::new(),
        );
        let transforms = posed.get_transforms();
        // Root stays at the model origin: both the per-frame clip root
        // transform and the root joint's animation transform are cancelled.
        assert_eq!(
            translation_from_matrix(&transforms[0]),
            Vector3::new(0.0, 0.0, 0.0)
        );
        // The child keeps its bind offset; the clip's per-joint translation
        // (which would stretch the limb) is stripped.
        assert_eq!(
            translation_from_matrix(&transforms[1]),
            Vector3::new(1.0, 0.0, 0.0)
        );
    }
}
