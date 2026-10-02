use super::{Effect, MessagePayload, Script, ScriptRestoreContext, ScriptState, ScriptStateError};
use crate::{physics::PhysicsWorld, time::Time};
use dark::properties::AITeam;
use shipyard::{EntityId, World};

/// CharmStim carries ten seconds; the caster scales its source by effective PSI.
/// The retail manual specifies 10 seconds per PSI for Neural Restructuring.
#[derive(Default)]
pub struct Charmable {
    remaining: Option<f32>,
    original_team: Option<AITeam>,
}

impl Charmable {
    fn end(&mut self, entity_id: EntityId) -> Effect {
        self.remaining = None;
        self.original_team
            .take()
            .map(|team| Effect::SetAITeam { entity_id, team })
            .unwrap_or(Effect::NoEffect)
    }
}
impl Script for Charmable {
    fn handle_message(
        &mut self,
        entity_id: EntityId,
        world: &World,
        _: &PhysicsWorld,
        message: &MessagePayload,
    ) -> Effect {
        match message {
            MessagePayload::Stimulus {
                stim_template: -3395,
                intensity,
            } if intensity.is_finite() && *intensity > 0.0 => {
                self.original_team
                    .get_or_insert_with(|| super::ai::ai_util::ai_team(world, entity_id));
                self.remaining = Some(*intensity);
                Effect::SetAITeam {
                    entity_id,
                    team: AITeam::Good,
                }
            }
            MessagePayload::Signal { name } if name.eq_ignore_ascii_case("AbortCharm") => {
                self.end(entity_id)
            }
            _ => Effect::NoEffect,
        }
    }
    fn update(&mut self, entity_id: EntityId, _: &World, _: &PhysicsWorld, time: &Time) -> Effect {
        let Some(remaining) = self.remaining else {
            return Effect::NoEffect;
        };
        self.remaining = Some(remaining - time.elapsed.as_secs_f32());
        if self.remaining.unwrap() <= 0.0 {
            self.end(entity_id)
        } else {
            Effect::NoEffect
        }
    }
    fn script_state_key(&self) -> Option<&'static str> {
        Some("shock2vr.charmable")
    }
    fn save_state(&self) -> Result<ScriptState, ScriptStateError> {
        ScriptState::encode(
            1,
            &(self.remaining, self.original_team),
            "shock2vr.charmable",
        )
    }
    fn restore_state(
        &mut self,
        saved: &ScriptState,
        _: &ScriptRestoreContext<'_>,
    ) -> Result<(), ScriptStateError> {
        (self.remaining, self.original_team) = saved.decode(1, "shock2vr.charmable")?;
        Ok(())
    }
}

/// Source sites call this only after positive player-authored damage resolves.
pub(crate) fn player_damage(victim: EntityId) -> Effect {
    Effect::Send {
        msg: super::Message {
            to: victim,
            payload: MessagePayload::Signal {
                name: "AbortCharm".into(),
            },
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dark::properties::PropAITeam;
    #[test]
    fn charm_restores_original_team_after_refresh_and_expiry() {
        let mut world = World::new();
        let victim = world.add_entity((PropAITeam(AITeam::Bad2),));
        let physics = PhysicsWorld::new();
        let mut script = Charmable::default();
        let stimulus = MessagePayload::Stimulus {
            stim_template: -3395,
            intensity: 60.0,
        };
        assert!(matches!(
            script.handle_message(victim, &world, &physics, &stimulus),
            Effect::SetAITeam {
                team: AITeam::Good,
                ..
            }
        ));
        world.add_component(victim, PropAITeam(AITeam::Good));
        script.handle_message(victim, &world, &physics, &stimulus);
        assert_eq!(script.remaining, Some(60.0));
        assert!(matches!(
            script.update(
                victim,
                &world,
                &physics,
                &Time {
                    elapsed: std::time::Duration::from_secs(61),
                    total: std::time::Duration::from_secs(61)
                }
            ),
            Effect::SetAITeam {
                team: AITeam::Bad2,
                ..
            }
        ));
        assert!(matches!(script.end(victim), Effect::NoEffect));
    }
    #[test]
    fn damage_without_a_human_source_does_not_abort_charm() {
        let mut world = World::new();
        let victim = world.add_entity((PropAITeam(AITeam::Good),));
        let physics = PhysicsWorld::new();
        let mut script = Charmable {
            remaining: Some(20.0),
            original_team: Some(AITeam::Bad1),
        };
        assert!(matches!(
            script.handle_message(
                victim,
                &world,
                &physics,
                &MessagePayload::Damage {
                    amount: 1.0,
                    impact: None
                }
            ),
            Effect::NoEffect
        ));
        assert_eq!(script.remaining, Some(20.0));
        assert!(matches!(
            script.handle_message(
                victim,
                &world,
                &physics,
                &MessagePayload::Signal {
                    name: "AbortCharm".into()
                }
            ),
            Effect::SetAITeam {
                team: AITeam::Bad1,
                ..
            }
        ));
        assert_eq!(script.remaining, None);
    }

    #[test]
    fn charm_state_round_trips_remaining_time_and_team() {
        let script = Charmable {
            remaining: Some(13.5),
            original_team: Some(AITeam::Bad1),
        };
        let state = script.save_state().unwrap();
        let mut restored = Charmable::default();
        restored
            .restore_state(
                &state,
                &ScriptRestoreContext::new(&std::collections::HashMap::new()),
            )
            .unwrap();
        assert_eq!(restored.remaining, Some(13.5));
        let mut world = World::new();
        let victim = world.add_entity((PropAITeam(AITeam::Good),));
        let physics = PhysicsWorld::new();
        let time = |seconds| Time {
            elapsed: std::time::Duration::from_secs(seconds),
            total: std::time::Duration::from_secs(seconds),
        };
        assert!(matches!(
            restored.update(victim, &world, &physics, &time(13)),
            Effect::NoEffect
        ));
        assert!(matches!(
            restored.update(victim, &world, &physics, &time(1)),
            Effect::SetAITeam {
                team: AITeam::Bad1,
                ..
            }
        ));
    }
}
