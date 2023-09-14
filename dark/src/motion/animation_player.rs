use std::{rc::Rc, time::Duration};

use cgmath::{vec3, Deg, Matrix4, Vector3};
use num::complex::ComplexFloat;
use rpds as immutable;

use crate::ss2_skeleton::{self, Skeleton};

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
pub struct AnimationPlayer {
    pub animation: immutable::List<(Rc<AnimationClip>, AnimationFlags)>,
    pub additional_joint_transforms: immutable::HashTrieMap<u32, Matrix4<f32>>,
    pub last_animation: Option<Rc<AnimationClip>>,
    pub current_frame: u32,
    pub remaining_time: f32,
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
        }
    }
    pub fn queue_animation(
        player: &AnimationPlayer,
        animation: Rc<AnimationClip>,
    ) -> AnimationPlayer {
        let new_animation = player
            .animation
            .push_front((animation, AnimationFlags::PlayOnce));

        AnimationPlayer {
            additional_joint_transforms: player.additional_joint_transforms.clone(),
            animation: new_animation,
            last_animation: None,
            current_frame: 0,
            remaining_time: 0.0,
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
        let maybe_current_clip = player.animation.first();

        if maybe_current_clip.is_none() {
            let motion_flags = MotionFlags::empty();
            (player.clone(), motion_flags, vec![], vec3(0.0, 0.0, 0.0))
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
                    },
                    motion_flags,
                    events,
                    velocity,
                )
            }
        }
    }

    pub fn get_transforms(&self, skeleton: &Skeleton) -> [Matrix4<f32>; 40] {
        let maybe_current_clip = self
            .animation
            .first()
            .map(|m| (m.0.clone(), false))
            .or_else(|| self.last_animation.clone().map(|m| (m, true)));

        if maybe_current_clip.is_none() {
            skeleton.get_transforms()
        } else {
            let (current_clip, is_last_anim) = maybe_current_clip.unwrap();
            let current_frame = if is_last_anim {
                current_clip.num_frames - 1
            } else {
                self.current_frame
            };
            let animated_skeleton = ss2_skeleton::animate(
                skeleton,
                &current_clip,
                current_frame,
                &self.additional_joint_transforms,
            );
            let mut ret = animated_skeleton.get_transforms();

            for i in 0..40 {
                ret[i] = Matrix4::from_translation(
                    (current_frame as f32 / current_clip.num_frames as f32)
                        * vec3(0.0, current_clip.translation.y, 0.0),
                ) * ret[i];
            }
            ret
        }
    }
}
