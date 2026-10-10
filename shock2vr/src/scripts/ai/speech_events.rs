//! Mobile-AI voice requests driven by perception, committed health, and combat
//! contacts. Playback still resolves the actor's authored voice and priorities.
use std::collections::{HashMap, VecDeque};

use dark::properties::AIAlertLevel;
use serde::{Deserialize, Serialize};
use shipyard::{EntityId, World};

use crate::scripts::{Effect, speech_util};

#[derive(Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum SpeechTactic {
    #[default]
    None,
    Charge,
    Shoot,
}

/// The AI supplies its current private state, before its ECS effects land.
#[derive(Clone, Copy)]
pub struct SpeechContext {
    pub level: AIAlertLevel,
    pub visible: bool,
    pub player_target: bool,
    pub active: bool,
    pub tactic: SpeechTactic,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub(crate) enum Cue {
    ToOne,
    ToTwo,
    ToThree,
    SpotPlayer,
    BackToZero,
    LostContact,
    HitHigh,
    HitLow,
    HitAmbush,
    HitNoDamage,
    SuccessfulHit,
    Blocked,
    Charge,
    Shoot,
}

#[cfg(test)]
mod tests {
    use super::*;
    use dark::properties::{PropAIBroadcastDisabled, PropVoiceIndex};

    fn fixture() -> (World, EntityId, SpeechEvents, SpeechContext) {
        let mut world = World::new();
        let entity = world.add_entity((PropVoiceIndex(2),));
        let context = SpeechContext {
            level: AIAlertLevel::Lowest,
            visible: false,
            player_target: true,
            active: true,
            tactic: SpeechTactic::None,
        };
        let mut speech = SpeechEvents::default();
        speech.synchronize(Some(context));
        (world, entity, speech, context)
    }

    fn concept(effect: Effect) -> Option<String> {
        match effect {
            Effect::PlaySpeech { concept, .. } => Some(concept),
            Effect::NoEffect => None,
            _ => panic!("unexpected speech effect"),
        }
    }

    #[test]
    fn escalation_acquisition_loss_and_relaxation_are_distinct_edges() {
        let (world, entity, mut speech, mut ctx) = fixture();
        for (level, visible, expected) in [
            (AIAlertLevel::Low, false, "tolevelone"),
            (AIAlertLevel::Moderate, false, "toleveltwo"),
            (AIAlertLevel::High, false, "tolevelthree"),
            (AIAlertLevel::High, true, "spotplayer"),
            (AIAlertLevel::Moderate, false, "lostcontact"),
            (AIAlertLevel::Lowest, false, "backtozero"),
        ] {
            ctx.level = level;
            ctx.visible = visible;
            assert_eq!(
                concept(speech.update(&world, entity, ctx, 0.1)).as_deref(),
                Some(expected)
            );
            assert_eq!(concept(speech.update(&world, entity, ctx, 0.1)), None);
        }
    }

    #[test]
    fn hearing_alone_does_not_claim_visual_contact_and_allies_do_not_spot_the_player() {
        let (world, entity, mut speech, mut ctx) = fixture();
        ctx.level = AIAlertLevel::High;
        assert_eq!(
            concept(speech.update(&world, entity, ctx, 0.1)).as_deref(),
            Some("tolevelthree")
        );
        ctx.level = AIAlertLevel::Moderate;
        assert_eq!(concept(speech.update(&world, entity, ctx, 0.1)), None);
        ctx.level = AIAlertLevel::High;
        ctx.visible = true;
        ctx.player_target = false;
        assert_eq!(
            concept(speech.update(&world, entity, ctx, 4.0)).as_deref(),
            Some("tolevelthree")
        );
    }

