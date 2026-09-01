//! The station's security console (`SecurityComputer`) - "hack to temporarily
//! disable security".
//!
//! Frobbing it opens the shared HRM board. Winning does the two things the
//! console is for:
//!
//! * the station alarm stands down - its count is cleared and every alerted
//!   ecology is reset, which returns it to the normal population profile and
//!   (over its switch links) clears the cameras that raised the alarm; and
//! * the level's cameras cannot see the player for the console's authored
//!   window (`P$HackTime`), which the alarm owns because raising an alarm
//!   cancels it.
//!
//! A critical failure breaks the console generically, and a broken console
//! refuses to open. Success deliberately leaves `P$ObjState` alone: the window
//! is temporary, so the console has to be hackable again once it lapses.

use dark::properties::PropHackTime;
use shipyard::{EntityId, Get, View, World};

use crate::{quest_info::QuestInfo, scripts::Effect};

use super::hack_board::{HackBoardGui, HackBoardObject};
use super::traits::TRAIT_SECURITY_EXPERT;

/// What the Security Expert O/S trait is worth at a security console, and
/// nowhere else.
const SECURITY_EXPERT_HACK_BONUS: i32 = 2;

/// The security console, on the shared hack board.
pub struct SecurityConsole;

pub type SecurityComputerGui = HackBoardGui<SecurityConsole>;

impl HackBoardObject for SecurityConsole {
    fn on_success(entity_id: EntityId, world: &World) -> Effect {
        security_hack_success(entity_id, world)
    }

    fn skill_bonus(world: &World) -> i32 {
        world
            .borrow::<shipyard::UniqueView<QuestInfo>>()
            .map(|quests| quests.player_stats().has_os_trait(TRAIT_SECURITY_EXPERT))
            .unwrap_or(false)
            .then_some(SECURITY_EXPERT_HACK_BONUS)
            .unwrap_or(0)
    }
}

/// How long a win blinds the cameras: the console's authored `P$HackTime`.
/// A console that authors none buys no window - only the stand-down.
fn blind_seconds(world: &World, entity_id: EntityId) -> f32 {
    world
        .borrow::<View<PropHackTime>>()
        .ok()
        .and_then(|times| times.get(entity_id).ok().map(PropHackTime::seconds))
        .unwrap_or(0.0)
}

fn security_hack_success(entity_id: EntityId, world: &World) -> Effect {
    Effect::combine(vec![
        Effect::ClearSecurityAlarm { from: entity_id },
        Effect::BlindSecurityCameras {
            seconds: blind_seconds(world, entity_id),
        },
    ])
}

#[cfg(test)]
mod tests {
    use dark::properties::{ObjectState, PropHackDiff, PropObjState};
    use shipyard::World;

    use super::*;
    use crate::gui::Gui;

    fn flatten(effect: &Effect) -> Vec<&Effect> {
        match effect {
            Effect::Combined { effects } => effects.iter().flat_map(flatten).collect(),
            other => vec![other],
        }
    }

    #[test]
    fn a_won_hack_stands_security_down_and_blinds_the_cameras() {
        let mut world = World::new();
        let console = world.add_entity(PropHackTime(120_000));

        let effects = security_hack_success(console, &world);
        let effects = flatten(&effects);
        assert!(effects.iter().any(
            |effect| matches!(effect, Effect::ClearSecurityAlarm { from } if *from == console)
        ));
        assert!(effects.iter().any(|effect| matches!(
            effect,
            Effect::BlindSecurityCameras { seconds } if (*seconds - 120.0).abs() < 1.0e-3
        )));
    }

    #[test]
    fn a_console_authoring_no_window_still_stands_security_down() {
        let mut world = World::new();
        let console = world.add_entity(());
        let effects = security_hack_success(console, &world);
        assert!(flatten(&effects).iter().any(
            |effect| matches!(effect, Effect::BlindSecurityCameras { seconds } if *seconds == 0.0)
        ));
    }

    #[test]
    fn critical_failure_breaks_the_console_and_it_refuses_to_reopen() {
        let mut world = World::new();
        let console = world.add_entity(PropHackDiff {
            success_chance: 20,
            critical_chance: 10,
            cost: 3.0,
        });
        assert!(matches!(
            super::super::keypad::break_on_critical_failure(console, &world),
            Effect::SetObjectState {
                entity_id,
                state: ObjectState::Broken,
            } if entity_id == console
        ));

        assert!(SecurityComputerGui::new().opens_on_frob(console, &world));
        world.add_component(console, PropObjState(ObjectState::Broken));
        assert!(!SecurityComputerGui::new().opens_on_frob(console, &world));
    }

    #[test]
    fn a_console_stays_hackable_after_a_win_because_the_window_lapses() {
        // Nothing in the win writes ObjState - the blindness is temporary, so
        // the console has to be hackable again once it runs out.
        let mut world = World::new();
        let console = world.add_entity(PropHackDiff {
            success_chance: 20,
            critical_chance: 10,
            cost: 3.0,
        });
        assert!(
            !flatten(&security_hack_success(console, &world))
                .iter()
                .any(|effect| matches!(effect, Effect::SetObjectState { .. }))
        );
        assert!(SecurityComputerGui::new().opens_on_frob(console, &world));
    }
}
