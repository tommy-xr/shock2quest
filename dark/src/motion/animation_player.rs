use std::{rc::Rc, time::Duration};

use cgmath::{vec3, Deg, Matrix4, Vector3};
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
pub struct AnimationPlayer {
    animation: immutable::List<(Rc<AnimationClip>, AnimationFlags)>,
    additional_joint_transforms: immutable::HashTrieMap<u32, Matrix4<f32>>,
    last_animation: Option<Rc<AnimationClip>>,
    current_frame: u32,
    remaining_time: f32,
    // Blending state
    blend_state: Option<BlendState>,
}

#[derive(Clone)]
struct BlendState {
    outgoing_clip: Rc<AnimationClip>,
    outgoing_frame: u32,
    remaining_blend_time: f32,
    total_blend_time: f32,
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

        // Set up blending if there's a current animation and blend_length > 0
        let blend_state = if !player.animation.is_empty() && animation.blend_length.as_millis() > 0 {
            let current_clip = player.animation.first().unwrap().0.clone();
            Some(BlendState {
                outgoing_clip: current_clip,
                outgoing_frame: player.current_frame,
                remaining_blend_time: animation.blend_length.as_secs_f32(),
                total_blend_time: animation.blend_length.as_secs_f32(),
            })
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
        let maybe_current_clip = player.animation.first();

        if maybe_current_clip.is_none() {
            let motion_flags = MotionFlags::empty();
            return (player.clone(), motion_flags, vec![], vec3(0.0, 0.0, 0.0));
        }

        let (current_clip, flags) = maybe_current_clip.unwrap();
        let velocity = current_clip.sliding_velocity;
        let mut next_frame = player.current_frame;
        let time_per_frame = current_clip.time_per_frame.as_secs_f32();

        // Advance animation frame
        while remaining_duration >= time_per_frame {
            remaining_duration -= time_per_frame;
            next_frame += 1;
        }

        // Update blend state if we're blending
        let mut updated_blend_state = player.blend_state.clone();
        if let Some(ref mut blend_state) = updated_blend_state {
            blend_state.remaining_blend_time -= time.as_secs_f32();

            // If blend is finished, clear blend state
            if blend_state.remaining_blend_time <= 0.0 {
                updated_blend_state = None;
            }
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
                        blend_state: updated_blend_state,
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
                            additional_joint_transforms: player.additional_joint_transforms.clone(),
                            animation,
                            last_animation,
                            current_frame: 0,
                            remaining_time: 0.0,
                            blend_state: None, // Clear blend state when animation completes
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
                vec![AnimationEvent::VelocityChanged(animation.0.sliding_velocity)]
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
                    blend_state: updated_blend_state,
                },
                motion_flags,
                events,
                velocity,
            )
        }
    }

    pub fn get_transforms(&self, skeleton: &Skeleton) -> [Matrix4<f32>; 40] {
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

        // Check if we're in a blend state
        if let Some(ref blend_state) = self.blend_state {
            // We're blending between outgoing and incoming animations
            let blend_factor = if blend_state.total_blend_time > 0.0 {
                1.0 - (blend_state.remaining_blend_time / blend_state.total_blend_time)
            } else {
                1.0 // No blend time, use incoming animation fully
            };

            // Get transforms from outgoing animation
            let outgoing_skeleton = ss2_skeleton::animate(
                skeleton,
                Some(AnimationInfo {
                    animation_clip: blend_state.outgoing_clip.as_ref(),
                    frame: blend_state.outgoing_frame,
                }),
                &self.additional_joint_transforms,
            );
            let mut outgoing_transforms = outgoing_skeleton.get_transforms();

            // Apply outgoing translation
            for i in 0..40 {
                outgoing_transforms[i] = Matrix4::from_translation(
                    (blend_state.outgoing_frame as f32 / blend_state.outgoing_clip.num_frames as f32)
                        * vec3(0.0, blend_state.outgoing_clip.translation.y, 0.0),
                ) * outgoing_transforms[i];
            }

            // Get transforms from incoming animation
            let incoming_skeleton = ss2_skeleton::animate(
                skeleton,
                Some(AnimationInfo {
                    animation_clip: current_clip,
                    frame: current_frame,
                }),
                &self.additional_joint_transforms,
            );
            let mut incoming_transforms = incoming_skeleton.get_transforms();

            // Apply incoming translation
            for i in 0..40 {
                incoming_transforms[i] = Matrix4::from_translation(
                    (current_frame as f32 / current_clip.num_frames as f32)
                        * vec3(0.0, current_clip.translation.y, 0.0),
                ) * incoming_transforms[i];
            }

            // Blend the transforms
            let mut blended_transforms = [Matrix4::from_scale(1.0); 40];
            for i in 0..40 {
                // Simple linear interpolation of the matrices
                // For better results, you might want to decompose into translation/rotation/scale
                // and interpolate each component separately, but this works for basic blending
                blended_transforms[i] = outgoing_transforms[i] * (1.0 - blend_factor)
                    + incoming_transforms[i] * blend_factor;
            }

            blended_transforms
        } else {
            // No blending, use standard single animation path
            let animated_skeleton = ss2_skeleton::animate(
                skeleton,
                Some(AnimationInfo {
                    animation_clip: current_clip,
                    frame: current_frame,
                }),
                &self.additional_joint_transforms,
            );

            let mut animated_transforms = animated_skeleton.get_transforms();

            // Apply any global translation changes in the current clip as well
            for i in 0..40 {
                animated_transforms[i] = Matrix4::from_translation(
                    (current_frame as f32 / current_clip.num_frames as f32)
                        * vec3(0.0, current_clip.translation.y, 0.0),
                ) * animated_transforms[i];
            }

            animated_transforms
        }
    }
}