    #[test]
    fn tactical_calls_wait_for_alert_audio_and_do_not_repeat_each_frame() {
        let (world, entity, mut speech, mut ctx) = fixture();
        ctx.level = AIAlertLevel::High;
        ctx.visible = true;
        ctx.tactic = SpeechTactic::Charge;
        assert_eq!(
            concept(speech.update(&world, entity, ctx, 0.1)).as_deref(),
            Some("spotplayer")
        );
        speech.played("spotplayer", 3.0);
        assert_eq!(concept(speech.update(&world, entity, ctx, 2.0)), None);
        assert_eq!(
            concept(speech.update(&world, entity, ctx, 1.0)).as_deref(),
            Some("reactcharge")
        );
        assert_eq!(concept(speech.update(&world, entity, ctx, 6.0)), None);
        ctx.tactic = SpeechTactic::Shoot;
        assert_eq!(
            concept(speech.update(&world, entity, ctx, 0.2)).as_deref(),
            Some("reactshoot")
        );
    }

    #[test]
    fn a_pending_tactical_call_is_discarded_when_sight_is_lost() {
        let (world, entity, mut speech, mut ctx) = fixture();
        ctx.level = AIAlertLevel::High;
        ctx.visible = true;
        ctx.tactic = SpeechTactic::Charge;
        speech.update(&world, entity, ctx, 0.1);
        speech.played("spotplayer", 3.0);
        ctx.visible = false;
        assert_eq!(concept(speech.update(&world, entity, ctx, 4.0)), None);
    }

    #[test]
    fn melee_feedback_waits_for_the_attack_voice_and_expires_when_stale() {
        let (world, entity, mut speech, ctx) = fixture();
        speech.played("comattack", 1.0);
        assert_eq!(
            concept(speech.melee_feedback(&world, entity, true)).as_deref(),
            Some("comblocked")
        );
        // No playback feedback: arbitration rejected the equal-priority request.
        assert_eq!(concept(speech.update(&world, entity, ctx, 0.5)), None);
        assert_eq!(
            concept(speech.update(&world, entity, ctx, 0.6)).as_deref(),
            Some("comblocked")
        );
        speech.played("comattack", 7.0);
        speech.melee_feedback(&world, entity, false);
        assert_eq!(concept(speech.update(&world, entity, ctx, 7.1)), None);
        speech.melee_feedback(&world, entity, true);
        speech.played("comblocked", 1.0);
        assert_eq!(concept(speech.update(&world, entity, ctx, 1.1)), None);
    }

    #[test]
    fn committed_damage_preserves_batched_ambush_order_and_ignores_healing_and_death() {
        let (_, _, mut speech, mut ctx) = fixture();
        speech.damage_pending(ctx);
        ctx.level = AIAlertLevel::High;
        ctx.visible = true;
        speech.damage_pending(ctx);
        assert_eq!(speech.health_changed(19, 20, 20), None);
        assert_eq!(speech.health_changed(20, 19, 20), Some(Cue::HitAmbush));
        assert_eq!(speech.health_changed(19, 18, 20), Some(Cue::HitHigh));
        assert_eq!(speech.health_changed(18, 10, 20), Some(Cue::HitLow));
        assert_eq!(speech.health_changed(10, 0, 20), None);
    }

