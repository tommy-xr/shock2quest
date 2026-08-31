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

    /// Toggle the in-game pause menu. Unlike every other action here this one
    /// is consumed by `Game` itself rather than turned into an `Effect`: the
    /// pause overlay lives above the active scene (so missions and debug
    /// scenes alike get it), and while paused the scene's `update` is skipped -
    /// an effect routed through the scene could never close the menu again.
    TogglePauseMenu,

    /// Toggle the detached debug ("free") camera. Like `TogglePauseMenu`
    /// this is consumed by `Game` itself rather than becoming an `Effect`:
    /// the free camera is render-layer state that deliberately leaves the
    /// simulation untouched (the pawn stays put, AI keeps reading its
    /// position), so routing it through the world would be both wrong and
    /// unnecessary. Ignored unless the `free_camera` dev param is on, so a
    /// stray press during normal play cannot detach the view.
    ToggleFreeCamera,

    /// Toggle the developer cheat pad. Consumed by `Game` itself for the same
    /// reason `TogglePauseMenu` is: the pad is a `Game`-owned overlay that
    /// suspends the scene, so an effect routed through the scene could never
    /// close it again. Inert unless the `cheats` developer option is on.
    ToggleCheatPad,

    /// Open/play the newest unread audio log (the original's
    /// `play_unread_log`), or replay the newest collected log once all are read.
    /// Collecting a disc only files it in the PDA, so this is how it is read.
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
            InputAction::TogglePauseMenu,
            InputAction::ToggleFreeCamera,
            InputAction::ToggleCheatPad,
        ]
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            InputAction::PathfindingTestCycle => "PathfindingTestCycle",
            InputAction::QuickSave => "QuickSave",
            InputAction::QuickLoad => "QuickLoad",
            InputAction::SpawnDebugItem => "SpawnDebugItem",
            InputAction::SpawnDebugMonster => "SpawnDebugMonster",
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
            InputAction::TogglePauseMenu => "TogglePauseMenu",
            InputAction::ToggleFreeCamera => "ToggleFreeCamera",
            InputAction::ToggleCheatPad => "ToggleCheatPad",
        }
    }

    /// Production Meta Quest Touch binding for the player-owned panels and the
    /// weapon-handling actions.
    /// Keeping these paths beside their semantic actions makes the Oculus
    /// mapping host-testable even though that runtime only compiles for Android.
    pub fn quest_touch_click_path(&self) -> Option<&'static str> {
        match self {
            InputAction::ReadLastUnreadLog => Some("/user/hand/left/input/y/click"),
            // Left X toggles the cyber interface (use mode) - the binding the
            // removed world-quad backpack (MoveInventory) used to own.
            InputAction::ToggleUseMode => Some("/user/hand/left/input/x/click"),
            // The right controller's menu button is reserved by the Quest
            // system UI; the left one is the app's.
            InputAction::TogglePauseMenu => Some("/user/hand/left/input/menu/click"),
            // The right controller holds the gun, so its two face buttons own
            // the two gun-handling actions - A reloads, B swaps ammo type.
            InputAction::Reload => Some("/user/hand/right/input/a/click"),
            InputAction::CycleAmmo => Some("/user/hand/right/input/b/click"),
            _ => None,
        }
    }

    /// Production Meta Quest Touch binding for actions reached by a two-button
    /// **chord** rather than a button of their own.
    ///
    /// The Touch has no button left to give: `X`, `Y` and the left `Menu` are
    /// taken, both thumbstick clicks are jump and crouch, right `A` and `B`
    /// are reload and swap-ammo, and the right `Menu` belongs to the Quest
    /// system UI. So a debug toggle *shares* a pair rather than owning one -
    /// here right `A`+`B`, whose own actions only matter with a weapon in
    /// hand. The pair's edge is resolved by [`InputActionState::sync_chord`].
    ///
    /// Sharing is safe because the chord is dead unless the free camera's
    /// developer option is on. While it IS on, `oculus_runtime` suppresses
    /// whichever button is pressed *second*, so completing the chord cannot
    /// also swap your ammo; the first press still fires its own action, which
    /// is why the harmless one (`Reload`) is the natural opener.
    ///
    /// [`InputActionState::sync_chord`]: crate::input::InputActionState::sync_chord
    pub fn quest_touch_chord_paths(&self) -> Option<(&'static str, &'static str)> {
        match self {
            InputAction::ToggleFreeCamera => Some((
                "/user/hand/right/input/a/click",
                "/user/hand/right/input/b/click",
            )),
            // The cheat pad takes the LEFT controller's face pair, because the
            // right one is already the free camera's. Same sharing argument:
            // the chord is dead unless the `cheats` developer option is on,
            // and `Game` additionally refuses it while the left hand is
            // holding something - the state in which X (the cyber interface)
            // and Y (play last log) are worth reaching for on their own.
            InputAction::ToggleCheatPad => Some((
                "/user/hand/left/input/x/click",
                "/user/hand/left/input/y/click",
            )),
            _ => None,
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
    fn quest_touch_assigns_x_to_use_mode_and_y_to_the_log_reader() {
        assert_eq!(
            InputAction::ToggleUseMode.quest_touch_click_path(),
            Some("/user/hand/left/input/x/click")
        );
        assert_eq!(
            InputAction::ReadLastUnreadLog.quest_touch_click_path(),
            Some("/user/hand/left/input/y/click")
        );
        assert_eq!(
            InputAction::TogglePauseMenu.quest_touch_click_path(),
            Some("/user/hand/left/input/menu/click")
        );
    }

    /// The free camera is a chord, not a button, and must never quietly
    /// become one: a single-path binding here would consume a face button the
    /// Touch does not have to spare.
    ///
    /// Its two halves deliberately DO collide with `Reload` and `CycleAmmo`
    /// (#1144) - there is no unbound button left - which is exactly why
    /// `oculus_runtime` suppresses the second press while the developer
    /// option is on. Pinning that here means a future rebinding of either gun
    /// action has to come back and re-read the suppression rule rather than
    /// silently making the chord fire both.
    #[test]
    fn the_free_camera_chord_shares_the_two_gun_buttons() {
        assert_eq!(InputAction::ToggleFreeCamera.quest_touch_click_path(), None);
        let (first, second) = InputAction::ToggleFreeCamera
            .quest_touch_chord_paths()
            .expect("free camera chord binding");
        assert_eq!(first, InputAction::Reload.quest_touch_click_path().unwrap());
        assert_eq!(
            second,
            InputAction::CycleAmmo.quest_touch_click_path().unwrap()
        );
        assert_eq!(first, "/user/hand/right/input/a/click");
        assert_eq!(second, "/user/hand/right/input/b/click");
    }

    /// The cheat pad shares the LEFT face pair, because the right one is
    /// already the free camera's. Pinning both chords here means a future
    /// rebinding of either has to come back and re-read `oculus_runtime`'s
    /// suppression rule rather than silently making one chord fire the other.
    #[test]
    fn the_cheat_pad_chord_shares_the_two_left_face_buttons() {
        assert_eq!(InputAction::ToggleCheatPad.quest_touch_click_path(), None);
        let (first, second) = InputAction::ToggleCheatPad
            .quest_touch_chord_paths()
            .expect("cheat pad chord binding");
        assert_eq!(
            first,
            InputAction::ToggleUseMode.quest_touch_click_path().unwrap()
        );
        assert_eq!(
            second,
            InputAction::ReadLastUnreadLog
                .quest_touch_click_path()
                .unwrap()
        );
        // ...and it is a different pair from the free camera's, so completing
        // one chord can never also complete the other.
        assert_ne!(
            InputAction::ToggleCheatPad.quest_touch_chord_paths(),
            InputAction::ToggleFreeCamera.quest_touch_chord_paths()
        );
    }

    /// Every action is reachable by exactly one kind of binding, or none.
    #[test]
    fn no_action_is_both_a_button_and_a_chord() {
        for action in InputAction::all() {
            assert!(
                action.quest_touch_click_path().is_none()
                    || action.quest_touch_chord_paths().is_none(),
                "{action}"
            );
        }
    }

    #[test]
    fn quest_touch_assigns_the_right_face_buttons_to_gun_handling() {
        assert_eq!(
            InputAction::Reload.quest_touch_click_path(),
            Some("/user/hand/right/input/a/click")
        );
        assert_eq!(
            InputAction::CycleAmmo.quest_touch_click_path(),
            Some("/user/hand/right/input/b/click")
        );
    }

    #[test]
    fn quest_touch_click_paths_are_unique() {
        let mut paths: Vec<&'static str> = InputAction::all()
            .iter()
            .filter_map(|action| action.quest_touch_click_path())
            .collect();
        let total = paths.len();
        paths.sort_unstable();
        paths.dedup();
        assert_eq!(paths.len(), total, "two actions share a Quest binding");
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
