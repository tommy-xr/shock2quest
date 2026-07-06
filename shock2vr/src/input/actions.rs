use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// Discrete actions triggered by button presses (edge-triggered).
///
/// NOTE: Only non-contextual actions belong here. Hand interactions
/// (trigger pull, grab, use, drop) are contextual - they depend on
/// game state (what's held, what's nearby) and are handled by
/// VirtualHand, which reads InputContext directly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum InputAction {
    /// Cycle the pathfinding test state (set start -> set goal -> show path -> reset)
    PathfindingTestCycle,

    /// Save the game to the quick-save slot
    QuickSave,

    /// Load the game from the quick-save slot
    QuickLoad,

    /// Spawn a debug item in front of the player
    SpawnDebugItem,

    /// Spawn a debug monster (og-pipe hybrid) in front of the player
    SpawnDebugMonster,

    /// Reposition the inventory in front of the player
    MoveInventory,

    /// Cycle creatures to their next animation pose (debug hitbox/ragdoll inspection)
    DebugHitboxCyclePose,

    /// Wield the next player weapon (debug: spawn-and-wield, dropping the
    /// previous). Cycles the full SS2 weapon roster for flat-mode aim/viewmodel
    /// testing.
    CycleWeapon,
    /// Cycle the wielded weapon's ammo type (its next Projectile link).
    CycleAmmo,
    /// Reload the wielded weapon: refill its clip to capacity.
    Reload,
    /// Select the next psi power (used when firing the psi amp).
    CyclePsiPower,
    /// Reload the current level in place (debug). Exercises the level-transition path,
    /// including the experimental loading screen.
    DebugReloadLevel,

    /// Force every monster to Moderate alertness (chase) - debug "make them
    /// angry". A forced level is a behavior reset: it also cancels scripted
    /// sequences (which won't restart). Dead AIs are unaffected; cameras and
    /// turrets manage their own alertness and ignore this.
    DebugAlertAll,
    /// Force every monster back to Lowest alertness (idle). Same caveats as
    /// DebugAlertAll.
    DebugCalmAll,

    /// Enlist in the Marine (combat) service branch at the station recruit
    /// deck. Registers the career (persisted quest bit) so it applies on the
    /// next deployment. Mutually exclusive with the other two.
    SelectCareerMarine,
    /// Enlist in the Navy (tech) service branch. See `SelectCareerMarine`.
    SelectCareerNavy,
    /// Enlist in the OSA (psi) service branch. See `SelectCareerMarine`.
    SelectCareerOsa,
}

impl InputAction {
    /// All known actions, for enumeration (e.g. the debug runtime's
    /// `/v1/input/actions` endpoint).
    pub fn all() -> &'static [InputAction] {
        &[
            InputAction::PathfindingTestCycle,
            InputAction::QuickSave,
            InputAction::QuickLoad,
            InputAction::SpawnDebugItem,
            InputAction::SpawnDebugMonster,
            InputAction::MoveInventory,
            InputAction::DebugHitboxCyclePose,
            InputAction::CycleWeapon,
            InputAction::CycleAmmo,
            InputAction::Reload,
            InputAction::CyclePsiPower,
            InputAction::DebugReloadLevel,
            InputAction::DebugAlertAll,
            InputAction::DebugCalmAll,
            InputAction::SelectCareerMarine,
            InputAction::SelectCareerNavy,
            InputAction::SelectCareerOsa,
        ]
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            InputAction::PathfindingTestCycle => "PathfindingTestCycle",
            InputAction::QuickSave => "QuickSave",
            InputAction::QuickLoad => "QuickLoad",
            InputAction::SpawnDebugItem => "SpawnDebugItem",
            InputAction::SpawnDebugMonster => "SpawnDebugMonster",
            InputAction::MoveInventory => "MoveInventory",
            InputAction::DebugHitboxCyclePose => "DebugHitboxCyclePose",
            InputAction::CycleWeapon => "CycleWeapon",
            InputAction::CycleAmmo => "CycleAmmo",
            InputAction::Reload => "Reload",
            InputAction::CyclePsiPower => "CyclePsiPower",
            InputAction::DebugReloadLevel => "DebugReloadLevel",
            InputAction::DebugAlertAll => "DebugAlertAll",
            InputAction::DebugCalmAll => "DebugCalmAll",
            InputAction::SelectCareerMarine => "SelectCareerMarine",
            InputAction::SelectCareerNavy => "SelectCareerNavy",
            InputAction::SelectCareerOsa => "SelectCareerOsa",
        }
    }
}

impl fmt::Display for InputAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownActionError(pub String);

impl fmt::Display for UnknownActionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "unknown input action: {}", self.0)
    }
}

impl std::error::Error for UnknownActionError {}

impl FromStr for InputAction {
    type Err = UnknownActionError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        InputAction::all()
            .iter()
            .find(|action| action.as_str().eq_ignore_ascii_case(s))
            .copied()
            .ok_or_else(|| UnknownActionError(s.to_owned()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_str_roundtrips_all_actions() {
        for action in InputAction::all() {
            let parsed: InputAction = action.as_str().parse().unwrap();
            assert_eq!(parsed, *action);
        }
    }

    #[test]
    fn from_str_is_case_insensitive() {
        let parsed: InputAction = "quicksave".parse().unwrap();
        assert_eq!(parsed, InputAction::QuickSave);
    }

    #[test]
    fn from_str_rejects_unknown_action() {
        let result = "NotARealAction".parse::<InputAction>();
        assert!(result.is_err());
    }

    #[test]
    fn serde_roundtrips() {
        let json = serde_json::to_string(&InputAction::PathfindingTestCycle).unwrap();
        assert_eq!(json, "\"PathfindingTestCycle\"");
        let parsed: InputAction = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, InputAction::PathfindingTestCycle);
    }
}
