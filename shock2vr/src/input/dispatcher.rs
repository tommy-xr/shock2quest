use crate::input_context::InputContext;
use crate::scripts::{Effect, GlobalEffect};
use dark::properties::AIAlertLevel;

use super::{InputAction, InputActionState};

/// Template spawned by InputAction::SpawnDebugItem (Pistol).
/// Other useful debug templates: Laser: -22, Wrench: -928,
/// grunt og-pipe: -397, monkey - red: -1432
const DEBUG_SPAWN_TEMPLATE_ID: i32 = -17;

/// Template spawned by InputAction::SpawnDebugMonster (grunt og-pipe)
const DEBUG_MONSTER_TEMPLATE_ID: i32 = -397;

/// Bare name of the quick save slot. It resolves through
/// [`crate::save_file_path`] so quicksaves land beside every other named save
/// in `<data_root>/saves`, which is the directory the load/game-over screens
/// search for a save to offer.
const QUICK_SAVE_NAME: &str = "save1";

fn quick_save_file() -> String {
    crate::save_file_path(QUICK_SAVE_NAME)
        .to_string_lossy()
        .into_owned()
}

/// Original System Shock 2 direct-weapon bindings. The input action describes
/// the semantic selection; desktop keys are mapped separately by
/// `DesktopInputMapper`.
const CARRIED_WEAPON_ACTIONS: &[(InputAction, i32)] = &[
    (InputAction::EquipWrench, -928),
    (InputAction::EquipPistol, -17),
    (InputAction::EquipShotgun, -19),
    (InputAction::EquipAssaultRifle, -18),
    (InputAction::EquipLaserPistol, -22),
    (InputAction::EquipEmpRifle, -23),
    (InputAction::EquipElectroShock, -24),
    (InputAction::EquipGrenadeLauncher, -21),
    (InputAction::EquipStasisFieldGenerator, -25),
    (InputAction::EquipFusionCannon, -26),
    (InputAction::EquipCrystalShard, -28),
    (InputAction::EquipViralProliferator, -29),
    (InputAction::EquipWormLauncher, -27),
    (InputAction::EquipPsiAmp, -247),
];

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
                file_name: quick_save_file(),
            }));
        }
        if state.just_triggered(InputAction::QuickLoad) {
            effects.push(Effect::GlobalEffect(GlobalEffect::Load {
                file_name: quick_save_file(),
            }));
        }
        if state.just_triggered(InputAction::SpawnDebugItem) {
            effects.push(Effect::SpawnInFrontOfPlayer {
                template_id: DEBUG_SPAWN_TEMPLATE_ID,
                head_rotation: input_context.head.rotation,
                auto_wield: true,
            });
        }
        if state.just_triggered(InputAction::SpawnDebugMonster) {
            effects.push(Effect::SpawnInFrontOfPlayer {
                template_id: DEBUG_MONSTER_TEMPLATE_ID,
                head_rotation: input_context.head.rotation,
                auto_wield: false,
            });
        }
        if state.just_triggered(InputAction::DebugHitboxCyclePose) {
            effects.push(Effect::DebugCycleHitboxPose);
        }
        if state.just_triggered(InputAction::DebugCycleWeapon) {
            effects.push(Effect::DebugCycleWeapon {
                head_rotation: input_context.head.rotation,
            });
        }
        for &(action, class_template_id) in CARRIED_WEAPON_ACTIONS {
            if state.just_triggered(action) {
                effects.push(Effect::EquipCarriedWeapon { class_template_id });
            }
        }
        if state.just_triggered(InputAction::CycleAmmo) {
            effects.push(Effect::CycleAmmo);
        }
        if state.just_triggered(InputAction::CycleGunSetting) {
            effects.push(Effect::CycleGunSetting);
        }
        if state.just_triggered(InputAction::Reload) {
            effects.push(Effect::ReloadWeapon);
        }
        if state.just_triggered(InputAction::CyclePsiPower) {
            effects.push(Effect::StepPsiSelection {
                axis: crate::psi::PsiSelectionAxis::Any,
                forward: true,
            });
        }
        if state.just_triggered(InputAction::DebugReloadLevel) {
            effects.push(Effect::GlobalEffect(GlobalEffect::TestReload));
        }
        if state.just_triggered(InputAction::DebugAlertAll) {
            // Moderate = chase; High maps to attack behaviors, which assume
            // the player is already in range
            effects.push(Effect::SetAllAIAlertness {
                level: AIAlertLevel::Moderate,
                pin: false,
            });
        }
        if state.just_triggered(InputAction::DebugCalmAll) {
            effects.push(Effect::SetAllAIAlertness {
                level: AIAlertLevel::Lowest,
                pin: false,
            });
        }
        if state.just_triggered(InputAction::DebugForceChase) {
            // Pinned: never decays, and every AI keeps hunting the player's
            // live position until DebugCalmAll clears the pin
            effects.push(Effect::SetAllAIAlertness {
                level: AIAlertLevel::Moderate,
                pin: true,
            });
        }
        if state.just_triggered(InputAction::ToggleUseMode) {
            effects.push(Effect::ToggleUseMode);
        }
        if state.just_triggered(InputAction::ToggleMap) {
            effects.push(Effect::ToggleMap);
        }
        if state.just_triggered(InputAction::ReadLastUnreadLog) {
            effects.push(Effect::ReadLastUnreadLog {
                head_rotation: input_context.head.rotation,
            });
        }
        // `InputAction::TogglePauseMenu` deliberately produces no effect: the
        // pause overlay is owned by `Game`, which reads the action directly
        // before dispatching. Routing it through the scene would make it
        // undeliverable exactly when it matters, because a paused scene is
        // not updated at all.
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
    fn audio_log_reader_carries_the_current_head_rotation() {
        use cgmath::{Deg, Rotation3};

        let mut state = InputActionState::new();
        state.trigger(InputAction::ReadLastUnreadLog);
        let mut input = InputContext::default();
        input.head.rotation = cgmath::Quaternion::from_angle_y(Deg(37.0));

        let effects = ActionDispatcher::dispatch(&state, &input);

        assert!(matches!(
            effects.as_slice(),
            [Effect::ReadLastUnreadLog { head_rotation }] if *head_rotation == input.head.rotation
        ));
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
    fn alert_all_maps_to_moderate_broadcast() {
        let mut state = InputActionState::new();
        state.trigger(InputAction::DebugAlertAll);

        let effects = ActionDispatcher::dispatch(&state, &InputContext::default());
        assert!(matches!(
            effects[0],
            Effect::SetAllAIAlertness {
                level: AIAlertLevel::Moderate,
                pin: false
            }
        ));
    }

    #[test]
    fn original_weapon_actions_select_their_carried_archetypes() {
        for &(action, class_template_id) in CARRIED_WEAPON_ACTIONS {
            let mut state = InputActionState::new();
            state.trigger(action);

            let effects = ActionDispatcher::dispatch(&state, &InputContext::default());
            assert!(matches!(
                effects.as_slice(),
                [Effect::EquipCarriedWeapon {
                    class_template_id: actual
                }] if *actual == class_template_id
            ));
        }
    }

    #[test]
    fn debug_weapon_cycle_maps_to_the_spawning_effect() {
        let mut state = InputActionState::new();
        state.trigger(InputAction::DebugCycleWeapon);

        let effects = ActionDispatcher::dispatch(&state, &InputContext::default());
        assert!(matches!(
            effects.as_slice(),
            [Effect::DebugCycleWeapon { .. }]
        ));
    }

    #[test]
    fn force_chase_maps_to_pinned_moderate_broadcast() {
        let mut state = InputActionState::new();
        state.trigger(InputAction::DebugForceChase);

        let effects = ActionDispatcher::dispatch(&state, &InputContext::default());
        assert!(matches!(
            effects[0],
            Effect::SetAllAIAlertness {
                level: AIAlertLevel::Moderate,
                pin: true
            }
        ));
    }

    #[test]
    fn calm_all_maps_to_lowest_broadcast() {
        let mut state = InputActionState::new();
        state.trigger(InputAction::DebugCalmAll);

        let effects = ActionDispatcher::dispatch(&state, &InputContext::default());
        assert!(matches!(
            effects[0],
            Effect::SetAllAIAlertness {
                level: AIAlertLevel::Lowest,
                pin: false
            }
        ));
    }

    #[test]
    fn toggle_use_mode_maps_to_toggle_effect() {
        let mut state = InputActionState::new();
        state.trigger(InputAction::ToggleUseMode);

        let effects = ActionDispatcher::dispatch(&state, &InputContext::default());
        assert!(matches!(effects[0], Effect::ToggleUseMode));
    }

    #[test]
    fn toggle_map_maps_to_toggle_map_effect() {
        let mut state = InputActionState::new();
        state.trigger(InputAction::ToggleMap);

        let effects = ActionDispatcher::dispatch(&state, &InputContext::default());
        assert!(matches!(effects[0], Effect::ToggleMap));
    }

    #[test]
    fn toggle_pause_menu_produces_no_scene_effect() {
        // `Game` consumes this one itself; if it ever became an effect the
        // scene would swallow it while paused and the menu could not close.
        let mut state = InputActionState::new();
        state.trigger(InputAction::TogglePauseMenu);

        let effects = ActionDispatcher::dispatch(&state, &InputContext::default());
        assert!(effects.is_empty());
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
