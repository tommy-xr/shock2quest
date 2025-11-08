use std::{rc::Rc, time::Duration};

use cgmath::InnerSpace;
use cgmath::{Deg, Matrix3, Matrix4, Quaternion, Vector3, vec3};
use rpds as immutable;

use crate::ss2_skeleton::{self, AnimationInfo, Skeleton};

use super::{AnimationClip, MotionFlags};
pub enum AnimationFlags {
    Loop,
    PlayOnce,
}

pub enum AnimationEvent {
    DirectionChanged(Deg<f32>),
    VelocityChanged(Vector3<f32>),
    Completed,
}

#[derive(Clone)]
struct BlendState {
    from_clip: Rc<AnimationClip>,
    from_frame: f32,
    duration: f32,
    elapsed: f32,
}

#[derive(Clone)]
pub struct AnimationPlayer {
    animation: immutable::List<(Rc<AnimationClip>, AnimationFlags)>,
    additional_joint_transforms: immutable::HashTrieMap<u32, Matrix4<f32>>,
    last_animation: Option<Rc<AnimationClip>>,
    current_frame: u32,
    remaining_time: f32,
    blend_state: Option<BlendState>,
}

impl AnimationPlayer {
    pub fn empty() -> AnimationPlayer {
        let animation = immutable::List::new();
        AnimationPlayer {
            animation,
            additional_joint_transforms: immutable::HashTrieMap::new(),
            last_animation: None,
            current_frame: 0,
            remaining_time: 0.0,
            blend_state: None,
        }
    }
    pub fn from_animation(animation_clip: Rc<AnimationClip>) -> AnimationPlayer {
        let animation = immutable::List::new();
        let animation = animation.push_front((animation_clip, AnimationFlags::Loop));
        AnimationPlayer {
            additional_joint_transforms: immutable::HashTrieMap::new(),
            animation,
            last_animation: None,
            current_frame: 0,
            remaining_time: 0.0,
            blend_state: None,
        }
    }
    pub fn queue_animation(
        player: &AnimationPlayer,
        animation: Rc<AnimationClip>,
    ) -> AnimationPlayer {
        let new_animation = player
            .animation
            .push_front((animation.clone(), AnimationFlags::PlayOnce));

        let blend_state = if let Some((current_clip, _)) = player.animation.first() {
            let duration = animation.blend_length.as_secs_f32();
            if duration > 0.0 {
                Some(BlendState {
                    from_clip: current_clip.clone(),
                    from_frame: player.current_frame as f32,
                    duration,
                    elapsed: 0.0,
                })
            } else {
                None
            }
        } else {
            None
        };

        AnimationPlayer {
            additional_joint_transforms: player.additional_joint_transforms.clone(),
            animation: new_animation,
            last_animation: None,
            current_frame: 0,
            remaining_time: 0.0,
            blend_state,
        }
    }

    pub fn set_additional_joint_transform(
        player: &AnimationPlayer,
        joint_idx: u32,
        transform: Matrix4<f32>,
    ) -> AnimationPlayer {
        let new_transforms = player
            .additional_joint_transforms
            .insert(joint_idx, transform);
        AnimationPlayer {
            additional_joint_transforms: new_transforms,
            animation: player.animation.clone(),
            last_animation: player.last_animation.clone(),
            current_frame: player.current_frame,
            remaining_time: player.remaining_time,
            blend_state: player.blend_state.clone(),
        }
    }

