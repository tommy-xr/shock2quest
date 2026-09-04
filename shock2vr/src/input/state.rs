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

    /// Drop an action entirely for this frame: neither triggered nor held.
    ///
    /// For a button that is bound to two things at once and must not fire
    /// both - the free-camera chord's halves, which are also the right hand's
    /// contextual face buttons.
    pub fn suppress(&mut self, action: InputAction) {
        self.triggered.remove(&action);
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

    /// Feed a two-button chord into semantic input state: the action fires on
    /// the frame both buttons are down and does not fire again until at least
    /// one is released. The held set is the latch, so a chord needs no state
    /// of its own - and, unlike [`sync_discrete_button`], this takes the raw
    /// current states rather than OpenXR edges, because the chord's edge is a
    /// property of the *pair* and neither button's own edge implies it.
    ///
    /// `is_active` must be false unless BOTH halves are live. While the pair
    /// is inactive the latch is left exactly as it was and nothing is minted:
    /// with the session merely VISIBLE (a system overlay up) OpenXR reports
    /// `current_state` false even for a physically held button, so treating
    /// that as a release would re-arm the chord and fire a second, unpressed
    /// toggle the moment focus came back with both buttons still down. This
    /// is the same hazard the runtime's latched crouch guards against.
    ///
    /// Chords exist so a debug toggle can be reachable on a headset without
    /// consuming a face button that gameplay may want later: each half stays
    /// individually unbound.
    ///
    /// [`sync_discrete_button`]: Self::sync_discrete_button
    pub fn sync_chord(
        &mut self,
        action: InputAction,
        is_active: bool,
        first_down: bool,
        second_down: bool,
    ) {
        if !is_active {
            return;
        }
        let chord_down = first_down && second_down;
        if chord_down {
            if !self.is_held(action) {
                self.trigger(action);
            }
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

    /// The chord fires once, not every frame it is held - the defect a naive
    /// `a && b -> trigger` would have.
    #[test]
    fn a_chord_fires_once_per_press() {
        let mut state = InputActionState::new();

        state.sync_chord(InputAction::ToggleFreeCamera, true, true, true);
        assert!(state.just_triggered(InputAction::ToggleFreeCamera));

        // Still held on the next frame: no new edge.
        state.clear_triggered();
        state.sync_chord(InputAction::ToggleFreeCamera, true, true, true);
        assert!(!state.just_triggered(InputAction::ToggleFreeCamera));
        assert!(state.is_held(InputAction::ToggleFreeCamera));
    }

    /// Releasing either half re-arms it; neither half alone fires it.
    #[test]
    fn a_chord_rearms_when_either_half_releases() {
        let mut state = InputActionState::new();
        state.sync_chord(InputAction::ToggleFreeCamera, true, true, true);
        state.clear_triggered();

        state.sync_chord(InputAction::ToggleFreeCamera, true, true, false);
        assert!(!state.is_held(InputAction::ToggleFreeCamera));
        assert!(!state.just_triggered(InputAction::ToggleFreeCamera));

        state.sync_chord(InputAction::ToggleFreeCamera, true, true, true);
        assert!(state.just_triggered(InputAction::ToggleFreeCamera));
    }

    #[test]
    fn one_half_of_a_chord_never_fires_it() {
        let mut state = InputActionState::new();
        state.sync_chord(InputAction::ToggleFreeCamera, true, true, false);
        state.sync_chord(InputAction::ToggleFreeCamera, true, false, true);
        assert!(!state.just_triggered(InputAction::ToggleFreeCamera));
        assert!(!state.is_held(InputAction::ToggleFreeCamera));
    }

    /// Losing focus with the chord held must not mint a second toggle when
    /// focus returns: OpenXR reports a held button as `current_state` false
    /// while the action is inactive, so an unguarded chord would read that as
    /// a release, re-arm, and fire again on reactivation.
    #[test]
    fn an_inactive_chord_keeps_its_latch_and_mints_nothing() {
        let mut state = InputActionState::new();
        state.sync_chord(InputAction::ToggleFreeCamera, true, true, true);
        assert!(state.just_triggered(InputAction::ToggleFreeCamera));
        state.clear_triggered();

        // System overlay up: inactive, and OpenXR reports both as up.
        state.sync_chord(InputAction::ToggleFreeCamera, false, false, false);
        assert!(
            state.is_held(InputAction::ToggleFreeCamera),
            "the latch must survive an inactive frame"
        );
        assert!(!state.just_triggered(InputAction::ToggleFreeCamera));

        // Focus returns with both buttons still physically held.
        state.sync_chord(InputAction::ToggleFreeCamera, true, true, true);
        assert!(
            !state.just_triggered(InputAction::ToggleFreeCamera),
            "refocusing on a held chord must not toggle"
        );

        // A genuine release still re-arms it.
        state.sync_chord(InputAction::ToggleFreeCamera, true, false, false);
        state.sync_chord(InputAction::ToggleFreeCamera, true, true, true);
        assert!(state.just_triggered(InputAction::ToggleFreeCamera));
    }

    #[test]
    fn suppress_drops_both_the_edge_and_the_latch() {
        let mut state = InputActionState::new();
        state.trigger(InputAction::RightHandLowerButton);

        state.suppress(InputAction::RightHandLowerButton);

        assert!(!state.just_triggered(InputAction::RightHandLowerButton));
        assert!(!state.is_held(InputAction::RightHandLowerButton));
        // Only the named action - the chord it belongs to must survive.
        state.trigger(InputAction::ToggleFreeCamera);
        state.suppress(InputAction::RightHandUpperButton);
        assert!(state.just_triggered(InputAction::ToggleFreeCamera));
    }

    #[test]
    fn actions_are_independent() {
        let mut state = InputActionState::new();
        state.trigger(InputAction::QuickSave);
        assert!(!state.just_triggered(InputAction::QuickLoad));
        assert!(!state.is_held(InputAction::QuickLoad));
    }

    #[test]
    fn quest_x_use_mode_and_y_reader_edges_remain_independent() {
        let mut state = InputActionState::new();

        state.sync_discrete_button(InputAction::ToggleUseMode, true, true, true);
        assert!(state.just_triggered(InputAction::ToggleUseMode));
        assert!(!state.just_triggered(InputAction::ReadLastUnreadLog));
        state.clear_triggered();

        state.sync_discrete_button(InputAction::ReadLastUnreadLog, true, true, true);
        assert!(state.just_triggered(InputAction::ReadLastUnreadLog));
        assert!(state.is_held(InputAction::ToggleUseMode));

        state.sync_discrete_button(InputAction::ToggleUseMode, true, true, false);
        assert!(!state.is_held(InputAction::ToggleUseMode));
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
