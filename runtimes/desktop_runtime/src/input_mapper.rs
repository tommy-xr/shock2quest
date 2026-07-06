use glfw::{Action, Key, Window};
use shock2vr::input::{InputAction, InputActionState};
use std::collections::HashSet;

/// Whether a binding requires the Alt modifier to be held.
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
            Binding {
                key: Key::Space,
                modifier: Modifier::None,
                action: InputAction::SpawnDebugItem,
            },
            Binding {
                key: Key::I,
                modifier: Modifier::None,
                action: InputAction::MoveInventory,
            },
            Binding {
                key: Key::B,
                modifier: Modifier::None,
                action: InputAction::CycleWeapon,
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
                Modifier::None => true,
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
