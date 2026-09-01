//! The panel every device hacked on the HRM board shows.
//!
//! Console, security console and turret differ only in what a win does, what
//! extra condition offers the board, and what the object itself is worth in
//! Hack skill. Everything else - the board, its authored instructions, the
//! per-open reset, the broken/no-terms gate, and the critical failure that
//! breaks the object - is the same, so it lives here once. A new hackable
//! device is an impl of [`HackBoardObject`] plus a line in the script table.

use std::marker::PhantomData;

use dark::properties::ObjectState;
use shipyard::{EntityId, World};

use crate::{
    gui::{Gui, GuiComponent, GuiConfig, GuiCursor},
    scripts::Effect,
};

use super::computer::{hack_board_config, hack_board_with_instructions};
use super::keypad::{
    HackPhase, HackState, HackTerms, KeyPadMsg, break_on_critical_failure, hack_diff,
    handle_hack_msg, object_state,
};

/// One device hacked on the shared board.
pub(crate) trait HackBoardObject {
    /// What winning does to this device.
    fn on_success(entity_id: EntityId, world: &World) -> Effect;

    /// Any condition beyond "it has hack terms and is not broken". The turret
    /// only offers a board while it is still hostile; a computer that has
    /// already given way offers none.
    fn is_offered(_world: &World, _entity_id: EntityId) -> bool {
        true
    }

    /// What this device itself is worth in Hack skill (retail's Security O/S
    /// trait is +2 at a security console, and nowhere else).
    fn skill_bonus(_world: &World) -> i32 {
        0
    }
}

pub struct HackBoardGui<T>(PhantomData<T>);

impl<T> HackBoardGui<T> {
    pub fn new() -> Self {
        Self(PhantomData)
    }
}

#[derive(Clone, Debug, Default)]
pub struct HackBoardState {
    hack: HackState,
}

#[derive(Clone)]
pub enum HackBoardMsg {
    Hack(KeyPadMsg),
}

impl<T: HackBoardObject> Gui<HackBoardState, HackBoardMsg> for HackBoardGui<T> {
    fn get_components(
        &self,
        _cursor: &Option<GuiCursor>,
        entity_id: EntityId,
        world: &World,
        state: &HackBoardState,
    ) -> Vec<GuiComponent<HackBoardMsg>> {
        hack_board_with_instructions(entity_id, world, &state.hack, HackBoardMsg::Hack)
    }

    fn get_config(&self) -> GuiConfig {
        hack_board_config()
    }

    fn handle_msg(
        &self,
        entity_id: EntityId,
        world: &World,
        state: &HackBoardState,
        msg: &HackBoardMsg,
    ) -> (HackBoardState, Effect) {
        let Some(diff) = hack_diff(world, entity_id) else {
            return (state.clone(), Effect::NoEffect);
        };
        let HackBoardMsg::Hack(msg) = msg;
        let (hack, effect) = handle_hack_msg(
            entity_id,
            world,
            &state.hack,
            msg,
            diff,
            HackTerms {
                skill_bonus: T::skill_bonus(world),
                success: T::on_success,
                critical_failure: break_on_critical_failure,
            },
        );
        (HackBoardState { hack }, effect)
    }

    fn opens_on_frob(&self, entity_id: EntityId, world: &World) -> bool {
        !matches!(
            object_state(world, entity_id),
            ObjectState::Broken | ObjectState::Destroyed
        ) && hack_diff(world, entity_id).is_some()
            && T::is_offered(world, entity_id)
    }

    fn prepare_state_on_frob(&self, state: &mut HackBoardState) {
        if state.hack.phase != HackPhase::Playing {
            *state = HackBoardState::default();
        }
    }
}

#[cfg(test)]
mod tests {
    use dark::properties::{PropHackDiff, PropObjState};
    use shipyard::World;

    use super::*;

    /// A device with no condition of its own beyond the shared gate.
    struct AnyDevice;
    impl HackBoardObject for AnyDevice {
        fn on_success(_entity_id: EntityId, _world: &World) -> Effect {
            Effect::NoEffect
        }
        fn is_offered(world: &World, entity_id: EntityId) -> bool {
            object_state(world, entity_id) != ObjectState::Hacked
        }
    }

    #[test]
    fn a_broken_or_already_hacked_device_does_not_reopen() {
        for state in [ObjectState::Broken, ObjectState::Hacked] {
            let mut world = World::new();
            let device = world.add_entity((
                PropHackDiff {
                    success_chance: 20,
                    critical_chance: 10,
                    cost: 3.0,
                },
                PropObjState(state),
            ));
            assert!(!HackBoardGui::<AnyDevice>::new().opens_on_frob(device, &world));
        }
    }

    #[test]
    fn a_device_with_no_authored_terms_is_not_hackable() {
        let mut world = World::new();
        let device = world.add_entity(());
        assert!(!HackBoardGui::<AnyDevice>::new().opens_on_frob(device, &world));
    }

    #[test]
    fn reopening_preserves_only_an_in_progress_hack() {
        let gui = HackBoardGui::<AnyDevice>::new();
        let mut state = HackBoardState {
            hack: HackState {
                phase: HackPhase::Playing,
                rng_state: 42,
                ..HackState::default()
            },
        };
        gui.prepare_state_on_frob(&mut state);
        assert_eq!(state.hack.phase, HackPhase::Playing);
        assert_eq!(state.hack.rng_state, 42);

        state.hack.phase = HackPhase::Won;
        gui.prepare_state_on_frob(&mut state);
        assert_eq!(state.hack.phase, HackPhase::Unpaid);
    }
}