    #[test]
    fn mute_inactive_context_and_saved_cooldowns_prevent_chatter() {
        let (mut world, entity, mut speech, mut ctx) = fixture();
        ctx.active = false;
        ctx.level = AIAlertLevel::High;
        ctx.visible = true;
        assert_eq!(concept(speech.update(&world, entity, ctx, 1.0)), None);
        ctx.active = true;
        assert_eq!(concept(speech.update(&world, entity, ctx, 1.0)), None);
        world.add_component(entity, (PropAIBroadcastDisabled(true),));
        assert_eq!(concept(speech.request(&world, entity, Cue::Blocked)), None);
        world.add_component(entity, (PropAIBroadcastDisabled(false),));
        assert_eq!(
            concept(speech.request(&world, entity, Cue::Blocked)).as_deref(),
            Some("comblocked")
        );
        let saved = serde_json::to_string(&speech).unwrap();
        let mut restored: SpeechEvents = serde_json::from_str(&saved).unwrap();
        assert_eq!(
            concept(restored.request(&world, entity, Cue::Blocked)),
            None
        );
        restored.update(&world, entity, ctx, 3.1);
        assert_eq!(
            concept(restored.request(&world, entity, Cue::Blocked)).as_deref(),
            Some("comblocked")
        );
    }
}

impl Cue {
    fn concept(self) -> &'static str {
        match self {
            Self::ToOne => "tolevelone",
            Self::ToTwo => "toleveltwo",
            Self::ToThree => "tolevelthree",
            Self::SpotPlayer => "spotplayer",
            Self::BackToZero => "backtozero",
            Self::LostContact => "lostcontact",
            Self::HitHigh => "comhithigh",
            Self::HitLow => "comhitlow",
            Self::HitAmbush => "comhitamb",
            Self::HitNoDamage => "comhitnodam",
            Self::SuccessfulHit => "comsucchit",
            Self::Blocked => "comblocked",
            Self::Charge => "reactcharge",
            Self::Shoot => "reactshoot",
        }
    }

    fn cooldown(self) -> f32 {
        // Repeated contact/visibility edges must not chatter faster than speech.
        // These are request throttles; clip duration/priority governs playback.
        match self {
            Self::SpotPlayer | Self::LostContact => 10.0,
            Self::HitHigh | Self::HitLow | Self::HitAmbush | Self::HitNoDamage => 1.0,
            Self::Charge | Self::Shoot => 5.0,
            _ => 3.0,
        }
    }
}

#[derive(Default, Serialize, Deserialize)]
pub(crate) struct SpeechEvents {
    level: Option<AIAlertLevel>,
    visible: bool,
    had_contact: bool,
    tactic: SpeechTactic,
    pending_tactic: Option<(Cue, f32)>,
    pending_feedback: Option<(Cue, f32)>,
    busy: f32,
    cooldowns: HashMap<Cue, f32>,
    /// One entry per emitted HP adjustment, preserving batched hit order.
    pending_damage: VecDeque<bool>,
}

impl SpeechEvents {
    pub fn synchronize(&mut self, context: Option<SpeechContext>) {
        self.level = context.map(|c| c.level);
        self.visible = context.is_some_and(|c| c.visible);
        self.had_contact = self.visible;
        self.tactic = context.map_or(SpeechTactic::None, |c| c.tactic);
        self.pending_tactic = None;
        self.pending_feedback = None;
    }

    pub fn played(&mut self, concept: &str, duration: f32) {
        if self
            .pending_feedback
            .is_some_and(|(cue, _)| cue.concept() == concept)
        {
            self.pending_feedback = None;
        }
        if duration.is_finite() && duration >= 0.0 {
            self.busy = duration;
        }
    }

    pub fn melee_feedback(&mut self, world: &World, entity: EntityId, blocked: bool) -> Effect {
        let cue = if blocked {
            Cue::Blocked
        } else {
            Cue::SuccessfulHit
        };
        // Try immediately so authored priority can interrupt an alert/ambient
        // line. If arbitration rejects it, retain one bounded retry. Accepted
        // playback clears this pending call through `played`.
        let was_busy = self.busy > 0.0;
        let effect = self.request(world, entity, cue);
        if was_busy && !matches!(effect, Effect::NoEffect) {
            self.pending_feedback = Some((cue, 6.0));
        }
        effect
    }

    pub fn request(&mut self, world: &World, entity: EntityId, cue: Cue) -> Effect {
        if self
            .cooldowns
            .get(&cue)
            .is_some_and(|seconds| *seconds > 0.0)
            || !speech_util::broadcasts_enabled(world, entity)
        {
            return Effect::NoEffect;
        }
        let Some(voice_index) = speech_util::resolve_entity_voice_index(world, entity) else {
            return Effect::NoEffect;
        };
        self.cooldowns.insert(cue, cue.cooldown());
        // Reserve the current tick until the playback feedback arrives.
        self.busy = self.busy.max(0.1);
        Effect::PlaySpeech {
            entity_id: entity,
            voice_index,
            concept: cue.concept().into(),
            tags: vec![],
        }
    }

