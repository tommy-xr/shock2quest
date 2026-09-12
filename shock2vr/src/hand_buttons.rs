//! What a face button means on the hand that pressed it.
//!
//! The Touch controllers offer two face buttons per hand (left X/Y, right
//! A/B), which this module treats as one symmetric pair per hand: a **lower**
//! button (left X, right A) and an **upper** one (left Y, right B). The lower
//! button is jump on both hands whatever they hold; the upper one depends on
//! what that hand is holding, so a gun carries its own handling control on the
//! controller wielding it while a free hand keeps the log reader.
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
//! host-testable. Gun taps are delayed until release by `weapon_button_hold`;
//! its long press ejects instead, before this instantaneous table is dispatched.
//! This is contextual input - it reads game state - so it lives
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
    /// Anything else carried: a log disc or a medkit.
    Other,
    /// A physical ammunition clip.
    Ammo,
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
/// | hand holds | lower (X/A) | upper (Y/B) |
/// |---|---|---|
/// | nothing, melee, or any other item | `Jump` | `ReadLastUnreadLog` |
/// | a gun | `Jump` | `CycleGunSetting` (tap/release on that hand's gun) |
/// | an ammo clip | `Jump` | `CycleAmmo` (swap carried clips) |
/// | the psi amp | `Jump` | `SelectPsiPower` (the selection MFD) |
///
/// **Lower is jump, on both hands, whatever they hold.** Jumping is the one
/// control a player reaches for mid-fight with both hands full, so it cannot
/// be something a held weapon takes away; the upper button carries the
/// context-dependent half of the pair alone.
///
/// **Mode first.** While the interface is up, lower keeps the close it always
/// had - the press that opened the interface can always shut it, and jumping
/// out of a menu means nothing anyway - and upper keeps the log reader.
///
/// `hand` does not change which action is returned (the mapping is symmetric);
/// it is taken because the gun row resolves to an action *about that hand's
/// weapon* - `CycleGunSetting` acts on the gun in the hand that pressed, so
/// the caller must carry the hand through.
pub fn resolve_hand_button(
    mode: HandButtonMode,
    hand: Handedness,
    held: HeldKind,
    button: HandButton,
) -> Option<InputAction> {
    let _ = hand;

    if mode == HandButtonMode::Interface {
        return Some(match button {
            HandButton::Lower => InputAction::ToggleUseMode,
            HandButton::Upper => InputAction::ReadLastUnreadLog,
        });
    }

    match button {
        // Unconditional: a hand with a gun in it still has to be able to jump.
        HandButton::Lower => Some(InputAction::Jump),
        HandButton::Upper => Some(match held {
            HeldKind::Empty | HeldKind::Melee | HeldKind::Other => InputAction::ReadLastUnreadLog,
            // The gun hand's own handling control: its fire mode.
            HeldKind::Gun => InputAction::CycleGunSetting,
            HeldKind::Ammo => InputAction::CycleAmmo,
            // The amp hand's own control: the power selection MFD, where a
            // power is *chosen* from a described grid. It names no hand -
            // there is one psi selection, not one per amp.
            HeldKind::PsiAmp => InputAction::SelectPsiPower,
        }),
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
    } else if crate::mission::reload::is_ammo_clip(world, held) {
        HeldKind::Ammo
    } else {
        HeldKind::Other
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HANDS: [Handedness; 2] = [Handedness::Left, Handedness::Right];
    const HELD: [HeldKind; 6] = [
        HeldKind::Empty,
        HeldKind::Melee,
        HeldKind::Gun,
        HeldKind::PsiAmp,
        HeldKind::Other,
        HeldKind::Ammo,
    ];

    fn resolve(held: HeldKind, hand: Handedness, button: HandButton) -> Option<InputAction> {
        resolve_hand_button(HandButtonMode::World, hand, held, button)
    }

    /// The whole lower column: jump, on either hand, holding anything. A gun
    /// in each hand is the case that motivates it - the player must still be
    /// able to jump without stowing a weapon first.
    #[test]
    fn the_lower_button_jumps_whatever_the_hand_holds() {
        for hand in HANDS {
            for held in HELD {
                assert_eq!(
                    resolve(held, hand, HandButton::Lower),
                    Some(InputAction::Jump),
                    "{held:?} in {hand:?}"
                );
            }
        }
    }

    /// Upper, row 1: a hand holding nothing of its own reaches the log reader.
    #[test]
    fn a_hand_without_a_weapon_reads_the_log_on_its_upper_button() {
        for hand in HANDS {
            for held in [HeldKind::Empty, HeldKind::Melee, HeldKind::Other] {
                assert_eq!(
                    resolve(held, hand, HandButton::Upper),
                    Some(InputAction::ReadLastUnreadLog),
                    "{held:?} in {hand:?}"
                );
            }
        }
    }

    /// Upper, row 2: the gun hand's upper button toggles that gun's fire mode
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

    #[test]
    fn ammo_upper_button_cycles_ammo_on_either_hand() {
        for hand in HANDS {
            assert_eq!(
                resolve(HeldKind::Ammo, hand, HandButton::Upper),
                Some(InputAction::CycleAmmo)
            );
        }
    }

    /// Upper, row 3: the amp hand's upper button opens the power selection
    /// MFD - the selector moved up from the lower button, which is now jump.
    #[test]
    fn a_psi_amp_hands_upper_button_opens_the_selector() {
        for hand in HANDS {
            assert_eq!(
                resolve(HeldKind::PsiAmp, hand, HandButton::Upper),
                Some(InputAction::SelectPsiPower),
                "{hand:?}"
            );
        }
    }

    /// A weapon hand's upper button is its weapon's, so it reaches neither
    /// player-owned panel.
    #[test]
    fn a_weapon_hands_upper_button_reaches_neither_panel() {
        for hand in HANDS {
            for held in [HeldKind::Gun, HeldKind::PsiAmp] {
                let resolved = resolve(held, hand, HandButton::Upper);
                assert!(
                    resolved != Some(InputAction::ToggleUseMode)
                        && resolved != Some(InputAction::ReadLastUnreadLog),
                    "{held:?} in {hand:?} reached a panel: {resolved:?}"
                );
            }
        }
    }

    /// The eject and quick-cycle controls lost their buttons: the settings
    /// MFD's UNLOAD and the psi MFD's stick navigation cover them, and a
    /// button that unloads a gun on a mis-press is worse than no button.
    #[test]
    fn no_button_ejects_a_clip_or_quick_cycles_a_power() {
        for mode in [HandButtonMode::World, HandButtonMode::Interface] {
            for hand in HANDS {
                for held in HELD {
                    for button in [HandButton::Lower, HandButton::Upper] {
                        let resolved = resolve_hand_button(mode, hand, held, button);
                        assert!(
                            resolved != Some(InputAction::EjectClip)
                                && resolved != Some(InputAction::CyclePsiPower),
                            "{mode:?}/{held:?}/{hand:?}/{button:?} resolved to {resolved:?}"
                        );
                    }
                }
            }
        }
    }

    /// Mode first: the interface takes both buttons back on both hands, so the
    /// press that opened it always closes it - even if a gun was taken off the
    /// inventory strip in between, and even though lower is otherwise jump.
    #[test]
    fn the_open_interface_outranks_whatever_is_held() {
        for hand in HANDS {
            for held in HELD {
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
