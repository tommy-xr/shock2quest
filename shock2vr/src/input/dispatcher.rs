use crate::input_context::InputContext;
use crate::scripts::{Effect, GlobalEffect};

use super::{InputAction, InputActionState};

/// Template spawned by InputAction::SpawnDebugItem (Pistol).
/// Other useful debug templates: Laser: -22, Wrench: -928,
/// grunt og-pipe: -397, monkey - red: -1432
const DEBUG_SPAWN_TEMPLATE_ID: i32 = -17;

const QUICK_SAVE_FILE: &str = "save1.sav";

pub struct ActionDispatcher;

impl ActionDispatcher {
    /// Convert triggered actions directly into effects (no Command indirection).
    ///
    /// Takes the input context because player-relative actions need the head
    /// rotation, which lives only in the input context (PlayerInfo in the
    /// world stores the body rotation). Player position is resolved later by
    /// the effect handler, which has world access.
    pub fn dispatch(state: &InputActionState, input_context: &InputContext) -> Vec<Effect> {
        let mut effects = Vec::new();

        if state.just_triggered(InputAction::PathfindingTestCycle) {
            effects.push(Effect::PathfindingTest);
        }
        if state.just_triggered(InputAction::QuickSave) {
            effects.push(Effect::GlobalEffect(GlobalEffect::Save {
                file_name: QUICK_SAVE_FILE.to_owned(),
            }));
        }
        if state.just_triggered(InputAction::QuickLoad) {
            effects.push(Effect::GlobalEffect(GlobalEffect::Load {
                file_name: QUICK_SAVE_FILE.to_owned(),
            }));
        }
        if state.just_triggered(InputAction::SpawnDebugItem) {
            effects.push(Effect::SpawnInFrontOfPlayer {
                template_id: DEBUG_SPAWN_TEMPLATE_ID,
                head_rotation: input_context.head.rotation,
            });
        }
        if state.just_triggered(InputAction::MoveInventory) {
            effects.push(Effect::PositionInventoryRelativeToPlayer {
                head_rotation: input_context.head.rotation,
            });
        }
        if state.just_triggered(InputAction::DebugHitboxCyclePose) {
            effects.push(Effect::DebugCycleHitboxPose);
        }
        if state.just_triggered(InputAction::CycleWeapon) {
            effects.push(Effect::DebugCycleWeapon {
                head_rotation: input_context.head.rotation,
            });
        }

        effects
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_actions_produces_no_effects() {
        let state = InputActionState::new();
        let effects = ActionDispatcher::dispatch(&state, &InputContext::default());
        assert!(effects.is_empty());
    }

    #[test]
    fn pathfinding_cycle_maps_to_pathfinding_test_effect() {
        let mut state = InputActionState::new();
        state.trigger(InputAction::PathfindingTestCycle);

        let effects = ActionDispatcher::dispatch(&state, &InputContext::default());
        assert_eq!(effects.len(), 1);
        assert!(matches!(effects[0], Effect::PathfindingTest));
    }

    #[test]
    fn quick_save_maps_to_save_global_effect() {
        let mut state = InputActionState::new();
        state.trigger(InputAction::QuickSave);

        let effects = ActionDispatcher::dispatch(&state, &InputContext::default());
        assert!(matches!(
            &effects[0],
            Effect::GlobalEffect(GlobalEffect::Save { .. })
        ));
    }

    #[test]
    fn spawn_debug_item_carries_head_rotation() {
        let mut state = InputActionState::new();
        state.trigger(InputAction::SpawnDebugItem);

        let input_context = InputContext::default();
        let effects = ActionDispatcher::dispatch(&state, &input_context);
        match &effects[0] {
            Effect::SpawnInFrontOfPlayer { head_rotation, .. } => {
                assert_eq!(*head_rotation, input_context.head.rotation);
            }
            other => panic!("expected SpawnInFrontOfPlayer, got {:?}", other),
        }
    }

    #[test]
    fn held_but_not_triggered_produces_no_effects() {
        let mut state = InputActionState::new();
        state.trigger(InputAction::QuickSave);
        state.clear_triggered();

        let effects = ActionDispatcher::dispatch(&state, &InputContext::default());
        assert!(effects.is_empty());
    }
}
