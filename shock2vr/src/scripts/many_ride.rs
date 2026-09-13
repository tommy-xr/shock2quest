//! Scripts used by Engineering 2's scripted ride through The Many.
//!
//! The retail sequence is wired entirely through ordinary mission objects:
//! `WhiteOut` hides the transition, then relays to `SitDownRightNow`,
//! `ParalyzePlayers`, and the continuously-moving cage platform. A second
//! white-out releases the player at the end.

use cgmath::{Quaternion, Vector3};
use dark::properties::{PropDelayTime, PropPosition};
use serde::{Deserialize, Serialize};
use shipyard::{EntityId, Get, UniqueView, View, World};

use crate::{mission::PlayerInfo, physics::PhysicsWorld, time::Time};

use super::{
    Effect, Message, MessagePayload, Script, ScriptRestoreContext, ScriptState, ScriptStateError,
    script_util::{get_first_entity_by_name, send_to_all_switch_links},
};

const WHITE_OUT_STATE_KEY: &str = "shock2vr.many_ride.white_out";
const SIT_DOWN_STATE_KEY: &str = "shock2vr.many_ride.sit_down";
const PARALYZE_STATE_KEY: &str = "shock2vr.many_ride.paralyze_players";

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize)]
enum WhiteOutPhase {
    #[default]
    Idle,
    FadingToWhite {
        elapsed: f32,
    },
    FadingFromWhite {
        elapsed: f32,
    },
}

#[derive(Deserialize, Serialize)]
struct WhiteOutState {
    duration_seconds: f32,
    phase: WhiteOutPhase,
}

/// Fade the view to white over the authored `DelayTime`, relay `TurnOn` while
/// the screen is fully covered, then reveal the new view over the same period.
pub struct WhiteOut {
    duration_seconds: f32,
    phase: WhiteOutPhase,
}

impl WhiteOut {
    pub fn new() -> Self {
        Self {
            duration_seconds: 1.0,
            phase: WhiteOutPhase::Idle,
        }
    }
}

impl Script for WhiteOut {
    fn initialize(&mut self, entity_id: EntityId, world: &World) -> Effect {
        self.duration_seconds = world
            .borrow::<View<PropDelayTime>>()
            .unwrap()
            .get(entity_id)
            .map(|delay| delay.delay.as_secs_f32())
            .unwrap_or(1.0)
            .max(f32::EPSILON);
        Effect::NoEffect
    }

    fn handle_message(
        &mut self,
        _entity_id: EntityId,
        _world: &World,
        _physics: &PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        if matches!(msg, MessagePayload::TurnOn { .. }) {
            self.phase = WhiteOutPhase::FadingToWhite { elapsed: 0.0 };
            Effect::SetScreenFade { alpha: 0.0 }
        } else {
            Effect::NoEffect
        }
    }

    fn update(
        &mut self,
        entity_id: EntityId,
        world: &World,
        _physics: &PhysicsWorld,
        time: &Time,
    ) -> Effect {
        let delta = time.elapsed.as_secs_f32();
        match self.phase {
            WhiteOutPhase::Idle => Effect::NoEffect,
            WhiteOutPhase::FadingToWhite { elapsed } => {
                let elapsed = elapsed + delta;
                let alpha = (elapsed / self.duration_seconds).clamp(0.0, 1.0);
                if elapsed >= self.duration_seconds {
                    self.phase = WhiteOutPhase::FadingFromWhite { elapsed: 0.0 };
                    Effect::combine(vec![
                        Effect::SetScreenFade { alpha: 1.0 },
                        send_to_all_switch_links(
                            world,
                            entity_id,
                            MessagePayload::TurnOn { from: entity_id },
                        ),
                    ])
                } else {
                    self.phase = WhiteOutPhase::FadingToWhite { elapsed };
                    Effect::SetScreenFade { alpha }
                }
            }
            WhiteOutPhase::FadingFromWhite { elapsed } => {
                let elapsed = elapsed + delta;
                let alpha = 1.0 - (elapsed / self.duration_seconds).clamp(0.0, 1.0);
                if elapsed >= self.duration_seconds {
                    self.phase = WhiteOutPhase::Idle;
                } else {
                    self.phase = WhiteOutPhase::FadingFromWhite { elapsed };
                }
                Effect::SetScreenFade { alpha }
            }
        }
    }

