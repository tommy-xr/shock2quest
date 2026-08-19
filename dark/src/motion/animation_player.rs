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
    /// Whether the interrupted clip loops - a fade that outlives the clip
    /// wraps a looping from-pose but holds a one-shot on its last frame
    from_looping: bool,
    duration: f32,
    elapsed: f32,
}

/// Read-only snapshot of an `AnimationPlayer`'s playback state, for debug
/// introspection (the debug runtime's animation endpoint).
pub struct AnimationPlayerSnapshot {
    /// Queued clips, head (currently playing) first.
    pub queue: Vec<AnimationQueueEntry>,
    /// Clip whose final frame poses the skeleton once the queue drains.
    pub last_clip: Option<String>,
    /// Current frame within the head clip.
    pub current_frame: u32,
    /// Sub-frame time carried toward the next frame advance (seconds).
    pub remaining_time: f32,
    pub blend: Option<AnimationBlendSnapshot>,
}

pub struct AnimationQueueEntry {
    pub name: Option<String>,
    pub num_frames: u32,
    pub looping: bool,
}

pub struct AnimationBlendSnapshot {
    pub from_clip: Option<String>,
    pub from_frame: f32,
    pub duration: f32,
    pub elapsed: f32,
}

#[derive(Clone)]
pub struct AnimationPlayer {
    animation: immutable::List<(Rc<AnimationClip>, AnimationFlags)>,
    additional_joint_transforms: immutable::HashTrieMap<u32, Matrix4<f32>>,
    last_animation: Option<Rc<AnimationClip>>,
    current_frame: u32,
    remaining_time: f32,
    blend_state: Option<BlendState>,
    /// Playback position (frame units) through which the head clip's
    /// end_rotation has been emitted - the anchor for the per-tick rotation
    /// ramp. Reset to 0 whenever a new clip becomes the head, so the ramp
    /// covers a carried seam remainder from position zero.
    rotation_pos: f32,
    /// Pose with the clip's root motion cancelled (see
    /// `AnimationInfo::cancel_root_motion`). Set for first-person viewmodels,
    /// whose entity transform is re-anchored to the camera every frame.
    cancel_root_motion: bool,
    /// Rigid model-space transform composed onto every posed joint after
    /// animation (`get_transforms` returns `post * joint`). Used by the VR
    /// melee wield to align the posed arm's fist and weapon axis with the
    /// entity's grip frame without moving the entity (and its contact
    /// collider). `None` for every ordinary player.
    post_transform: Option<Matrix4<f32>>,
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
            rotation_pos: 0.0,
            cancel_root_motion: false,
            post_transform: None,
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
            rotation_pos: 0.0,
            cancel_root_motion: false,
            post_transform: None,
        }
    }

    /// Hold `animation_clip` on its final frame without ever playing it.
    ///
    /// This is the same state a one-shot reaches after its queue drains, but
    /// constructed directly for restored static poses. Because the playback
    /// queue is empty, [`Self::update`] emits no motion flags, completion or
    /// direction events, and no root velocity.
    pub fn from_completed_animation(animation_clip: Rc<AnimationClip>) -> AnimationPlayer {
        AnimationPlayer {
            animation: immutable::List::new(),
            additional_joint_transforms: immutable::HashTrieMap::new(),
            last_animation: Some(animation_clip),
            current_frame: 0,
            remaining_time: 0.0,
            blend_state: None,
            rotation_pos: 0.0,
            cancel_root_motion: false,
            post_transform: None,
        }
    }

    pub fn queue_animation(
        player: &AnimationPlayer,
        animation: Rc<AnimationClip>,
    ) -> AnimationPlayer {
        let new_animation = player
            .animation
            .push_front((animation.clone(), AnimationFlags::PlayOnce));

        // Fade from the playing clip, or - when the queue already drained (the
        // normal case for AI clips, whose completion handler queues the next
        // clip one tick after the previous one finished) - from the frozen
        // last-frame pose that get_transforms has been showing. Without the
        // fallback a clip's authored blend_length only ever applied to
        // interruptions, so e.g. idle cycling (500ms authored) hard-popped.
        let duration = animation.blend_length.as_secs_f32();
        let blend_state = if duration > f32::EPSILON {
            player
                .blend_from()
                .map(|(from_clip, from_frame, from_looping)| BlendState {
                    from_clip,
                    from_frame,
                    from_looping,
                    duration,
                    elapsed: 0.0,
                })
        } else {
            None
        };

        AnimationPlayer {
            additional_joint_transforms: player.additional_joint_transforms.clone(),
            animation: new_animation,
            last_animation: None,
            current_frame: 0,
            // Preserve the sub-frame playback remainder (including the
            // overshoot carried across the previous clip's completion) so a
            // queued continuation keeps the clip cadence instead of
            // restarting the frame clock at every seam.
            remaining_time: player.remaining_time,
            blend_state,
            rotation_pos: 0.0,
            cancel_root_motion: player.cancel_root_motion,
            post_transform: player.post_transform,
        }
    }

    /// The pose a new clip should cross-fade from: the playing queue head at
    /// its current frame, or - when the queue already drained - the frozen
    /// final frame of `last_animation` (what `get_transforms` is showing).
    fn blend_from(&self) -> Option<(Rc<AnimationClip>, f32, bool)> {
        self.animation
            .first()
            .map(|(clip, flags)| {
                // Fade from the pose actually on screen - the whole frame
                // plus the sub-frame remainder - not the integer keyframe
                // behind it (which would start the fade with a backward pop).
                let time_per_frame = clip.time_per_frame.as_secs_f32();
                let fraction = if time_per_frame > f32::EPSILON {
                    (self.remaining_time / time_per_frame).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                (
                    clip.clone(),
                    self.current_frame as f32 + fraction,
                    matches!(flags, AnimationFlags::Loop),
                )
            })
            .or_else(|| {
                self.last_animation.as_ref().map(|clip| {
                    (
                        clip.clone(),
                        clip.num_frames.saturating_sub(1) as f32,
                        false,
                    )
                })
            })
    }

    /// Play `animation` immediately, replacing the whole queue (unlike
    /// `queue_animation`, which pushes on top and lets interrupted clips
    /// resume later). Cross-fades from the interrupted pose over the clip's
    /// authored blend length, floored so a zero-blend clip doesn't pop when
    /// it cuts a clip mid-play.
    pub fn play_animation(
        player: &AnimationPlayer,
        animation: Rc<AnimationClip>,
    ) -> AnimationPlayer {
        const MIN_INTERRUPT_BLEND_SECS: f32 = 0.15;

        // Known limit: an interrupt landing mid-blend fades from the head
        // clip's pure pose, not the blended one on screen - a small pop
        // proportional to how fresh the interrupted blend was.
        let blend_state = player
            .blend_from()
            .map(|(from_clip, from_frame, from_looping)| BlendState {
                from_clip,
                from_frame,
                from_looping,
                duration: animation
                    .blend_length
                    .as_secs_f32()
                    .max(MIN_INTERRUPT_BLEND_SECS),
                elapsed: 0.0,
            });

        AnimationPlayer {
            additional_joint_transforms: player.additional_joint_transforms.clone(),
            animation: immutable::List::new().push_front((animation, AnimationFlags::PlayOnce)),
            last_animation: None,
            current_frame: 0,
            remaining_time: 0.0,
            blend_state,
            rotation_pos: 0.0,
            cancel_root_motion: player.cancel_root_motion,
            post_transform: player.post_transform,
        }
    }

    /// A copy of `player` that poses with the clip's root motion cancelled
    /// (see `AnimationInfo::cancel_root_motion`).
    pub fn with_root_motion_cancelled(player: &AnimationPlayer) -> AnimationPlayer {
        let mut new_player = player.clone();
        new_player.cancel_root_motion = true;
        new_player
    }

    /// A copy of `player` whose posed joints are all pre-multiplied by
    /// `transform` (see the `post_transform` field).
    pub fn with_post_transform(
        player: &AnimationPlayer,
        transform: Matrix4<f32>,
    ) -> AnimationPlayer {
        let mut new_player = player.clone();
        new_player.post_transform = Some(transform);
        new_player
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
            rotation_pos: player.rotation_pos,
            cancel_root_motion: player.cancel_root_motion,
            post_transform: player.post_transform,
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
                    frame = if blend.from_looping {
                        frame % frame_count
                    } else {
                        frame_count - 1.0
                    };
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
            // Move at the rate the mocap root actually moves this frame -
            // the clip-average (`sliding_velocity`) integrates to the same
            // endpoint but smears non-uniform motion (a death keeps gliding
            // at constant speed while the body is already down). The value is
            // a rate (units/sec) keyed to the clip's own frame time, so it is
            // independent of the render/physics refresh. Sampled from the
            // frame at the start of the tick: whenever the tick is shorter
            // than a clip frame (true at 60/90/120Hz against 30fps clips) it
            // crosses at most one frame, so multi-frame advance only happens
            // on a hitch and briefly commanding the first frame's rate isn't
            // worth averaging over.
            let velocity = current_clip
                .root_velocity_at(player.current_frame)
                .unwrap_or(current_clip.sliding_velocity);
            let mut next_frame = player.current_frame;
            let time_per_frame = current_clip.time_per_frame.as_secs_f32();
            // The ramp anchor: everything up to rotation_pos has already been
            // emitted. Anchoring on an explicit field (not a position derived
            // from current_frame) lets a fresh clip's ramp start at zero even
            // when it inherits a carried seam remainder.
            let prev_pos = player.rotation_pos;
            while remaining_duration >= time_per_frame {
                remaining_duration -= time_per_frame;
                next_frame += 1;
            }
            // A clip's authored end direction turns the entity ACROSS the
            // clip, proportionally to the playback traversed each tick, not
            // as a snap on the final frame (a hard yaw pop on every turn or
            // gesture clip). The per-tick fractions telescope, so a full
            // playthrough applies exactly the authored rotation.
            let direction_delta = |end_pos: f32, events: &mut Vec<AnimationEvent>| {
                if current_clip.end_rotation != Deg(0.0) && current_clip.num_frames > 0 {
                    let fraction = (end_pos - prev_pos).max(0.0) / current_clip.num_frames as f32;
                    if fraction > 0.0 {
                        events.push(AnimationEvent::DirectionChanged(Deg(current_clip
                            .end_rotation
                            .0
                            * fraction)));
                    }
                }
            };
            // Raw end-of-tick position, uncapped: past num_frames it covers
            // the wrapped region of a looping clip (a hitch may cover more
            // than one full cycle - the fraction then exceeds 1 and emits
            // the extra cycles' rotation too).
            let raw_end_pos = if time_per_frame > f32::EPSILON {
                next_frame as f32 + remaining_duration / time_per_frame
            } else {
                next_frame as f32
            };

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

                match flags {
                    AnimationFlags::Loop => {
                        // The wrapped region was traversed this tick too -
                        // emit past the cap so no rotation slice is lost,
                        // then re-anchor at the post-wrap position.
                        direction_delta(raw_end_pos, &mut events);
                        (
                            AnimationPlayer {
                                additional_joint_transforms: player
                                    .additional_joint_transforms
                                    .clone(),
                                last_animation: player.last_animation.clone(),
                                animation: player.animation.clone(),
                                current_frame: next_frame - current_clip.num_frames,
                                remaining_time: remaining_duration,
                                blend_state,
                                rotation_pos: raw_end_pos - current_clip.num_frames as f32,
                                cancel_root_motion: player.cancel_root_motion,
                                post_transform: player.post_transform,
                            },
                            motion_flags,
                            events,
                            velocity,
                        )
                    }
                    AnimationFlags::PlayOnce => {
                        // Final ramp segment: up to the clip's exact end
                        // (hitch overshoot doesn't over-rotate, matching the
                        // sub-frame carry below).
                        direction_delta(current_clip.num_frames as f32, &mut events);
                        let last_animation = player.animation.first().map(|m| m.0.clone());
                        let animation = player.animation.drop_first().unwrap_or_default();
                        // Carry the sub-frame remainder past the final frame
                        // into the next clip - zeroing it phase-reset the
                        // cadence at every seam. Whole overshot frames (a
                        // large dt hitch landing on a completion) are
                        // deliberately dropped: carrying more than one frame
                        // would leave the new clip's logical time ahead of
                        // its rendered pose. The advance loop already leaves
                        // remaining_duration < time_per_frame.
                        let overshoot = remaining_duration;
                        (
                            AnimationPlayer {
                                additional_joint_transforms: player
                                    .additional_joint_transforms
                                    .clone(),
                                animation,
                                last_animation,
                                current_frame: 0,
                                remaining_time: overshoot,
                                blend_state,
                                // Fresh anchor: the next head clip's ramp
                                // starts at zero, so the carried remainder's
                                // slice is emitted on its first tick.
                                rotation_pos: 0.0,
                                cancel_root_motion: player.cancel_root_motion,
                                post_transform: player.post_transform,
                            },
                            motion_flags,
                            events,
                            velocity,
                        )
                    }
                }
            } else {
                let mut events = if !player.animation.is_empty()
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
                direction_delta(raw_end_pos, &mut events);
                (
                    AnimationPlayer {
                        additional_joint_transforms: player.additional_joint_transforms.clone(),
                        last_animation: player.last_animation.clone(),
                        animation: player.animation.clone(),
                        current_frame: next_frame,
                        remaining_time: remaining_duration,
                        blend_state,
                        rotation_pos: raw_end_pos,
                        cancel_root_motion: player.cancel_root_motion,
                        post_transform: player.post_transform,
                    },
                    motion_flags,
                    events,
                    velocity,
                )
            }
        }
    }

    pub fn snapshot(&self) -> AnimationPlayerSnapshot {
        AnimationPlayerSnapshot {
            queue: self
                .animation
                .iter()
                .map(|(clip, flags)| AnimationQueueEntry {
                    name: clip.name.clone(),
                    num_frames: clip.num_frames,
                    looping: matches!(flags, AnimationFlags::Loop),
                })
                .collect(),
            last_clip: self
                .last_animation
                .as_ref()
                .and_then(|clip| clip.name.clone()),
            current_frame: self.current_frame,
            remaining_time: self.remaining_time,
            blend: self
                .blend_state
                .as_ref()
                .map(|blend| AnimationBlendSnapshot {
                    from_clip: blend.from_clip.name.clone(),
                    from_frame: blend.from_frame,
                    duration: blend.duration,
                    elapsed: blend.elapsed,
                }),
        }
    }

    pub fn is_queue_empty(&self) -> bool {
        self.animation.is_empty()
    }

    pub fn get_transforms(&self, skeleton: &Skeleton) -> [Matrix4<f32>; 40] {
        // We need to clarify if this animation is the current run, or a carry over from the previous one,
        // so add a separate boolean flag `is_last_anim` if we fallback to the last_animation.
        let maybe_current_clip = self
            .animation
            .first()
            .map(|m| (m.0.clone(), matches!(m.1, AnimationFlags::Loop), false))
            .or_else(|| self.last_animation.clone().map(|m| (m, false, true)));

        // If there is no animation, we still may need to apply joint transforms (ie, for camera or turret)
        if maybe_current_clip.is_none() {
            let animated_skeleton =
                ss2_skeleton::animate(skeleton, None, &self.additional_joint_transforms);
            return self.apply_post_transform(animated_skeleton.get_transforms());
        }

        let (rc_animation_clip, is_looping, is_last_anim) = maybe_current_clip.unwrap();
        let current_clip = rc_animation_clip.as_ref();

        // Sub-frame position: the whole frame plus the accumulated remainder
        // toward the next one, so 60Hz+ playback of 30fps clips interpolates
        // between keyframes instead of holding each for two ticks. A drained
        // queue holds the final keyframe exactly.
        let current_frame = if is_last_anim {
            (current_clip.num_frames - 1) as f32
        } else {
            let time_per_frame = current_clip.time_per_frame.as_secs_f32();
            let fraction = if time_per_frame > f32::EPSILON {
                (self.remaining_time / time_per_frame).clamp(0.0, 1.0)
            } else {
                0.0
            };
            self.current_frame as f32 + fraction
        };

        let mut animated_transforms = Self::compute_transforms_for_clip(
            skeleton,
            current_clip,
            current_frame,
            is_looping,
            &self.additional_joint_transforms,
            self.cancel_root_motion,
        );

        if let Some(blend) = &self.blend_state {
            if blend.duration > f32::EPSILON && blend.elapsed < blend.duration {
                // Raised-cosine ease-in/ease-out rather than linear - a linear
                // ramp starts and stops the correction abruptly, which reads
                // as two small hitches bracketing the fade.
                let t = (blend.elapsed / blend.duration).clamp(0.0, 1.0);
                let alpha = (1.0 - (std::f32::consts::PI * t).cos()) / 2.0;
                // update() keeps from_frame in range (loops wrap, one-shots
                // clamp), and the fractional position advances smoothly
                // during the fade.
                let frame = blend.from_frame;

                let from_transforms = Self::compute_transforms_for_clip(
                    skeleton,
                    &blend.from_clip,
                    frame,
                    blend.from_looping,
                    &self.additional_joint_transforms,
                    self.cancel_root_motion,
                );

                animated_transforms =
                    Self::blend_transforms(&from_transforms, &animated_transforms, alpha);
            }
        }

        self.apply_post_transform(animated_transforms)
    }

    fn apply_post_transform(&self, mut transforms: [Matrix4<f32>; 40]) -> [Matrix4<f32>; 40] {
        if let Some(post) = self.post_transform {
            for transform in transforms.iter_mut() {
                *transform = post * *transform;
            }
        }
        transforms
    }

    fn compute_transforms_for_clip(
        skeleton: &Skeleton,
        clip: &AnimationClip,
        frame: f32,
        wrap: bool,
        additional_joint_transforms: &immutable::HashTrieMap<u32, Matrix4<f32>>,
        cancel_root_motion: bool,
    ) -> [Matrix4<f32>; 40] {
        let animated_skeleton = ss2_skeleton::animate(
            skeleton,
            Some(AnimationInfo {
                animation_clip: clip,
                frame: frame as u32,
                fraction: frame.fract(),
                wrap,
                cancel_root_motion,
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

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    /// 3-frame clip at 10fps whose root moves 1 unit in x during frame 0->1
    /// and then rests: per-frame velocity is (10,0,0) then zero, while the
    /// clip-average (`sliding_velocity`) smears it to ~(3.33,0,0).
    fn clip_with_root_motion() -> Rc<AnimationClip> {
        let time_per_frame = Duration::from_millis(100);
        Rc::new(AnimationClip {
            num_frames: 3,
            time_per_frame,
            duration: time_per_frame * 3,
            blend_length: Duration::ZERO,
            end_rotation: Deg(0.0),
            sliding_velocity: vec3(1.0, 0.0, 0.0) / 0.3,
            translation: vec3(1.0, 0.0, 0.0),
            joint_to_frame: HashMap::new(),
            root_transforms: Vec::new(),
            root_positions: vec![
                vec3(0.0, 0.0, 0.0),
                vec3(1.0, 0.0, 0.0),
                vec3(1.0, 0.0, 0.0),
            ],
            motion_flags: Vec::new(),
            name: None,
        })
    }

    #[test]
    fn update_returns_per_frame_root_velocity_not_clip_average() {
        let player =
            AnimationPlayer::queue_animation(&AnimationPlayer::empty(), clip_with_root_motion());

        // Frame 0 -> 1: the root moves a full unit in one 100ms frame
        let (player, _, _, velocity) = AnimationPlayer::update(&player, Duration::from_millis(100));
        assert!(
            (velocity.x - 10.0).abs() < 1e-4,
            "expected the frame's own rate (10.0), got {:?}",
            velocity
        );

        // Frame 1 -> 2: the root rests, so the entity must stop
        let (_, _, _, velocity) = AnimationPlayer::update(&player, Duration::from_millis(100));
        assert!(
            velocity.x.abs() < 1e-4,
            "expected zero while the root rests, got {:?}",
            velocity
        );
    }

    #[test]
    fn root_velocity_uses_trailing_delta_on_the_final_frame() {
        // Monotonic stride: each frame advances 1 unit. A looping clip lands
        // on the final frame once per cycle; using the forward segment there
        // would read positions[last]-positions[last] = 0 and stall the walk,
        // so the final frame must fall back to the trailing segment's rate.
        let mut clip = (*clip_with_root_motion()).clone();
        clip.root_positions = vec![
            vec3(0.0, 0.0, 0.0),
            vec3(1.0, 0.0, 0.0),
            vec3(2.0, 0.0, 0.0),
        ];

        assert!((clip.root_velocity_at(0).unwrap().x - 10.0).abs() < 1e-4);
        assert!((clip.root_velocity_at(1).unwrap().x - 10.0).abs() < 1e-4);
        assert!(
            (clip.root_velocity_at(2).unwrap().x - 10.0).abs() < 1e-4,
            "final frame should keep the stride rate, not drop to zero: {:?}",
            clip.root_velocity_at(2)
        );
    }

    #[test]
    fn end_rotation_ramps_across_playback_and_totals_exactly() {
        let mut clip = (*clip_with_root_motion()).clone();
        clip.end_rotation = Deg(90.0);
        let mut player = AnimationPlayer::queue_animation(&AnimationPlayer::empty(), Rc::new(clip));

        // 3 frames at 100ms, stepped in 60ms ticks: the turn must arrive as
        // several increments summing to exactly the authored 90 degrees.
        let mut total = 0.0;
        let mut rotation_events = 0;
        for _ in 0..100 {
            let (next, _, events, _) = AnimationPlayer::update(&player, Duration::from_millis(60));
            player = next;
            let mut completed = false;
            for event in &events {
                match event {
                    AnimationEvent::DirectionChanged(d) => {
                        total += d.0;
                        rotation_events += 1;
                    }
                    AnimationEvent::Completed => completed = true,
                    _ => {}
                }
            }
            if completed {
                break;
            }
        }
        assert!(
            rotation_events > 1,
            "rotation must ramp across ticks, not snap once (got {rotation_events} events)"
        );
        assert!(
            (total - 90.0).abs() < 1e-3,
            "increments must total the authored rotation, got {total}"
        );
    }

    #[test]
    fn end_rotation_covers_the_loop_wrap_segment() {
        // Looping 3-frame clip (100ms/frame), 90 deg per cycle, stepped in
        // 80ms ticks (never divides 300ms, so every wrap carries a sub-frame
        // remainder into the next cycle). After exactly 4 cycles' worth of
        // time (1.2s = 15 ticks), the emitted total must be 4 * 90 with no
        // slice lost at the wraps.
        let mut clip = (*clip_with_root_motion()).clone();
        clip.end_rotation = Deg(90.0);
        let mut player = AnimationPlayer::from_animation(Rc::new(clip));
        let mut total = 0.0;
        for _ in 0..15 {
            let (next, _, events, _) = AnimationPlayer::update(&player, Duration::from_millis(80));
            player = next;
            for event in &events {
                if let AnimationEvent::DirectionChanged(d) = event {
                    total += d.0;
                }
            }
        }
        assert!(
            (total - 360.0).abs() < 1e-2,
            "4 loop cycles must emit exactly 4x the authored rotation, got {total}"
        );
    }

    #[test]
    fn end_rotation_totals_exactly_across_a_carried_seam() {
        // Clip 1 completes with a sub-frame carry; clip 2 (also rotating)
        // inherits it. Each clip's emissions must independently total the
        // authored rotation - the carried slice belongs to clip 2's ramp.
        let mut clip = (*clip_with_root_motion()).clone();
        clip.end_rotation = Deg(90.0);
        let clip = Rc::new(clip);
        let mut player = AnimationPlayer::queue_animation(&AnimationPlayer::empty(), clip.clone());

        // 350ms tick: clip 1 (300ms) completes with 50ms carried.
        let (next, _, events, _) = AnimationPlayer::update(&player, Duration::from_millis(350));
        player = next;
        let mut clip1_total = 0.0;
        for event in &events {
            if let AnimationEvent::DirectionChanged(d) = event {
                clip1_total += d.0;
            }
        }
        assert!(
            (clip1_total - 90.0).abs() < 1e-3,
            "clip 1 must total its authored rotation, got {clip1_total}"
        );

        // Queue clip 2 (inherits the 50ms carry) and run it to completion.
        player = AnimationPlayer::queue_animation(&player, clip.clone());
        let mut clip2_total = 0.0;
        for _ in 0..100 {
            let (next, _, events, _) = AnimationPlayer::update(&player, Duration::from_millis(60));
            player = next;
            let mut completed = false;
            for event in &events {
                match event {
                    AnimationEvent::DirectionChanged(d) => clip2_total += d.0,
                    AnimationEvent::Completed => completed = true,
                    _ => {}
                }
            }
            if completed {
                break;
            }
        }
        assert!(
            (clip2_total - 90.0).abs() < 1e-3,
            "clip 2 must total its authored rotation including the carried slice, got {clip2_total}"
        );
    }

    #[test]
    fn completion_carries_overshoot_and_queueing_preserves_it() {
        // 3-frame clip at 10fps (0.3s). A 0.35s tick completes it with 0.05s
        // of overshoot past the final frame.
        let player =
            AnimationPlayer::queue_animation(&AnimationPlayer::empty(), clip_with_root_motion());
        let (player, _, events, _) = AnimationPlayer::update(&player, Duration::from_millis(350));
        assert!(
            events
                .iter()
                .any(|e| matches!(e, AnimationEvent::Completed))
        );
        let carried = player.snapshot().remaining_time;
        assert!(
            (carried - 0.05).abs() < 1e-4,
            "overshoot must carry across completion (got {carried})"
        );

        // Queueing the continuation keeps the remainder...
        let player = AnimationPlayer::queue_animation(&player, clip_with_root_motion());
        assert!((player.snapshot().remaining_time - 0.05).abs() < 1e-4);

        // ...so the frame clock stays on cadence: 0.05 carried + 0.06 tick
        // crosses the 0.1s frame boundary exactly one frame in.
        let (player, _, _, _) = AnimationPlayer::update(&player, Duration::from_millis(60));
        assert_eq!(player.snapshot().current_frame, 1);
    }

    #[test]
    fn blend_starts_from_the_fractional_displayed_pose() {
        // Advance half a frame (tpf 100ms, dt 50ms): the screen shows frame
        // 0.5. A blend must fade from that pose, not integer frame 0.
        let player =
            AnimationPlayer::queue_animation(&AnimationPlayer::empty(), clip_with_root_motion());
        let (player, _, _, _) = AnimationPlayer::update(&player, Duration::from_millis(50));
        let mut blending_clip = (*clip_with_root_motion()).clone();
        blending_clip.blend_length = Duration::from_millis(500);
        let player = AnimationPlayer::queue_animation(&player, Rc::new(blending_clip));
        let blend = player.snapshot().blend.expect("blend engages");
        assert!(
            (blend.from_frame - 0.5).abs() < 1e-4,
            "from-pose must be the displayed fractional frame, got {}",
            blend.from_frame
        );
    }

    #[test]
    fn queued_clip_blends_from_drained_queues_last_pose() {
        // Complete a clip so the queue drains and last_animation holds the
        // final pose (the normal state when the AI's completion handler
        // queues the next clip one tick later).
        let player =
            AnimationPlayer::queue_animation(&AnimationPlayer::empty(), clip_with_root_motion());
        let (player, _, _, _) = AnimationPlayer::update(&player, Duration::from_millis(300));
        assert!(player.snapshot().queue.is_empty());
        assert!(player.snapshot().last_clip.is_none()); // clip is unnamed
        assert!(player.last_animation.is_some());

        // A clip with an authored blend must fade from that last pose...
        let mut blending_clip = (*clip_with_root_motion()).clone();
        blending_clip.blend_length = Duration::from_millis(500);
        let player = AnimationPlayer::queue_animation(&player, Rc::new(blending_clip));
        let blend = player.snapshot().blend.expect(
            "authored blend_length must engage from the drained queue's last pose, not only on interruptions",
        );
        assert!((blend.duration - 0.5).abs() < 1e-6);
        // ...from the final frame of the completed clip.
        assert!((blend.from_frame - 2.0).abs() < 1e-6);
    }

    #[test]
    fn zero_blend_clip_still_hard_cuts() {
        // Stride clips author blend_length 0 (the pose lines up by design);
        // they must not acquire a synthetic fade.
        let player =
            AnimationPlayer::queue_animation(&AnimationPlayer::empty(), clip_with_root_motion());
        let (player, _, _, _) = AnimationPlayer::update(&player, Duration::from_millis(300));
        let player = AnimationPlayer::queue_animation(&player, clip_with_root_motion());
        assert!(player.snapshot().blend.is_none());
    }

    #[test]
    fn completed_animation_constructor_holds_without_events_or_motion() {
        let mut clip = (*clip_with_root_motion()).clone();
        clip.name = Some("death_pose".to_owned());
        let player = AnimationPlayer::from_completed_animation(Rc::new(clip));

        let snapshot = player.snapshot();
        assert!(snapshot.queue.is_empty());
        assert_eq!(snapshot.last_clip.as_deref(), Some("death_pose"));

        for duration in [
            Duration::from_millis(16),
            Duration::from_secs(1),
            Duration::from_secs(30),
        ] {
            let (next, flags, events, velocity) = AnimationPlayer::update(&player, duration);
            assert!(flags.is_empty());
            assert!(events.is_empty());
            assert_eq!(velocity, vec3(0.0, 0.0, 0.0));
            assert!(next.snapshot().queue.is_empty());
            assert_eq!(next.snapshot().last_clip.as_deref(), Some("death_pose"));
        }
    }

    #[test]
    fn update_falls_back_to_sliding_velocity_without_root_stream() {
        let mut clip = (*clip_with_root_motion()).clone();
        clip.root_positions = Vec::new();
        let player = AnimationPlayer::queue_animation(&AnimationPlayer::empty(), Rc::new(clip));

        let (_, _, _, velocity) = AnimationPlayer::update(&player, Duration::from_millis(100));
        assert!(
            (velocity.x - 1.0 / 0.3).abs() < 1e-4,
            "expected the clip-average fallback, got {:?}",
            velocity
        );
    }
}
