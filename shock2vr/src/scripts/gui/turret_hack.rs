//! Hacking a turret - "hack to make turret shoot your foes".
//!
//! Frobbing a hostile turret opens the shared HRM board. Winning moves the
//! turret onto the player's AI team; nothing else about it changes - it stays
//! powered, keeps its state, and its targeting simply reads the new team, so
//! it now opens fire on whatever is still hostile and leaves the player alone.
//! A turret already on the player's team offers no board.
//!
//! A critical failure breaks the turret generically, and a broken turret
//! refuses to open its board.

use dark::properties::ObjectState;
use shipyard::{EntityId, World};

use crate::{
    gui::{Gui, GuiComponent, GuiConfig, GuiCursor},
    scripts::{Effect, ai::ai_util},
};

use super::computer::{hack_board_config, hack_board_with_instructions};
use super::keypad::{
    HackPhase, HackState, HackTerms, KeyPadMsg, hack_diff, handle_hack_msg, object_state,
};

pub struct TurretHackGui;

#[derive(Clone, Debug, Default)]
pub struct TurretHackState {
    hack: HackState,
}

#[derive(Clone)]
pub enum TurretHackMsg {
    Hack(KeyPadMsg),
}

/// A turret is worth hacking while it is still hostile: on the player's team
/// there is nothing left to buy, and broken is broken.
fn can_hack(world: &World, entity_id: EntityId) -> bool {
    !matches!(
        object_state(world, entity_id),
        ObjectState::Broken | ObjectState::Destroyed
    ) && ai_util::ai_team(world, entity_id) != ai_util::PLAYER_TEAM
        && hack_diff(world, entity_id).is_some()
}

fn turret_hack_success(entity_id: EntityId, _world: &World) -> Effect {
    Effect::SetAITeam {
        entity_id,
        team: ai_util::PLAYER_TEAM,
    }
}

fn turret_hack_critical_failure(entity_id: EntityId, _world: &World) -> Effect {
    Effect::SetObjectState {
        entity_id,
        state: ObjectState::Broken,
    }
}

impl Gui<TurretHackState, TurretHackMsg> for TurretHackGui {
    fn get_components(
        &self,
        _cursor: &Option<GuiCursor>,
        entity_id: EntityId,
        world: &World,
        state: &TurretHackState,
    ) -> Vec<GuiComponent<TurretHackMsg>> {
        hack_board_with_instructions(entity_id, world, &state.hack, TurretHackMsg::Hack)
    }

    fn get_config(&self) -> GuiConfig {
        hack_board_config()
    }

    fn handle_msg(
        &self,
        entity_id: EntityId,
        world: &World,
        state: &TurretHackState,
        msg: &TurretHackMsg,
    ) -> (TurretHackState, Effect) {
        let Some(diff) = hack_diff(world, entity_id) else {
            return (state.clone(), Effect::NoEffect);
        };
        let TurretHackMsg::Hack(msg) = msg;
        let (hack, effect) = handle_hack_msg(
            entity_id,
            world,
            &state.hack,
            msg,
            diff,
            HackTerms {
                skill_bonus: 0,
                success: turret_hack_success,
                critical_failure: turret_hack_critical_failure,
            },
        );
        (TurretHackState { hack }, effect)
    }

    fn opens_on_frob(&self, entity_id: EntityId, world: &World) -> bool {
        can_hack(world, entity_id)
    }

    fn prepare_state_on_frob(&self, state: &mut TurretHackState) {
        if state.hack.phase != HackPhase::Playing {
            *state = TurretHackState::default();
        }
    }
}

#[cfg(test)]
mod tests {
    use dark::properties::{AITeam, PropAITeam, PropHackDiff, PropObjState};
    use shipyard::World;

    use super::*;

    fn turret(world: &mut World) -> EntityId {
        world.add_entity(PropHackDiff {
            success_chance: 20,
            critical_chance: 5,
            cost: 5.0,
        })
    }

    #[test]
    fn a_won_hack_moves_the_turret_onto_the_players_team() {
        let mut world = World::new();
        let turret = turret(&mut world);
        assert!(matches!(
            turret_hack_success(turret, &world),
            Effect::SetAITeam {
                entity_id,
                team: AITeam::Good,
            } if entity_id == turret
        ));
    }

    #[test]
    fn a_hacked_turret_cannot_be_hacked_again() {
        let mut world = World::new();
        let turret = turret(&mut world);
        assert!(TurretHackGui.opens_on_frob(turret, &world));
        world.add_component(turret, PropAITeam(ai_util::PLAYER_TEAM));
        assert!(!TurretHackGui.opens_on_frob(turret, &world));
    }

    #[test]
    fn a_critically_failed_turret_breaks_and_refuses_its_board() {
        let mut world = World::new();
        let turret = turret(&mut world);
        assert!(matches!(
            turret_hack_critical_failure(turret, &world),
            Effect::SetObjectState {
                entity_id,
                state: ObjectState::Broken,
            } if entity_id == turret
        ));
        world.add_component(turret, PropObjState(ObjectState::Broken));
        assert!(!TurretHackGui.opens_on_frob(turret, &world));
    }
}
