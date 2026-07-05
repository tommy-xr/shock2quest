use std::io::{self, SeekFrom};

use cgmath::{Matrix4, SquareMatrix, Vector3, vec3};

use crate::{
    SCALE_FACTOR,
    motion::MpsMotion,
    ss2_common::{read_array_u32, read_quat, read_u32, read_vec3},
};

#[derive(Debug)]
pub struct MotionClip {
    pub num_joints: u32,
    pub root_transforms: Vec<Matrix4<f32>>, // root transforms across frames
    /// Full per-frame root positions (scaled). The y also drives
    /// `root_transforms`; the x/z deltas drive entity movement.
    pub root_positions: Vec<Vector3<f32>>,
    pub animation: Vec<Vec<Matrix4<f32>>>, // joint -> animations across frames
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
        let mut root_positions = Vec::new();
        let mut frame_transforms = Vec::new();
        for _frame in 0..num_frames {
            // We handle the root transforms in a special way,
            // but we still need to populate Joint 0 for the other animations
            // to work correctly
            frame_transforms.push(Matrix4::identity());

            // The pose only carries the root's y (vertical bob/descent); the
            // x/z displacement moves the entity instead, driven by the
            // per-frame deltas of `root_positions`.
            let xform = read_vec3(reader);
            root_positions.push(xform / SCALE_FACTOR);
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
            root_transforms,
            root_positions,
            num_joints,
            animation,
        }
    }
}
