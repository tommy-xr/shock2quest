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
}
