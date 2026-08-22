use glfw::{Action, Key, Window};
use shock2vr::input::{InputAction, InputActionState};
use std::collections::HashSet;

/// Modifier requirement for a binding. `None` requires Alt NOT held (so a
/// plain-key binding doesn't also fire during a system chord like Alt+Tab);
/// `Alt` requires it held.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Modifier {
    None,
    Alt,
}

struct Binding {
    key: Key,
    modifier: Modifier,
    action: InputAction,
}

/// Maps polled keyboard state to discrete input actions.
///
/// The desktop runtime polls key state every frame (rather than consuming
/// key events), so the mapper tracks which bindings were down last frame
/// and triggers actions only on rising edges.
pub struct DesktopInputMapper {
    bindings: Vec<Binding>,
    prev_down: HashSet<InputAction>,
}

impl DesktopInputMapper {
    pub fn new() -> Self {
        let bindings = vec![
            Binding {
                key: Key::P,
                modifier: Modifier::None,
                action: InputAction::PathfindingTestCycle,
            },
            // I toggles use mode like Tab: the key kept its inventory
            // meaning when the world-quad backpack (MoveInventory) was
            // replaced by the use-mode toggle.
            Binding {
                key: Key::I,
                modifier: Modifier::None,
                action: InputAction::ToggleUseMode,
            },
            Binding {
                key: Key::B,
                modifier: Modifier::None,
                action: InputAction::DebugCycleWeapon,
            },
            // Original System Shock 2 direct weapon bindings. These select a
            // matching carried item; unlike DebugCycleWeapon they never spawn one.
            Binding {
                key: Key::Num1,
                modifier: Modifier::None,
                action: InputAction::EquipWrench,
            },
            Binding {
                key: Key::Num2,
                modifier: Modifier::None,
                action: InputAction::EquipPistol,
            },
            Binding {
                key: Key::Num3,
                modifier: Modifier::None,
                action: InputAction::EquipShotgun,
            },
            Binding {
                key: Key::Num4,
                modifier: Modifier::None,
                action: InputAction::EquipAssaultRifle,
            },
            Binding {
                key: Key::Num5,
                modifier: Modifier::None,
                action: InputAction::EquipLaserPistol,
            },
            Binding {
                key: Key::Num6,
                modifier: Modifier::None,
                action: InputAction::EquipEmpRifle,
            },
            Binding {
                key: Key::Num7,
                modifier: Modifier::None,
                action: InputAction::EquipElectroShock,
            },
            Binding {
                key: Key::Num8,
                modifier: Modifier::None,
                action: InputAction::EquipGrenadeLauncher,
            },
            Binding {
                key: Key::Num9,
                modifier: Modifier::None,
                action: InputAction::EquipStasisFieldGenerator,
            },
            Binding {
                key: Key::Num0,
                modifier: Modifier::None,
                action: InputAction::EquipFusionCannon,
            },
            Binding {
                key: Key::Minus,
                modifier: Modifier::None,
                action: InputAction::EquipCrystalShard,
            },
            Binding {
                key: Key::Equal,
                modifier: Modifier::None,
                action: InputAction::EquipViralProliferator,
            },
            Binding {
                key: Key::Backslash,
                modifier: Modifier::None,
                action: InputAction::EquipWormLauncher,
            },
            Binding {
                key: Key::GraveAccent,
                modifier: Modifier::None,
                action: InputAction::EquipPsiAmp,
            },
            Binding {
                key: Key::R,
                modifier: Modifier::None,
                action: InputAction::Reload,
            },
            Binding {
                key: Key::T,
                modifier: Modifier::None,
                action: InputAction::CycleAmmo,
            },
            Binding {
                key: Key::Y,
                modifier: Modifier::None,
                action: InputAction::CyclePsiPower,
            },
            Binding {
                key: Key::S,
                modifier: Modifier::Alt,
                action: InputAction::QuickSave,
            },
            Binding {
                key: Key::L,
                modifier: Modifier::Alt,
                action: InputAction::QuickLoad,
            },
            Binding {
                key: Key::Tab,
                modifier: Modifier::None,
                action: InputAction::ToggleUseMode,
            },
            // Escape opens/closes the in-game pause menu, as it does in the
            // original. It used to close the window outright (a raw glfw
            // handler in `main.rs`); the pause menu's "Quit to Main Menu" plus
            // the main menu's "Quit" is now the way out.
            Binding {
                key: Key::Escape,
                modifier: Modifier::None,
                action: InputAction::TogglePauseMenu,
            },
            // AI debug: make everything hunt the player (pinned) / calm down
            Binding {
                key: Key::G,
                modifier: Modifier::Alt,
                action: InputAction::DebugForceChase,
            },
            Binding {
                key: Key::C,
                modifier: Modifier::Alt,
                action: InputAction::DebugCalmAll,
            },
            Binding {
                key: Key::U,
                modifier: Modifier::None,
                action: InputAction::ReadLastUnreadLog,
            },
            Binding {
                key: Key::M,
                modifier: Modifier::None,
                action: InputAction::ToggleMap,
            },
        ];

        Self {
            bindings,
            prev_down: HashSet::new(),
        }
    }

    /// Call once per frame. Polls bound keys and triggers actions on
    /// rising edges; releases them when the keys come back up.
    pub fn poll(&mut self, window: &Window, state: &mut InputActionState) {
        let is_alt_pressed = window.get_key(Key::LeftAlt) == Action::Press
            || window.get_key(Key::RightAlt) == Action::Press;

        for binding in &self.bindings {
            let modifier_satisfied = match binding.modifier {
                Modifier::None => !is_alt_pressed,
                Modifier::Alt => is_alt_pressed,
            };
            let is_down = modifier_satisfied && window.get_key(binding.key) == Action::Press;
            let was_down = self.prev_down.contains(&binding.action);

            if is_down && !was_down {
                state.trigger(binding.action);
                self.prev_down.insert(binding.action);
            } else if !is_down && was_down {
                state.release(binding.action);
                self.prev_down.remove(&binding.action);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn original_weapon_keys_map_to_direct_equip_actions() {
        let mapper = DesktopInputMapper::new();
        let expected = [
            (Key::Num1, InputAction::EquipWrench),
            (Key::Num2, InputAction::EquipPistol),
            (Key::Num3, InputAction::EquipShotgun),
            (Key::Num4, InputAction::EquipAssaultRifle),
            (Key::Num5, InputAction::EquipLaserPistol),
            (Key::Num6, InputAction::EquipEmpRifle),
            (Key::Num7, InputAction::EquipElectroShock),
            (Key::Num8, InputAction::EquipGrenadeLauncher),
            (Key::Num9, InputAction::EquipStasisFieldGenerator),
            (Key::Num0, InputAction::EquipFusionCannon),
            (Key::Minus, InputAction::EquipCrystalShard),
            (Key::Equal, InputAction::EquipViralProliferator),
            (Key::Backslash, InputAction::EquipWormLauncher),
            (Key::GraveAccent, InputAction::EquipPsiAmp),
        ];

        for (key, action) in expected {
            let binding = mapper
                .bindings
                .iter()
                .find(|binding| binding.key == key && binding.modifier == Modifier::None);
            assert_eq!(binding.map(|binding| binding.action), Some(action));
        }
    }
}
