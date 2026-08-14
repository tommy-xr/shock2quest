use std::collections::HashSet;

use super::InputAction;

/// Discrete actions triggered this frame.
///
/// Runtimes populate this from platform input (key presses, controller
/// buttons, HTTP injection) and pass it to `Game::update`, which converts
/// triggered actions into effects via the `ActionDispatcher`.
#[derive(Debug, Default, Clone)]
pub struct InputActionState {
    /// Actions triggered this frame (just pressed)
    triggered: HashSet<InputAction>,

    /// Actions currently held (for hold-to-activate patterns)
    held: HashSet<InputAction>,
}

impl InputActionState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Check if action was just triggered (rising edge)
    pub fn just_triggered(&self, action: InputAction) -> bool {
        self.triggered.contains(&action)
    }

    /// Check if action is currently held
    pub fn is_held(&self, action: InputAction) -> bool {
        self.held.contains(&action)
    }

    /// Trigger an action (for runtime mappers and debug injection)
    pub fn trigger(&mut self, action: InputAction) {
        self.triggered.insert(action);
        self.held.insert(action);
    }

    /// Clear triggered actions (call at end of frame)
    pub fn clear_triggered(&mut self) {
        self.triggered.clear();
    }

    /// Release a held action
    pub fn release(&mut self, action: InputAction) {
        self.held.remove(&action);
    }

    /// Feed one platform boolean action into semantic input state. OpenXR uses
    /// `is_active`/`changed_since_last_sync`; keeping that edge policy here
    /// makes Quest button mappings host-testable without compiling Android-only
    /// runtime dependencies.
    pub fn sync_discrete_button(
        &mut self,
        action: InputAction,
        is_active: bool,
        changed_since_last_sync: bool,
        current_state: bool,
    ) {
        if !is_active || !changed_since_last_sync {
            return;
        }
        if current_state {
            self.trigger(action);
        } else {
            self.release(action);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trigger_sets_triggered_and_held() {
        let mut state = InputActionState::new();
        assert!(!state.just_triggered(InputAction::QuickSave));
        assert!(!state.is_held(InputAction::QuickSave));

        state.trigger(InputAction::QuickSave);
        assert!(state.just_triggered(InputAction::QuickSave));
        assert!(state.is_held(InputAction::QuickSave));
    }

    #[test]
    fn clear_triggered_preserves_held() {
        let mut state = InputActionState::new();
        state.trigger(InputAction::QuickSave);

        state.clear_triggered();
        assert!(!state.just_triggered(InputAction::QuickSave));
        assert!(state.is_held(InputAction::QuickSave));
    }

    #[test]
    fn release_removes_held() {
        let mut state = InputActionState::new();
        state.trigger(InputAction::QuickSave);
        state.clear_triggered();

        state.release(InputAction::QuickSave);
        assert!(!state.is_held(InputAction::QuickSave));
    }

    #[test]
    fn actions_are_independent() {
        let mut state = InputActionState::new();
        state.trigger(InputAction::QuickSave);
        assert!(!state.just_triggered(InputAction::QuickLoad));
        assert!(!state.is_held(InputAction::QuickLoad));
    }

    #[test]
    fn quest_x_backpack_and_y_reader_edges_remain_independent() {
        let mut state = InputActionState::new();

        state.sync_discrete_button(InputAction::MoveInventory, true, true, true);
        assert!(state.just_triggered(InputAction::MoveInventory));
        assert!(!state.just_triggered(InputAction::ReadLastUnreadLog));
        state.clear_triggered();

        state.sync_discrete_button(InputAction::ReadLastUnreadLog, true, true, true);
        assert!(state.just_triggered(InputAction::ReadLastUnreadLog));
        assert!(state.is_held(InputAction::MoveInventory));

        state.sync_discrete_button(InputAction::MoveInventory, true, true, false);
        assert!(!state.is_held(InputAction::MoveInventory));
        assert!(state.is_held(InputAction::ReadLastUnreadLog));
    }

    #[test]
    fn inactive_or_unchanged_platform_buttons_do_not_mint_edges() {
        let mut state = InputActionState::new();
        state.sync_discrete_button(InputAction::ReadLastUnreadLog, false, true, true);
        state.sync_discrete_button(InputAction::ReadLastUnreadLog, true, false, true);

        assert!(!state.just_triggered(InputAction::ReadLastUnreadLog));
        assert!(!state.is_held(InputAction::ReadLastUnreadLog));
    }
}