    pub fn update(
        player: &AnimationPlayer,
        time: Duration,
    ) -> (
        AnimationPlayer,
        MotionFlags,
        Vec<AnimationEvent>,
        Vector3<f32>,
    ) {
        let mut remaining_duration = player.remaining_time + time.as_secs_f32();
        let mut blend_state = player.blend_state.clone();
        let mut clear_blend = false;

        if let Some(blend) = blend_state.as_mut() {
            blend.elapsed += time.as_secs_f32();

            if blend.from_clip.num_frames > 0 {
                let frames_advance =
                    time.as_secs_f32() / blend.from_clip.time_per_frame.as_secs_f32();
                let mut frame = blend.from_frame + frames_advance;
                let frame_count = blend.from_clip.num_frames as f32;
                if frame >= frame_count && frame_count > 0.0 {
                    frame = frame % frame_count;
                }
                blend.from_frame = frame;
            }

            if blend.duration <= f32::EPSILON || blend.elapsed >= blend.duration {
                clear_blend = true;
            }
        }

        if clear_blend {
            blend_state = None;
        }

        let maybe_current_clip = player.animation.first();

        if maybe_current_clip.is_none() {
            let motion_flags = MotionFlags::empty();
            let mut updated_player = player.clone();
            updated_player.blend_state = blend_state;
            (updated_player, motion_flags, vec![], vec3(0.0, 0.0, 0.0))
        } else {
            let (current_clip, flags) = maybe_current_clip.unwrap();
            let velocity = current_clip.sliding_velocity;
            let mut next_frame = player.current_frame;
            let time_per_frame = current_clip.time_per_frame.as_secs_f32();
            while remaining_duration >= time_per_frame {
                remaining_duration -= time_per_frame;
                next_frame += 1;
            }

            let motion_flags = {
                let mut output = MotionFlags::empty();
                for flag in &current_clip.motion_flags {
                    if flag.frame > player.current_frame && flag.frame <= next_frame {
                        output = output.union(flag.flags);
                    }
                }
                output
            };

            if next_frame >= current_clip.num_frames {
                let mut events = Vec::new();

                events.push(AnimationEvent::Completed);

                if current_clip.end_rotation != Deg(0.0) {
                    events.push(AnimationEvent::DirectionChanged(current_clip.end_rotation));
                }

                match flags {
                    AnimationFlags::Loop => (
                        AnimationPlayer {
                            additional_joint_transforms: player.additional_joint_transforms.clone(),
                            last_animation: player.last_animation.clone(),
                            animation: player.animation.clone(),
                            current_frame: next_frame - current_clip.num_frames,
                            remaining_time: remaining_duration,
                            blend_state,
                        },
                        motion_flags,
                        events,
                        velocity,
                    ),
                    AnimationFlags::PlayOnce => {
                        let last_animation = player.animation.first().map(|m| m.0.clone());
                        let animation = player.animation.drop_first().unwrap_or_default();
                        (
                            AnimationPlayer {
                                additional_joint_transforms: player
                                    .additional_joint_transforms
                                    .clone(),
                                animation,
                                last_animation,
                                current_frame: 0,
                                remaining_time: 0.0,
                                blend_state,
                            },
                            motion_flags,
                            events,
                            velocity,
                        )
                    }
                }
            } else {
                let events = if !player.animation.is_empty()
                    && player.current_frame == 0
                    && next_frame > 0
                {
                    let animation = player.animation.first().unwrap();
                    vec![AnimationEvent::VelocityChanged(
                        animation.0.sliding_velocity,
                    )]
                } else {
                    vec![]
                };
                (
                    AnimationPlayer {
                        additional_joint_transforms: player.additional_joint_transforms.clone(),
                        last_animation: player.last_animation.clone(),
                        animation: player.animation.clone(),
                        current_frame: next_frame,
                        remaining_time: remaining_duration,
                        blend_state,
                    },
                    motion_flags,
                    events,
                    velocity,
                )
            }
        }
    }

