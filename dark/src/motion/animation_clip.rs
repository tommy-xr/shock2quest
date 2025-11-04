use std::{collections::HashMap, time::Duration};

use cgmath::{Deg, Matrix4, Vector3};

use crate::motion::{JointId, MpsMotion};

use super::{FrameFlags, MotionClip, MotionStuff};

#[derive(Clone)]
pub struct AnimationClip {
    pub num_frames: u32,
    pub time_per_frame: Duration,
    pub duration: Duration,
    pub blend_length: Duration,
    pub end_rotation: Deg<f32>,
    pub sliding_velocity: Vector3<f32>,
    pub translation: Vector3<f32>,
    pub joint_to_frame: HashMap<JointId, Vec<Matrix4<f32>>>,
    pub motion_flags: Vec<FrameFlags>,
    pub name: Option<String>, // Added for GLB animation support
}

impl AnimationClip {
    pub fn create(
        motion_clip: &MotionClip,
        mps_motion: &MpsMotion,
        motion_stuff: &MotionStuff,
    ) -> AnimationClip {
        let num_frames = mps_motion.frame_count as u32;

        // Verify that mps_motion has the equivalent data to the motion info!
        // assert!(mps_motion.motion_type == motion_info.motion_type);
        // assert!(mps_motion.sig == motion_info.sig);
        // assert!(mps_motion.frame_count == motion_info.frame_count);
        // assert!(mps_motion.frame_rate == motion_info.frame_rate);
        // assert!(mps_motion.mot_num == motion_info.mot_num);
        // assert!(mps_motion.name == motion_info.name);

        // TODO: Figure out duration
        //let framerate = motion_info.frame_rate as u32;

        let time_per_frame = Duration::from_secs_f32(1.0 / mps_motion.frame_rate as f32);
        let duration = time_per_frame * (mps_motion.frame_count as u32);

        let sliding_velocity = motion_stuff.translation / duration.as_secs_f32();
        let end_rotation = motion_stuff.end_direction;

        let mut joint_to_frame = HashMap::new();

        let animation = &motion_clip.animation;
        let joint_count = animation.len();
        for joint_index in 0..joint_count {
            let joint_id = mps_motion.get_joint_id(joint_index as u32);

            let frames = animation[joint_index].clone();
            joint_to_frame.insert(joint_id, frames);
        }

        AnimationClip {
            num_frames,
            duration,
            blend_length: Duration::from_millis(motion_stuff.blend_length as u64),
            joint_to_frame,
            time_per_frame,
            motion_flags: mps_motion.motion_flags.clone(),
            sliding_velocity,
            translation: motion_stuff.translation,
            end_rotation,
            name: Some(mps_motion.name.clone()), // Use motion name for traditional SS2 animations
        }
    }
}