    fn script_state_key(&self) -> Option<&'static str> {
        Some(WHITE_OUT_STATE_KEY)
    }

    fn save_state(&self) -> Result<ScriptState, ScriptStateError> {
        ScriptState::encode(
            1,
            &WhiteOutState {
                duration_seconds: self.duration_seconds,
                phase: self.phase,
            },
            WHITE_OUT_STATE_KEY,
        )
    }

    fn restore_state(
        &mut self,
        state: &ScriptState,
        _context: &ScriptRestoreContext<'_>,
    ) -> Result<(), ScriptStateError> {
        let restored: WhiteOutState = state.decode(1, WHITE_OUT_STATE_KEY)?;
        self.duration_seconds = restored.duration_seconds.max(f32::EPSILON);
        self.phase = restored.phase;
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
struct PlayerPose {
    position: Vector3<f32>,
    rotation: Quaternion<f32>,
}

#[derive(Deserialize, Serialize)]
struct SitDownState {
    return_pose: Option<PlayerPose>,
}

/// Temporarily seat the single supported player at retail's `Seat1` marker.
///
/// The original sends `GotoSeat` to PlayerScript and later pairs it with
/// `StandUp`, returning the player from the hallucination to their pre-seat
/// pose. The port moves the physical pawn to render the ride, so retain that
/// exact pose here (including across save/load).
pub struct SitDownRightNow {
    return_pose: Option<PlayerPose>,
}

impl SitDownRightNow {
    pub fn new() -> Self {
        Self { return_pose: None }
    }
}

impl Script for SitDownRightNow {
    fn handle_message(
        &mut self,
        _entity_id: EntityId,
        world: &World,
        _physics: &PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        match msg {
            MessagePayload::TurnOn { .. } => {
                let Some(seat) = get_first_entity_by_name(world, "Seat1") else {
                    return Effect::NoEffect;
                };
                let positions = world.borrow::<View<PropPosition>>().unwrap();
                let Ok(seat_position) = positions.get(seat) else {
                    return Effect::NoEffect;
                };

                if self.return_pose.is_none() {
                    self.return_pose =
                        world
                            .borrow::<UniqueView<PlayerInfo>>()
                            .ok()
                            .map(|player| PlayerPose {
                                position: player.pos,
                                rotation: player.rotation,
                            });
                }

                Effect::combine(vec![
                    Effect::SetPlayerPosition {
                        position: seat_position.position,
                        is_teleport: true,
                        source: dark::properties::TeleportSource::ScriptedTrap,
                    },
                    Effect::SetPlayerRotation {
                        rotation: seat_position.rotation,
                    },
                    Effect::SetPlayerControlsEnabled { enabled: false },
                ])
            }
            MessagePayload::StandUp => {
                let Some(return_pose) = self.return_pose.take() else {
                    return Effect::SetPlayerControlsEnabled { enabled: true };
                };
                Effect::combine(vec![
                    Effect::SetPlayerPosition {
                        position: return_pose.position,
                        is_teleport: true,
                        source: dark::properties::TeleportSource::ScriptedTrap,
                    },
                    Effect::SetPlayerRotation {
                        rotation: return_pose.rotation,
                    },
                    Effect::SetPlayerControlsEnabled { enabled: true },
                ])
            }
            _ => Effect::NoEffect,
        }
    }

    fn script_state_key(&self) -> Option<&'static str> {
        Some(SIT_DOWN_STATE_KEY)
    }

    fn save_state(&self) -> Result<ScriptState, ScriptStateError> {
        ScriptState::encode(
            1,
            &SitDownState {
                return_pose: self.return_pose,
            },
            SIT_DOWN_STATE_KEY,
        )
    }

    fn restore_state(
        &mut self,
        state: &ScriptState,
        _context: &ScriptRestoreContext<'_>,
    ) -> Result<(), ScriptStateError> {
        let restored: SitDownState = state.decode(1, SIT_DOWN_STATE_KEY)?;
        self.return_pose = restored.return_pose;
        Ok(())
    }
}

