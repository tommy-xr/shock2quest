//! What a face button means on the hand that pressed it.
//!
//! The Touch controllers offer two face buttons per hand (left X/Y, right
//! A/B), which this module treats as one symmetric pair per hand: a **lower**
//! button (left X, right A) and an **upper** one (left Y, right B). What the
//! pair does depends on what that hand is holding, so a gun can carry its own
//! handling controls on the controller wielding it while the other hand keeps
//! the player-owned panels.
//!
//! Only the Quest binds these (`InputAction::quest_touch_click_path`); flat
//! has no face buttons and binds no key to them. That matters because in flat
//! the LEFT hand slot is where the controller wields, so a flat press would
//! read the wielded weapon as "the left hand's load".
//!
//! One exception lives outside the table: while the free-camera developer
//! option is on, `Game::update` suppresses the RIGHT hand's two buttons -
//! they are that toggle's chord, and a chord is pressed one button at a time.
//!
//! Resolution is pure ([`resolve_hand_button`]) and the world lookup that
//! feeds it is one function ([`held_kind_in_hand`]), so the whole table is
//! host-testable. This is contextual input - it reads game state - so it lives
//! beside `VirtualHand` rather than in `ActionDispatcher`, which only handles
//! actions whose meaning never changes.

use dark::properties::PropBaseGunDesc;
use shipyard::{Get, View, World};

use crate::{input::InputAction, vr_config::Handedness};

/// Which of a hand's two face buttons was pressed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HandButton {
    /// Left X / right A.
    Lower,
    /// Left Y / right B.
    Upper,
}

/// What a hand is holding, as the resolution table sees it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HeldKind {
    Empty,
    /// A weapon swung by contact (wrench, shard, psi sword).
    Melee,
    /// A ranged weapon, which owns its hand's buttons for gun handling.
    Gun,
    /// The psi amp, which owns its hand's buttons for power selection.
    PsiAmp,
    /// Anything else carried: a clip, a log disc, a medkit.
    Other,
}

/// Whether the player-owned interface (the cyber interface / use mode) is up.
///
/// It takes the buttons back from whatever is held, so the press that opened
/// the interface can always close it again.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HandButtonMode {
    World,
    Interface,
}

/// The action a face button means for the hand that pressed it.
///
/// | hand holds | lower | upper |
/// |---|---|---|
/// | nothing, melee, or any other item | `ToggleUseMode` | `ReadLastUnreadLog` |
/// | a gun | `EjectClip` (that hand's gun) | `CycleGunSetting` (that hand's gun) |
/// | the psi amp | power selection (not yet bound) | power selection (not yet bound) |
///
/// **Mode first.** While the interface is up both buttons keep their interface
/// meaning on both hands whatever is held, so grabbing a gun off the inventory
/// strip cannot strand the player inside the interface.
///
/// Consequence: with a gun in each hand neither the interface nor the log
/// reader is reachable until a hand is free. That is the point of a per-hand
/// mapping - free a hand, or use the one that is already free.
///
/// `hand` does not change which action is returned (the mapping is symmetric);
/// it is taken because a gun/psi row resolves to an action *about that hand's
/// weapon* - `EjectClip` and `CycleGunSetting` act on the gun in the hand that
/// pressed, so the caller must carry the hand through.
pub fn resolve_hand_button(
    mode: HandButtonMode,
    hand: Handedness,
    held: HeldKind,
    button: HandButton,
) -> Option<InputAction> {
    let _ = hand;

    let interface_action = match button {
        HandButton::Lower => InputAction::ToggleUseMode,
        HandButton::Upper => InputAction::ReadLastUnreadLog,
    };

    if mode == HandButtonMode::Interface {
        return Some(interface_action);
    }

    match held {
        HeldKind::Empty | HeldKind::Melee | HeldKind::Other => Some(interface_action),
        // The gun hand's own handling controls: lower ejects that gun's
        // magazine, upper toggles its fire mode.
        HeldKind::Gun => match button {
            HandButton::Lower => Some(InputAction::EjectClip),
            HandButton::Upper => Some(InputAction::CycleGunSetting),
        },
        // Reserved for power selection, not yet bound.
        HeldKind::PsiAmp => None,
    }
}