    pub fn get_transforms(&self, skeleton: &Skeleton) -> [Matrix4<f32>; 40] {
        // We need to clarify if this animation is the current run, or a carry over from the previous one,
        // so add a separate boolean flag `is_last_anim` if we fallback to the last_animation.
        let maybe_current_clip = self
            .animation
            .first()
            .map(|m| (m.0.clone(), false))
            .or_else(|| self.last_animation.clone().map(|m| (m, true)));

        // If there is no animation, we still may need to apply joint transforms (ie, for camera or turret)
        if maybe_current_clip.is_none() {
            let animated_skeleton =
                ss2_skeleton::animate(skeleton, None, &self.additional_joint_transforms);
            return animated_skeleton.get_transforms();
        }

        let (rc_animation_clip, is_last_anim) = maybe_current_clip.unwrap();
        let current_clip = rc_animation_clip.as_ref();

        let current_frame = if is_last_anim {
            current_clip.num_frames - 1
        } else {
            self.current_frame
        };

        let mut animated_transforms = Self::compute_transforms_for_clip(
            skeleton,
            current_clip,
            current_frame,
            &self.additional_joint_transforms,
        );

        if let Some(blend) = &self.blend_state {
            if blend.duration > f32::EPSILON && blend.elapsed < blend.duration {
                let alpha = (blend.elapsed / blend.duration).clamp(0.0, 1.0);
                let frame = if blend.from_clip.num_frames > 0 {
                    (blend.from_frame.floor() as u32) % blend.from_clip.num_frames
                } else {
                    0
                };

                let from_transforms = Self::compute_transforms_for_clip(
                    skeleton,
                    &blend.from_clip,
                    frame,
                    &self.additional_joint_transforms,
                );

                animated_transforms =
                    Self::blend_transforms(&from_transforms, &animated_transforms, alpha);
            }
        }

        animated_transforms
    }

    fn compute_transforms_for_clip(
        skeleton: &Skeleton,
        clip: &AnimationClip,
        frame: u32,
        additional_joint_transforms: &immutable::HashTrieMap<u32, Matrix4<f32>>,
    ) -> [Matrix4<f32>; 40] {
        let animated_skeleton = ss2_skeleton::animate(
            skeleton,
            Some(AnimationInfo {
                animation_clip: clip,
                frame,
            }),
            additional_joint_transforms,
        );

        // TODO: We're not handling whatever this vertical translation is correctly right now
        // let mut transforms = animated_skeleton.get_transforms();
        // let frame_ratio = if clip.num_frames > 0 {
        //     frame as f32 / clip.num_frames as f32
        // } else {
        //     0.0
        // };

        // for matrix in transforms.iter_mut() {
        //     *matrix = Matrix4::from_translation(frame_ratio * vec3(0.0, clip.translation.y, 0.0))
        //         * *matrix;
        // }

        // transforms

        animated_skeleton.get_transforms()
    }

    fn blend_transforms(
        from: &[Matrix4<f32>; 40],
        to: &[Matrix4<f32>; 40],
        alpha: f32,
    ) -> [Matrix4<f32>; 40] {
        if alpha <= 0.0 {
            return *from;
        }

        if alpha >= 1.0 {
            return *to;
        }

        let mut result = [Matrix4::from_scale(1.0); 40];

        for (idx, output) in result.iter_mut().enumerate() {
            let from_matrix = from[idx];
            let to_matrix = to[idx];

            let from_translation = Vector3::new(from_matrix.w.x, from_matrix.w.y, from_matrix.w.z);
            let to_translation = Vector3::new(to_matrix.w.x, to_matrix.w.y, to_matrix.w.z);
            let blended_translation = from_translation * (1.0 - alpha) + to_translation * alpha;

            let from_rotation = Matrix3::new(
                from_matrix.x.x,
                from_matrix.x.y,
                from_matrix.x.z,
                from_matrix.y.x,
                from_matrix.y.y,
                from_matrix.y.z,
                from_matrix.z.x,
                from_matrix.z.y,
                from_matrix.z.z,
            );
            let to_rotation = Matrix3::new(
                to_matrix.x.x,
                to_matrix.x.y,
                to_matrix.x.z,
                to_matrix.y.x,
                to_matrix.y.y,
                to_matrix.y.z,
                to_matrix.z.x,
                to_matrix.z.y,
                to_matrix.z.z,
            );

            let from_quat = Quaternion::from(from_rotation).normalize();
            let to_quat = Quaternion::from(to_rotation).normalize();
            let blended_quat = from_quat.slerp(to_quat, alpha).normalize();

            let mut blended_matrix = Matrix4::from(blended_quat);
            blended_matrix.w.x = blended_translation.x;
            blended_matrix.w.y = blended_translation.y;
            blended_matrix.w.z = blended_translation.z;

            *output = blended_matrix;
        }

        result
    }
}
