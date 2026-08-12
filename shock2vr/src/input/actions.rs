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

    /// Spawn and wield the next debug weapon, holstering the previous one.
    /// Cycles the full SS2 weapon roster for flat-mode aim/viewmodel testing.
    DebugCycleWeapon,
    /// Equip the matching weapon from the player's carried inventory. These
    /// mirror System Shock 2's original direct weapon bindings; they never
    /// create a weapon or drop the displaced weapon into the world.
    EquipWrench,
    EquipPistol,
    EquipShotgun,
    EquipAssaultRifle,
    EquipLaserPistol,
    EquipEmpRifle,
    EquipElectroShock,
    EquipGrenadeLauncher,
    EquipStasisFieldGenerator,
    EquipFusionCannon,
    EquipCrystalShard,
    EquipViralProliferator,
    EquipWormLauncher,
    EquipPsiAmp,
    /// Cycle an empty wielded weapon's ammo type (its next Projectile link).
    CycleAmmo,
    /// Reload the wielded weapon from compatible backpack reserve.
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
    /// DebugAlertAll. Also clears a DebugForceChase pin.
    DebugCalmAll,
    /// Like DebugAlertAll, but PINNED: alertness never decays and every
    /// monster keeps hunting the player's live position until DebugCalmAll.
    /// The demo cheat - "everything on the map converges on the player".
    DebugForceChase,

    /// Toggle the flat-mode "use" (metagame) mode: cursor-driven UI over the
    /// 3D view, as the original game does on Tab. Shooter mode is restored on
    /// a second toggle. No-op in VR. See `projects/flat-ui.md`.
    ToggleUseMode,

    /// Toggle the flat-mode automap panel (the original's BIOFULL MAP button /
    /// `M` key). No-op in VR. See `projects/flat-ui-panels.md` §5.
    ToggleMap,

    /// Play back the most recently collected audio log the player has not read
    /// yet, opening the reader on it (the original's `play_unread_log`).
    /// Collecting a disc only files it in the PDA, so this is how a log is
    /// actually read.
    ReadLastUnreadLog,
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
            InputAction::DebugCycleWeapon,
            InputAction::EquipWrench,
            InputAction::EquipPistol,
            InputAction::EquipShotgun,
            InputAction::EquipAssaultRifle,
            InputAction::EquipLaserPistol,
            InputAction::EquipEmpRifle,
            InputAction::EquipElectroShock,
            InputAction::EquipGrenadeLauncher,
            InputAction::EquipStasisFieldGenerator,
            InputAction::EquipFusionCannon,
            InputAction::EquipCrystalShard,
            InputAction::EquipViralProliferator,
            InputAction::EquipWormLauncher,
            InputAction::EquipPsiAmp,
            InputAction::CycleAmmo,
            InputAction::Reload,
            InputAction::CyclePsiPower,
            InputAction::DebugReloadLevel,
            InputAction::DebugAlertAll,
            InputAction::DebugCalmAll,
            InputAction::DebugForceChase,
            InputAction::ToggleUseMode,
            InputAction::ReadLastUnreadLog,
            InputAction::ToggleMap,
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
            InputAction::DebugCycleWeapon => "DebugCycleWeapon",
            InputAction::EquipWrench => "EquipWrench",
            InputAction::EquipPistol => "EquipPistol",
            InputAction::EquipShotgun => "EquipShotgun",
            InputAction::EquipAssaultRifle => "EquipAssaultRifle",
            InputAction::EquipLaserPistol => "EquipLaserPistol",
            InputAction::EquipEmpRifle => "EquipEmpRifle",
            InputAction::EquipElectroShock => "EquipElectroShock",
            InputAction::EquipGrenadeLauncher => "EquipGrenadeLauncher",
            InputAction::EquipStasisFieldGenerator => "EquipStasisFieldGenerator",
            InputAction::EquipFusionCannon => "EquipFusionCannon",
            InputAction::EquipCrystalShard => "EquipCrystalShard",
            InputAction::EquipViralProliferator => "EquipViralProliferator",
            InputAction::EquipWormLauncher => "EquipWormLauncher",
            InputAction::EquipPsiAmp => "EquipPsiAmp",
            InputAction::CycleAmmo => "CycleAmmo",
            InputAction::Reload => "Reload",
            InputAction::CyclePsiPower => "CyclePsiPower",
            InputAction::DebugReloadLevel => "DebugReloadLevel",
            InputAction::DebugAlertAll => "DebugAlertAll",
            InputAction::DebugCalmAll => "DebugCalmAll",
            InputAction::DebugForceChase => "DebugForceChase",
            InputAction::ToggleUseMode => "ToggleUseMode",
            InputAction::ReadLastUnreadLog => "ReadLastUnreadLog",
            InputAction::ToggleMap => "ToggleMap",
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
    fn spawning_weapon_cycle_requires_an_explicit_debug_name() {
        assert!("CycleWeapon".parse::<InputAction>().is_err());
        assert_eq!(
            "DebugCycleWeapon".parse::<InputAction>().unwrap(),
            InputAction::DebugCycleWeapon
        );
    }

    #[test]
    fn serde_roundtrips() {
        let json = serde_json::to_string(&InputAction::PathfindingTestCycle).unwrap();
        assert_eq!(json, "\"PathfindingTestCycle\"");
        let parsed: InputAction = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, InputAction::PathfindingTestCycle);
    }
}
