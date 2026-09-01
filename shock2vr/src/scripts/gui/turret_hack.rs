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

use shipyard::{EntityId, World};

use crate::scripts::{Effect, ai::ai_util};

use super::hack_board::{HackBoardGui, HackBoardObject};

/// A turret, on the shared hack board.
pub struct HackableTurret;

pub type TurretHackGui = HackBoardGui<HackableTurret>;

impl HackBoardObject for HackableTurret {
    fn on_success(entity_id: EntityId, _world: &World) -> Effect {
        Effect::SetAITeam {
            entity_id,
            team: ai_util::PLAYER_TEAM,
        }
    }

    /// Only while it is still hostile: on the player's team there is nothing
    /// left to buy.
    fn is_offered(world: &World, entity_id: EntityId) -> bool {
        ai_util::ai_team(world, entity_id) != ai_util::PLAYER_TEAM
    }
}

#[cfg(test)]
mod tests {
    use crate::gui::Gui;
    use dark::properties::{AITeam, ObjectState, PropAITeam, PropHackDiff, PropObjState};
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
            HackableTurret::on_success(turret, &world),
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
        assert!(TurretHackGui::new().opens_on_frob(turret, &world));
        world.add_component(turret, PropAITeam(ai_util::PLAYER_TEAM));
        assert!(!TurretHackGui::new().opens_on_frob(turret, &world));
    }

    #[test]
    fn a_critically_failed_turret_breaks_and_refuses_its_board() {
        let mut world = World::new();
        let turret = turret(&mut world);
        assert!(matches!(
            super::super::keypad::break_on_critical_failure(turret, &world),
            Effect::SetObjectState {
                entity_id,
                state: ObjectState::Broken,
            } if entity_id == turret
        ));
        world.add_component(turret, PropObjState(ObjectState::Broken));
        assert!(!TurretHackGui::new().opens_on_frob(turret, &world));
    }
}
