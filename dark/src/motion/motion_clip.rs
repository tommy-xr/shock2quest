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

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;
    use crate::motion::MotionComponent;

    /// Serialize a minimal 2-track .mc payload: a root position track and one
    /// joint rotation track built from `quat_wxzy` (the on-disk w,-x,z,y
    /// order), repeated for every frame.
    fn clip_bytes(num_frames: u32, quat_wxzy: [f32; 4]) -> Vec<u8> {
        let num_joints = 2u32;
        let header_len = 4 + 4 * num_joints;
        let root_len = num_frames * 12;
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&num_joints.to_le_bytes());
        bytes.extend_from_slice(&header_len.to_le_bytes());
        bytes.extend_from_slice(&(header_len + root_len).to_le_bytes());
        for _ in 0..num_frames {
            for c in [0.0f32; 3] {
                bytes.extend_from_slice(&c.to_le_bytes());
            }
        }
        for _ in 0..num_frames {
            for c in quat_wxzy {
                bytes.extend_from_slice(&c.to_le_bytes());
            }
        }
        bytes
    }

    fn mps_motion(num_frames: u32) -> MpsMotion {
        MpsMotion {
            motion_type: 0,
            motion_components: (0..2)
                .map(|joint_id| MotionComponent {
                    motion_type: 0,
                    joint_id,
                    handle: 0,
                })
                .collect(),
            motion_flags: Vec::new(),
            sig: 0,
            frame_count: num_frames as f32,
            frame_rate: 30,
            mot_num: 0,
            name: "test".to_owned(),
        }
    }

    /// 23 of the 613 stock motion clips (e.g. bh114009, humwalks, humalert)
    /// carry an all-zero quaternion track for joints the clip doesn't
    /// animate. Inverting that zero quaternion divides by zero and floods the
    /// track - and every pose blended from it - with NaN (issue #508: NaN
    /// joint transforms fed kinematic hitboxes, poisoning the physics
    /// broad-phase). A zero quaternion must parse as the identity rotation,
    /// exactly like the original engine's unnormalized quat->matrix
    /// conversion treats it.
    #[test]
    fn zero_quaternion_track_parses_as_identity_not_nan() {
        let bytes = clip_bytes(3, [0.0, 0.0, 0.0, 0.0]);
        let mps = mps_motion(3);
        let clip = MotionClip::read(&mut Cursor::new(bytes), &mps);

        for (joint, frames) in clip.animation.iter().enumerate() {
            for (frame, m) in frames.iter().enumerate() {
                let cells: &[f32; 16] = m.as_ref();
                assert!(
                    cells.iter().all(|c| c.is_finite()),
                    "joint {joint} frame {frame} must stay finite, got {m:?}"
                );
                assert_eq!(
                    *m,
                    Matrix4::identity(),
                    "an unanimated (zero-quaternion) track must pose as identity"
                );
            }
        }
    }

    /// A genuine unit-quaternion track must be untouched by the zero-quat
    /// guard: 90 degrees about +y on disk still comes out as 90 degrees about
    /// +y (inverted per the engine's handedness fixup, like every clip since).
    #[test]
    fn unit_quaternion_track_still_parses_as_its_rotation() {
        let half = std::f32::consts::FRAC_1_SQRT_2;
        // On-disk order is (w, -x, z, y): a +y rotation stores its y in the
        // final slot.
        let bytes = clip_bytes(1, [half, 0.0, 0.0, half]);
        let mps = mps_motion(1);
        let clip = MotionClip::read(&mut Cursor::new(bytes), &mps);

        let m = clip.animation[1][0];
        let cells: &[f32; 16] = m.as_ref();
        assert!(cells.iter().all(|c| c.is_finite()));
        // invert() of a +90deg yaw is a -90deg yaw: x axis maps to +z.
        assert!(
            (m.x.z - 1.0).abs() < 1e-5 && (m.x.x).abs() < 1e-5,
            "expected a yaw rotation, got {m:?}"
        );
    }
}
