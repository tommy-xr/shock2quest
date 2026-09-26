//! Ownership of a pivot's turn clip, from the request to the last frame of
//! the fade its yaw hands over across (`Effect::PlayTurnClip`).
//!
//! The applier owns the animation player, so it - not a clock in the script -
//! is what knows whose clip is playing and how far the fade that follows it
//! has got. Scripts and the animation player tick at different points in the
//! frame, so a script integrating its own copy of the fade runs a frame ahead
//! of the pose; every report below is read off the player itself.

use std::collections::HashMap;

use cgmath::Deg;
use dark::motion::AnimationPlayer;
use shipyard::EntityId;

use crate::scripts::MessagePayload;

/// How far a tracked turn clip has got.
enum TurnClipPhase {
    /// Still playing: any other animation applied to the entity preempts it.
    Playing,
    /// Ran to its end. The next clip applied is the transition its pose gives
    /// the turn up across, not a preemption.
    Completed,
    /// That transition is running and the pivot's yaw is riding it. A clip
    /// applied now (a wound reaction, a behavior swap) replaces the fade, so
    /// the rest of the turn is dropped rather than eased against a curve
    /// nothing is drawing any more.
    HandingOff,
}

struct TurnClipPlayback {
    /// The `Effect::PlayTurnClip` request this playback answers.
    token: u64,
    phase: TurnClipPhase,
}

/// The turn clip in flight per entity. Each method reports what the requester
/// must be told, if anything; the caller dispatches it.
pub struct TurnClipTracker {
    playbacks: HashMap<EntityId, TurnClipPlayback>,
}

impl TurnClipTracker {
    pub fn new() -> TurnClipTracker {
        TurnClipTracker {
            playbacks: HashMap::new(),
        }
    }

    /// An animation was applied to `entity_id`: `applied` says whether the
    /// clip that started cross-fades in (`Some(true)`) or hard-cuts
    /// (`Some(false)`), and is `None` when the request resolved to no clip at
    /// all. `answers_pivot` says whether the apply came from a `PlayTurnClip` -
    /// one that resolved to nothing still displaces whatever was tracked, so
    /// the entry is taken either way.
    pub fn on_animation_applied(
        &mut self,
        entity_id: EntityId,
        applied: Option<bool>,
        answers_pivot: bool,
    ) -> Option<MessagePayload> {
        let previous = if applied.is_some() || answers_pivot {
            self.playbacks.remove(&entity_id)?
        } else {
            return None;
        };
        let token = previous.token;
        match (applied, previous.phase) {
            (Some(fades), TurnClipPhase::Completed) => {
                if fades {
                    self.playbacks.insert(
                        entity_id,
                        TurnClipPlayback {
                            token,
                            phase: TurnClipPhase::HandingOff,
                        },
                    );
                }
                Some(MessagePayload::TurnClipHandoff { token, fades })
            }
            // Preempted while playing, displaced mid-fade, or orphaned by an
            // apply that started nothing: no pose is performing the turn any
            // more, so its authored facing change must not be taken.
            _ => Some(MessagePayload::TurnClipCancelled { token }),
        }
    }

    /// A `PlayTurnClip` resolved: `started` carries the facing change the clip
    /// the applier picked is authored to end on, or `None` when nothing was
    /// affordable. The requester is answered either way, so it never waits on
    /// a clip that will not come.
    pub fn on_turn_resolved(
        &mut self,
        entity_id: EntityId,
        token: u64,
        started: Option<Deg<f32>>,
    ) -> MessagePayload {
        match started {
            Some(turn) => {
                self.playbacks.insert(
                    entity_id,
                    TurnClipPlayback {
                        token,
                        phase: TurnClipPhase::Playing,
                    },
                );
                MessagePayload::TurnClipStarted { token, turn }
            }
            None => MessagePayload::TurnClipCancelled { token },
        }
    }

    /// An animation on `entity_id` reached its end. Only the pivot's own clip
    /// is reported - the clips that follow it complete too, and one of those
    /// completions used to be able to restart the handover.
    pub fn on_clip_completed(&mut self, entity_id: EntityId) -> Option<MessagePayload> {
        let playback = self.playbacks.get_mut(&entity_id)?;
        match playback.phase {
            TurnClipPhase::Playing => {
                playback.phase = TurnClipPhase::Completed;
                Some(MessagePayload::TurnClipCompleted {
                    token: playback.token,
                })
            }
            _ => None,
        }
    }

    /// One frame of the fade the pivot's yaw hands over across, read off the
    /// player after it has been advanced. The last report of a fade is 1.0,
    /// and ends the tracking.
    pub fn on_blend_tick(
        &mut self,
        entity_id: EntityId,
        player: &AnimationPlayer,
    ) -> Option<MessagePayload> {
        let playback = self.playbacks.get(&entity_id)?;
        if !matches!(playback.phase, TurnClipPhase::HandingOff) {
            return None;
        }
        let token = playback.token;
        let alpha = player.blend_alpha_now();
        if alpha >= 1.0 {
            self.playbacks.remove(&entity_id);
        }
        Some(MessagePayload::TurnClipBlend { token, alpha })
    }

    /// The entity's animation player is being replaced or taken away, so no
    /// report can ever arrive for a turn clip it was playing.
    pub fn abandon(&mut self, entity_id: EntityId) -> Option<MessagePayload> {
        let playback = self.playbacks.remove(&entity_id)?;
        Some(MessagePayload::TurnClipCancelled {
            token: playback.token,
        })
    }

    /// The entity is gone; there is nobody left to report to.
    pub fn forget(&mut self, entity_id: EntityId) {
        self.playbacks.remove(&entity_id);
    }
}
