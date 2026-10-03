use std::{collections::HashMap, rc::Rc, time::Duration};

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
    /// Shared, so a retimed copy (`with_timing`) does not duplicate keyframes.
    pub joint_to_frame: Rc<HashMap<JointId, Vec<Matrix4<f32>>>>,
    pub root_transforms: Vec<Matrix4<f32>>, // root transforms per frame
    /// Full per-frame root positions (scaled); drives per-frame root-motion
    /// velocity. Empty for clips without a root stream (e.g. GLB clips).
    pub root_positions: Vec<Vector3<f32>>,
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
            joint_to_frame: Rc::new(joint_to_frame),
            root_transforms: motion_clip.root_transforms.clone(),
            root_positions: motion_clip.root_positions.clone(),
            time_per_frame,
            motion_flags: mps_motion.motion_flags.clone(),
            sliding_velocity,
            translation: motion_stuff.translation,
            end_rotation,
            name: Some(mps_motion.name.clone()), // Use motion name for traditional SS2 animations
        }
    }

    /// Frames the clip plays across: a loop wraps from its last keyframe back
    /// to the first, a one-shot ends ON its last keyframe (a single-keyframe
    /// pose still lasts one frame).
    pub fn play_frames(&self, looping: bool) -> u32 {
        if looping {
            self.num_frames
        } else {
            self.num_frames.saturating_sub(1).max(1)
        }
    }

    /// How long the clip plays (`play_frames` frame periods).
    pub fn play_duration(&self, looping: bool) -> Duration {
        self.time_per_frame * self.play_frames(looping)
    }

    /// This clip played under a schema's timing: `time_scale` multiplies its
    /// duration and `stretch` its root travel (see `SchemaTiming`). The pose is
    /// untouched; only the clock and the travel that drives movement change.
    pub fn with_timing(&self, time_scale: f32, stretch: f32) -> AnimationClip {
        let mut clip = self.clone();
        clip.time_per_frame = self.time_per_frame.mul_f32(time_scale);
        clip.duration = self.duration.mul_f32(time_scale);
        clip.translation = self.translation * stretch;
        clip.sliding_velocity = self.sliding_velocity * (stretch / time_scale);
        for position in &mut clip.root_positions {
            *position *= stretch;
        }
        clip
    }

    /// Instantaneous root-motion velocity at `frame`: the rate implied by
    /// the clip's per-frame root delta, so entity movement tracks the
    /// authored motion instead of the clip-average. `None` when the clip has
    /// no usable root stream (fall back to `sliding_velocity`).
    ///
    /// Samples the segment starting at `frame`; on the final frame (which has
    /// no forward segment) it uses the trailing segment. That keeps a looping
    /// walk moving at its stride rate across the wrap instead of stalling for
    /// a frame, while a one-shot clip that rests at its end still reads ~0
    /// there (its trailing frames don't move).
    pub fn root_velocity_at(&self, frame: u32) -> Option<Vector3<f32>> {
        if self.root_positions.len() < 2 {
            return None;
        }
        let last = self.root_positions.len() - 1;
        let f0 = (frame as usize).min(last - 1);
        Some(
            (self.root_positions[f0 + 1] - self.root_positions[f0])
                / self.time_per_frame.as_secs_f32(),
        )
    }

    /// Root travel between two playback positions, in frames, along the
    /// per-frame root stream. Past the final frame the trailing stride
    /// continues (as in `root_velocity_at`), and a looping clip carries whole
    /// cycles across the wrap. `None` when the clip has no root stream.
    pub fn root_travel(&self, from: f32, to: f32, looping: bool) -> Option<Vector3<f32>> {
        if self.root_positions.len() < 2 {
            return None;
        }
        Some(self.root_position(to, looping) - self.root_position(from, looping))
    }

    fn root_position(&self, pos: f32, looping: bool) -> Vector3<f32> {
        let positions = &self.root_positions;
        let last = positions.len() - 1;
        let frames = positions.len() as f32;
        let trailing = positions[last] - positions[last - 1];
        let within = |pos: f32| {
            if pos >= last as f32 {
                positions[last] + trailing * (pos - last as f32)
            } else {
                let frame = pos.floor() as usize;
                let t = pos - frame as f32;
                positions[frame] + (positions[frame + 1] - positions[frame]) * t
            }
        };
        let pos = pos.max(0.0);
        if looping && pos >= frames {
            let cycles = (pos / frames).floor();
            let cycle_travel = positions[last] - positions[0] + trailing;
            cycle_travel * cycles + within(pos - cycles * frames)
        } else {
            // A one-shot ends on its last keyframe (see `play_frames`)
            within(pos.min(last as f32))
        }
    }
}