#[derive(Deserialize, Serialize)]
struct ParalyzePlayersState {
    paralyzed: bool,
}

/// Prevent player movement between TurnOn and TurnOff. In eng2 the seated
/// player rests on the linked moving cage, so moving-support carry preserves
/// the relative pose while the platform travels.
pub struct ParalyzePlayers {
    paralyzed: bool,
}

impl ParalyzePlayers {
    pub fn new() -> Self {
        Self { paralyzed: false }
    }
}

impl Script for ParalyzePlayers {
    fn update(
        &mut self,
        _entity_id: EntityId,
        _world: &World,
        _physics: &PhysicsWorld,
        _time: &Time,
    ) -> Effect {
        if self.paralyzed {
            Effect::SetPlayerControlsEnabled { enabled: false }
        } else {
            Effect::NoEffect
        }
    }

    fn handle_message(
        &mut self,
        _entity_id: EntityId,
        _world: &World,
        _physics: &PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        match msg {
            MessagePayload::TurnOn { .. } => {
                self.paralyzed = true;
                Effect::SetPlayerControlsEnabled { enabled: false }
            }
            MessagePayload::TurnOff { .. } => {
                self.paralyzed = false;
                Effect::SetPlayerControlsEnabled { enabled: true }
            }
            _ => Effect::NoEffect,
        }
    }

    fn script_state_key(&self) -> Option<&'static str> {
        Some(PARALYZE_STATE_KEY)
    }

    fn save_state(&self) -> Result<ScriptState, ScriptStateError> {
        ScriptState::encode(
            1,
            &ParalyzePlayersState {
                paralyzed: self.paralyzed,
            },
            PARALYZE_STATE_KEY,
        )
    }

    fn restore_state(
        &mut self,
        state: &ScriptState,
        _context: &ScriptRestoreContext<'_>,
    ) -> Result<(), ScriptStateError> {
        let restored: ParalyzePlayersState = state.decode(1, PARALYZE_STATE_KEY)?;
        self.paralyzed = restored.paralyzed;
        Ok(())
    }
}

/// Release the player at the end of the ride.
pub struct StandUpAgain;

impl StandUpAgain {
    pub fn new() -> Self {
        Self
    }
}

impl Script for StandUpAgain {
    fn handle_message(
        &mut self,
        _entity_id: EntityId,
        world: &World,
        _physics: &PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        if !matches!(msg, MessagePayload::TurnOn { .. }) {
            return Effect::NoEffect;
        }

        // Retail sends StandUp to PlayerScript, which owns the matching
        // GotoSeat state. Our single-player equivalent keeps that state on the
        // unique SitDownNow1 script instance.
        get_first_entity_by_name(world, "SitDownNow1")
            .map(|sit_down| Effect::Send {
                msg: Message {
                    payload: MessagePayload::StandUp,
                    to: sit_down,
                },
            })
            .unwrap_or(Effect::SetPlayerControlsEnabled { enabled: true })
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, time::Duration};

    use cgmath::{Quaternion, vec3};
    use dark::properties::{Link, Links, PropSymName, ToLink, WrappedEntityId};

    use super::*;

    fn time(seconds: f32) -> Time {
        Time {
            elapsed: Duration::from_secs_f32(seconds),
            total: Duration::from_secs_f32(seconds),
        }
    }