    pub fn update(
        &mut self,
        world: &World,
        entity: EntityId,
        context: SpeechContext,
        dt: f32,
    ) -> Effect {
        self.busy = (self.busy - dt).max(0.0);
        self.cooldowns.retain(|_, remaining| {
            *remaining -= dt;
            *remaining > 0.0
        });
        if !context.active {
            self.synchronize(Some(context));
            return Effect::NoEffect;
        }
        if let Some((_, remaining)) = self.pending_feedback.as_mut() {
            *remaining -= dt;
            if *remaining <= 0.0 {
                self.pending_feedback = None;
            }
        }
        let previous = self.level.replace(context.level);
        let was_visible = std::mem::replace(&mut self.visible, context.visible);
        self.had_contact |= context.visible;
        let transition = previous.and_then(|old| {
            if old != context.level {
                if (context.level as u32) > (old as u32) {
                    Some(match context.level {
                        AIAlertLevel::Low => Cue::ToOne,
                        AIAlertLevel::Moderate => Cue::ToTwo,
                        AIAlertLevel::High if context.visible && context.player_target => {
                            Cue::SpotPlayer
                        }
                        _ => Cue::ToThree,
                    })
                } else if context.level == AIAlertLevel::Lowest {
                    Some(Cue::BackToZero)
                } else if old == AIAlertLevel::High && !context.visible && self.had_contact {
                    self.had_contact = false;
                    Some(Cue::LostContact)
                } else {
                    None
                }
            } else if context.level == AIAlertLevel::High
                && context.visible
                && context.player_target
                && !was_visible
            {
                Some(Cue::SpotPlayer)
            } else {
                None
            }
        });
        if context.level == AIAlertLevel::Lowest {
            self.had_contact = false;
        }

        if self.tactic != context.tactic {
            self.tactic = context.tactic;
            self.pending_tactic = if context.visible {
                match context.tactic {
                    SpeechTactic::Charge => Some((Cue::Charge, 6.0)),
                    SpeechTactic::Shoot => Some((Cue::Shoot, 6.0)),
                    SpeechTactic::None => None,
                }
            } else {
                None
            };
        }
        if let Some((_, remaining)) = self.pending_tactic.as_mut() {
            *remaining -= dt;
            if *remaining <= 0.0 || !context.visible {
                self.pending_tactic = None;
            }
        }
        if let Some(cue) = transition {
            return self.request(world, entity, cue);
        }
        // Let the alert line finish before the tactical response. Drop stale
        // tactics rather than announcing a charge after the target is lost.
        if self.busy <= 0.0 {
            if let Some((cue, _)) = self.pending_feedback.take() {
                self.cooldowns.remove(&cue);
                return self.request(world, entity, cue);
            }
            if let Some((cue, _)) = self.pending_tactic.take() {
                return self.request(world, entity, cue);
            }
        }
        Effect::NoEffect
    }

    pub fn damage_pending(&mut self, context: SpeechContext) {
        self.pending_damage.push_back(
            !context.visible && matches!(context.level, AIAlertLevel::Lowest | AIAlertLevel::Low),
        );
    }

    pub fn health_changed(&mut self, previous: i32, current: i32, max_hp: i32) -> Option<Cue> {
        if current >= previous {
            return None;
        }
        let ambushed = self.pending_damage.pop_front().unwrap_or(false);
        if current <= 0 {
            return None;
        } // death has its own higher-priority voice
        Some(if ambushed {
            Cue::HitAmbush
        }
        // Audio-specific damage thresholds are not decoded yet. Use a
        // substantial wound (a quarter of max HP) for the heavy-hit line.
        else if (previous - current) as f32 >= max_hp.max(previous).max(1) as f32 * 0.25 {
            Cue::HitLow
        } else {
            Cue::HitHigh
        })
    }
}