/// What `hand` is holding, in the terms [`resolve_hand_button`] resolves.
pub fn held_kind_in_hand(world: &World, hand: Handedness) -> HeldKind {
    let Some(held) = crate::wielded_weapon::held_by_hand(world, hand) else {
        return HeldKind::Empty;
    };

    // Psi amp first: it is a gun by property (it fires) and answers to power
    // selection, not gun handling. Melee before guns for the same reason -
    // the electro-shock and the shard are swung, whatever they carry.
    if crate::wielded_weapon::is_psi_amp(world, held) {
        HeldKind::PsiAmp
    } else if crate::mission::mission_core::is_melee_weapon(world, held) {
        HeldKind::Melee
    // `PropBaseGunDesc` rather than the runtime clip (`PropGunState`): it is
    // the authored description every player gun carries, so a gun with no
    // magazine component yet still reads as one.
    } else if world
        .borrow::<View<PropBaseGunDesc>>()
        .is_ok_and(|guns| guns.get(held).is_ok())
    {
        HeldKind::Gun
    } else {
        HeldKind::Other
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HANDS: [Handedness; 2] = [Handedness::Left, Handedness::Right];
    const BUTTONS: [HandButton; 2] = [HandButton::Lower, HandButton::Upper];

    fn resolve(held: HeldKind, hand: Handedness, button: HandButton) -> Option<InputAction> {
        resolve_hand_button(HandButtonMode::World, hand, held, button)
    }

    /// Row 1, on both hands: an empty hand (or one holding anything that is
    /// not a weapon of its own) reaches the player-owned panels.
    #[test]
    fn a_hand_without_a_weapon_owns_the_panels() {
        for hand in HANDS {
            for held in [HeldKind::Empty, HeldKind::Melee, HeldKind::Other] {
                assert_eq!(
                    resolve(held, hand, HandButton::Lower),
                    Some(InputAction::ToggleUseMode),
                    "{held:?} in {hand:?}"
                );
                assert_eq!(
                    resolve(held, hand, HandButton::Upper),
                    Some(InputAction::ReadLastUnreadLog),
                    "{held:?} in {hand:?}"
                );
            }
        }
    }

    /// Rows 2 and 3: a weapon hand's buttons are its weapon's, so neither
    /// reaches a panel - whether or not the weapon's own action is bound yet.
    #[test]
    fn a_weapon_hand_reaches_neither_panel() {
        for hand in HANDS {
            for held in [HeldKind::Gun, HeldKind::PsiAmp] {
                for button in BUTTONS {
                    let resolved = resolve(held, hand, button);
                    assert!(
                        resolved != Some(InputAction::ToggleUseMode)
                            && resolved != Some(InputAction::ReadLastUnreadLog),
                        "{held:?} in {hand:?} reached a panel: {resolved:?}"
                    );
                }
            }
        }
    }

    /// Row 2, lower: the gun hand's lower button ejects the magazine - on
    /// either hand, since the action is resolved against the hand that pressed
    /// it.
    #[test]
    fn a_gun_hands_lower_button_ejects_its_clip() {
        for hand in HANDS {
            assert_eq!(
                resolve(HeldKind::Gun, hand, HandButton::Lower),
                Some(InputAction::EjectClip),
                "{hand:?}"
            );
        }
    }

    /// Row 2, upper: the gun hand's upper button toggles that gun's fire mode
    /// - on either hand, resolved against the hand that pressed it.
    #[test]
    fn a_gun_hands_upper_button_toggles_its_fire_mode() {
        for hand in HANDS {
            assert_eq!(
                resolve(HeldKind::Gun, hand, HandButton::Upper),
                Some(InputAction::CycleGunSetting),
                "{hand:?}"
            );
        }
    }

    /// Mode first: the interface takes both buttons back on both hands, so the
    /// press that opened it always closes it - even if a gun was grabbed off
    /// the inventory strip in between.
    #[test]
    fn the_open_interface_outranks_whatever_is_held() {
        for hand in HANDS {
            for held in [
                HeldKind::Empty,
                HeldKind::Melee,
                HeldKind::Gun,
                HeldKind::PsiAmp,
                HeldKind::Other,
            ] {
                assert_eq!(
                    resolve_hand_button(HandButtonMode::Interface, hand, held, HandButton::Lower),
                    Some(InputAction::ToggleUseMode),
                    "{held:?} in {hand:?}"
                );
                assert_eq!(
                    resolve_hand_button(HandButtonMode::Interface, hand, held, HandButton::Upper),
                    Some(InputAction::ReadLastUnreadLog),
                    "{held:?} in {hand:?}"
                );
            }
        }
    }
}