    #[test]
    fn white_out_relays_only_after_reaching_full_white() {
        let mut world = World::new();
        let target = world.add_entity(());
        let source = world.add_entity((
            PropDelayTime {
                delay: Duration::from_secs(2),
            },
            Links {
                to_links: vec![ToLink {
                    to_template_id: 0,
                    to_entity_id: Some(WrappedEntityId(target)),
                    link: Link::SwitchLink,
                }],
            },
        ));
        let physics = PhysicsWorld::new();
        let mut script = WhiteOut::new();
        script.initialize(source, &world);
        script.handle_message(
            source,
            &world,
            &physics,
            &MessagePayload::TurnOn { from: source },
        );

        let halfway = Effect::flatten(vec![script.update(source, &world, &physics, &time(1.0))]);
        assert!(
            halfway
                .iter()
                .any(|effect| matches!(effect, Effect::SetScreenFade { alpha } if *alpha == 0.5))
        );
        assert!(
            !halfway
                .iter()
                .any(|effect| matches!(effect, Effect::Send { .. }))
        );

        let covered = Effect::flatten(vec![script.update(source, &world, &physics, &time(1.0))]);
        assert!(
            covered
                .iter()
                .any(|effect| matches!(effect, Effect::SetScreenFade { alpha } if *alpha == 1.0))
        );
        assert!(covered.iter().any(|effect| matches!(
            effect,
            Effect::Send { msg }
                if msg.to == target && matches!(msg.payload, MessagePayload::TurnOn { .. })
        )));
    }

    #[test]
    fn sit_down_uses_the_single_player_seat_marker() {
        let mut world = World::new();
        let player_entity = world.add_entity(());
        let inventory_entity = world.add_entity(());
        let return_rotation = Quaternion::new(0.5, 0.0, 0.5, 0.0);
        world.add_unique(PlayerInfo {
            pos: vec3(1.0, 2.0, 3.0),
            rotation: return_rotation,
            entity_id: player_entity,
            left_hand_entity_id: None,
            right_hand_entity_id: None,
            inventory_entity_id: inventory_entity,
        });
        world.add_entity((
            PropSymName("Seat1".to_owned()),
            PropPosition {
                position: vec3(4.0, 5.0, 6.0),
                cell: 0,
                rotation: Quaternion::new(1.0, 0.0, 0.0, 0.0),
            },
        ));
        let physics = PhysicsWorld::new();
        let mut script = SitDownRightNow::new();
        let effects = Effect::flatten(vec![script.handle_message(
            EntityId::dead(),
            &world,
            &physics,
            &MessagePayload::TurnOn {
                from: EntityId::dead(),
            },
        )]);

        assert!(effects.iter().any(|effect| matches!(
            effect,
            Effect::SetPlayerPosition { position, .. } if *position == vec3(4.0, 5.0, 6.0)
        )));
        assert!(
            effects.iter().any(|effect| matches!(
                effect,
                Effect::SetPlayerControlsEnabled { enabled: false }
            ))
        );

        // The pre-seat pose is script-private, versioned state: a save/load
        // during the ride must still let the paired StandUp return safely.
        let saved = script.save_state().unwrap();
        let remap = HashMap::new();
        let context = ScriptRestoreContext::new(&remap);
        let mut restored = SitDownRightNow::new();
        restored.restore_state(&saved, &context).unwrap();
        let stand_up = Effect::flatten(vec![restored.handle_message(
            EntityId::dead(),
            &world,
            &physics,
            &MessagePayload::StandUp,
        )]);
        assert!(stand_up.iter().any(|effect| matches!(
            effect,
            Effect::SetPlayerPosition { position, .. } if *position == vec3(1.0, 2.0, 3.0)
        )));
        assert!(stand_up.iter().any(|effect| matches!(
            effect,
            Effect::SetPlayerRotation { rotation } if *rotation == return_rotation
        )));
        assert!(
            stand_up
                .iter()
                .any(|effect| matches!(effect, Effect::SetPlayerControlsEnabled { enabled: true }))
        );
    }
}
