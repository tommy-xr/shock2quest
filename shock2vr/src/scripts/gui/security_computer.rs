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

use dark::properties::{ObjectState, PropHackTime};
use shipyard::{EntityId, Get, View, World};

use crate::{
    gui::{Gui, GuiComponent, GuiConfig, GuiCursor},
    quest_info::QuestInfo,
    scripts::Effect,
};

use super::computer::{hack_board_config, hack_board_with_instructions};
use super::keypad::{
    HackPhase, HackState, HackTerms, KeyPadMsg, hack_diff, handle_hack_msg, object_state,
};
use super::traits::TRAIT_SECURITY_EXPERT;

/// What the Security Expert O/S trait is worth at a security console, and
/// nowhere else.
const SECURITY_EXPERT_HACK_BONUS: i32 = 2;

pub struct SecurityComputerGui;

#[derive(Clone, Debug, Default)]
pub struct SecurityComputerState {
    hack: HackState,
}

#[derive(Clone)]
pub enum SecurityComputerMsg {
    Hack(KeyPadMsg),
}

fn can_hack(world: &World, entity_id: EntityId) -> bool {
    !matches!(
        object_state(world, entity_id),
        ObjectState::Broken | ObjectState::Destroyed
    ) && hack_diff(world, entity_id).is_some()
}

/// The console's own skill bonus: Security Expert reads as two extra levels of
/// Hack here.
fn skill_bonus(world: &World) -> i32 {
    world
        .borrow::<shipyard::UniqueView<QuestInfo>>()
        .map(|quests| quests.player_stats().has_os_trait(TRAIT_SECURITY_EXPERT))
        .unwrap_or(false)
        .then_some(SECURITY_EXPERT_HACK_BONUS)
        .unwrap_or(0)
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

fn security_hack_critical_failure(entity_id: EntityId, _world: &World) -> Effect {
    Effect::SetObjectState {
        entity_id,
        state: ObjectState::Broken,
    }
}

impl Gui<SecurityComputerState, SecurityComputerMsg> for SecurityComputerGui {
    fn get_components(
        &self,
        _cursor: &Option<GuiCursor>,
        entity_id: EntityId,
        world: &World,
        state: &SecurityComputerState,
    ) -> Vec<GuiComponent<SecurityComputerMsg>> {
        hack_board_with_instructions(entity_id, world, &state.hack, SecurityComputerMsg::Hack)
    }

    fn get_config(&self) -> GuiConfig {
        hack_board_config()
    }

    fn handle_msg(
        &self,
        entity_id: EntityId,
        world: &World,
        state: &SecurityComputerState,
        msg: &SecurityComputerMsg,
    ) -> (SecurityComputerState, Effect) {
        let Some(diff) = hack_diff(world, entity_id) else {
            return (state.clone(), Effect::NoEffect);
        };
        let SecurityComputerMsg::Hack(msg) = msg;
        let (hack, effect) = handle_hack_msg(
            entity_id,
            world,
            &state.hack,
            msg,
            diff,
            HackTerms {
                skill_bonus: skill_bonus(world),
                success: security_hack_success,
                critical_failure: security_hack_critical_failure,
            },
        );
        (SecurityComputerState { hack }, effect)
    }

    fn opens_on_frob(&self, entity_id: EntityId, world: &World) -> bool {
        can_hack(world, entity_id)
    }

    fn prepare_state_on_frob(&self, state: &mut SecurityComputerState) {
        if state.hack.phase != HackPhase::Playing {
            *state = SecurityComputerState::default();
        }
    }
}

#[cfg(test)]
mod tests {
    use dark::properties::{PropHackDiff, PropObjState};
    use shipyard::World;

    use super::*;

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
            security_hack_critical_failure(console, &world),
            Effect::SetObjectState {
                entity_id,
                state: ObjectState::Broken,
            } if entity_id == console
        ));

        assert!(SecurityComputerGui.opens_on_frob(console, &world));
        world.add_component(console, PropObjState(ObjectState::Broken));
        assert!(!SecurityComputerGui.opens_on_frob(console, &world));
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
        assert!(SecurityComputerGui.opens_on_frob(console, &world));
    }
}
