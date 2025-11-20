use std::io::{self, SeekFrom};

use cgmath::{Matrix4, SquareMatrix, vec3};

use crate::{
    SCALE_FACTOR,
    motion::MpsMotion,
    ss2_common::{read_array_u32, read_quat, read_u32, read_vec3},
};

#[derive(Debug)]
pub struct MotionClip {
    pub num_joints: u32,
    pub root_transforms: Vec<Matrix4<f32>>, // root transforms across frames
    pub animation: Vec<Vec<Matrix4<f32>>>,  // joint -> animations across frames
}

impl MotionClip {
    pub fn read<T: io::Read + io::Seek>(reader: &mut T, mps_motion: &MpsMotion) -> MotionClip {
        let num_joints = read_u32(reader);

        let joint_offsets = read_array_u32(reader, num_joints);
        let num_frames = mps_motion.frame_count.floor() as u32;

        let _ = reader.seek(SeekFrom::Start(joint_offsets[0] as u64));
        // Read transforms for root joint
        let mut animation = Vec::new();
        let mut root_transforms = Vec::new();
        let mut frame_transforms = Vec::new();
        for _frame in 0..num_frames {
            // We handle the root transforms in a special way,
            // but we still need to populate Joint 0 for the other animations
            // to work correctly
            frame_transforms.push(Matrix4::identity());

            // Record the root translation - we _only_ record the y, because
            // the x/z transform is handled by our movement system.
            let xform = read_vec3(reader);
            root_transforms.push(Matrix4::from_translation(vec3(
                0.0,
                xform.y / SCALE_FACTOR,
                0.0,
            )));
        }
        animation.push(frame_transforms);

        // animation for each joint
        for joint in 1..num_joints {
            let _ = reader.seek(SeekFrom::Start(joint_offsets[joint as usize] as u64));
            let mut frame_rotations = Vec::new();
            for _frame in 0..num_frames {
                let quat = read_quat(reader);
                let xform = Matrix4::from(quat);
                frame_rotations.push(xform);
            }
            animation.push(frame_rotations);
        }

        MotionClip {
            root_transforms: root_transforms,
            num_joints,
            animation,
        }
    }
}
