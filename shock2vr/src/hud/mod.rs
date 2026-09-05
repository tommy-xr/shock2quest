use engine::assets::asset_cache::AssetCache;
use shipyard::{Unique, UniqueView, World};

use crate::scripts::gui::{HrmMode, HrmResult};

mod damage_flash;
pub use damage_flash::*;

mod item_outline;
pub use item_outline::*;

pub(crate) mod virtual_arms;
pub use virtual_arms::*;

pub(crate) mod ammo_panel;

pub(crate) mod message_line;
pub use message_line::HudMessages;

mod flat_hud;
pub(crate) use flat_hud::*;

/// Which tenth of its condition a gun is in, 1 (worn out) to 10 (pristine).
///
/// The original truncates the 0..100 condition and divides it into ten
/// buckets. Both readouts of a gun's condition - the word its name carries and
/// the badge on the ammo gauge - grade it here, so the label and the picture
/// can never disagree about how worn a gun is.
pub(crate) fn gun_condition_bucket(condition: f32) -> i32 {
    (condition as i32 / 10).clamp(0, 9) + 1
}

/// The English fallback for the reload button, verbatim from the shipped
/// MISC.STR, for a data install that lacks the table.
const FALLBACK_RELOAD_LABEL: &str = "RELOAD";

/// The English fallback for the weapon-settings modification line, verbatim
/// from the shipped MISC.STR (`%d` is the gun's `PropGunState.modification`).
const FALLBACK_MOD_LEVEL_LABEL: &str = "Modification Level %d";

/// The English fallback for the line a gun's owner gets when it breaks,
/// verbatim from the shipped MISC.STR (`%s` is the gun's short name).
const FALLBACK_WEAPON_BREAKS: &str = "%s has broken!";

/// The English fallbacks for the maintenance tool's four refusals, verbatim
/// from the shipped MISC.STR. `%d` in the skill line is the minimum Maintain
/// level the weapon authors.
const FALLBACK_WRENCH_ON_NON_GUN: &str = "Drag tool to ranged weapon to use.";
const FALLBACK_WRENCH_ON_BROKEN: &str = "Use repair skill on weapon first.";
const FALLBACK_WRENCH_SKILL_REQ: &str = "Maintaining this weapon requires a skill of %d.";
const FALLBACK_WRENCH_UNUSED: &str = "Weapon already in good condition.";

/// The English fallbacks for the board's modify mode, verbatim from the
/// shipped HRM.STR.
const FALLBACK_MODIFY_SKILL_REQ: &str = "Modify skill %d required.";
const FALLBACK_MODIFY_MAXED: &str = "This weapon cannot be modified any further.";

/// The English fallbacks for the board's result lines, verbatim from the
/// shipped HRM.STR. Indexed `[mode][result]` in `HrmMode`/`HrmResult` order:
/// hack, repair, modify x played-out, won, lost.
const FALLBACK_HRM_RESULTS: [[&str; 3]; 3] = [
    [
        "You could not break the security on this attempt.",
        "Hacking successful!",
        "Hacking failed!",
    ],
    [
        "You were unable to repair the item on this attempt.",
        "The item has been successfully repaired, and can be used normally.",
        "You have destroyed the item!",
    ],
    [
        "You did not successfully modify the weapon on this attempt.",
        "Modification completed!",
        "Modification Failed!",
    ],
];

/// The English fallback for the board's repair skill gate, verbatim from the
/// shipped HRM.STR (`%d` is the minimum Repair level the object authors).
const FALLBACK_REPAIR_SKILL_REQ: &str = "Repair skill %d required.";

/// HUD label strings preloaded from MISC.STR, stored as a world `Unique`
/// because the readout layout has no `AssetCache` at draw time (the
/// `ElevatorContext` pattern).
#[derive(Unique, Clone, Debug)]
pub struct HudStrings {
    /// MISC.STR `Reload` ("RELOAD"), the AMMOFULL reload button's label.
    pub reload_label: String,
    /// MISC.STR `ModLevel` ("Modification Level %d"), the settings MFD's
    /// modification line. The `%d` is substituted at draw time.
    pub mod_level_label: String,
    /// MISC.STR `WeaponBreaks` ("%s has broken!"), the status line a gun posts
    /// when it gives out. The `%s` is the gun's short name.
    pub weapon_breaks_message: String,
    /// MISC.STR `WrenchOnNonGun`: the maintenance tool was used on something
    /// that is not a ranged weapon.
    pub wrench_on_non_gun: String,
    /// MISC.STR `WrenchOnBroken`: a broken weapon needs repairing, not
    /// maintaining.
    pub wrench_on_broken: String,
    /// MISC.STR `WrenchSkillReq` ("... a skill of %d"): the player's Maintain
    /// level is below the minimum this weapon authors.
    pub wrench_skill_req: String,
    /// MISC.STR `WrenchUnused`: the weapon is already at full condition.
    pub wrench_unused: String,
    /// HRM.STR `techminskill1` ("Repair skill %d required."): the player's
    /// Repair level is below the minimum the object authors, so the board
    /// never opens in repair mode.
    pub repair_skill_req: String,
    /// HRM.STR `techminskill2` ("Modify skill %d required."): the player's
    /// Modify level is below what the gun's next modification demands.
    pub modify_skill_req: String,
    /// HRM.STR `ModifyResult3`: the gun has had every modification it can take.
    /// Not part of `hrm_results` despite the shared key family - it is a refusal
    /// at the panel's door, not the outcome of a board that was played.
    pub modify_maxed: String,
    /// HRM.STR `<mode>Result<n>`: what a finished board says, per mode. Read
    /// through [`HudStrings::hrm_result`] rather than indexed by hand.
    hrm_results: [[String; 3]; 3],
}

/// Resolve the nine `<mode>result<n>` lines out of HRM.STR, or the English
/// fallbacks when it has none.
///
/// Every slot is addressed by `HrmMode::index`/`HrmResult::index` - the same
/// pair [`HudStrings::hrm_result`] reads it back with - so a mode can never end
/// up speaking in another mode's voice through a reordered literal.
fn hrm_results(strings: Option<&std::collections::HashMap<String, String>>) -> [[String; 3]; 3] {
    let mut resolved: [[String; 3]; 3] = Default::default();
    for mode in [HrmMode::Hack, HrmMode::Repair, HrmMode::Modify] {
        for result in [HrmResult::PlayedOut, HrmResult::Won, HrmResult::Lost] {
            let fallback = FALLBACK_HRM_RESULTS[mode.index()][result.index()];
            resolved[mode.index()][result.index()] = crate::ui::resolve_menu_label(
                strings,
                &format!("{}result{}", mode.string_prefix(), result.index()),
                fallback,
            );
        }
    }
    resolved
}

impl Default for HudStrings {
    fn default() -> Self {
        Self {
            reload_label: FALLBACK_RELOAD_LABEL.to_owned(),
            mod_level_label: FALLBACK_MOD_LEVEL_LABEL.to_owned(),
            weapon_breaks_message: FALLBACK_WEAPON_BREAKS.to_owned(),
            wrench_on_non_gun: FALLBACK_WRENCH_ON_NON_GUN.to_owned(),
            wrench_on_broken: FALLBACK_WRENCH_ON_BROKEN.to_owned(),
            wrench_skill_req: FALLBACK_WRENCH_SKILL_REQ.to_owned(),
            wrench_unused: FALLBACK_WRENCH_UNUSED.to_owned(),
            repair_skill_req: FALLBACK_REPAIR_SKILL_REQ.to_owned(),
            modify_skill_req: FALLBACK_MODIFY_SKILL_REQ.to_owned(),
            modify_maxed: FALLBACK_MODIFY_MAXED.to_owned(),
            hrm_results: hrm_results(None),
        }
    }
}

impl HudStrings {
    /// What the board says when it finishes, in the words of the mode it was
    /// played in.
    pub(crate) fn hrm_result(&self, mode: HrmMode, result: HrmResult) -> &str {
        &self.hrm_results[mode.index()][result.index()]
    }

    pub fn load(asset_cache: &mut AssetCache) -> HudStrings {
        let strings = asset_cache.get_opt(&dark::importers::STRINGS_IMPORTER, "misc.str");
        // The board's own table: everything the HRM's three modes say lives in
        // HRM.STR, not in the general HUD one.
        let hrm_strings = asset_cache.get_opt(&dark::importers::STRINGS_IMPORTER, "hrm.str");
        HudStrings {
            repair_skill_req: crate::ui::resolve_menu_label(
                hrm_strings.as_deref(),
                "techminskill1",
                FALLBACK_REPAIR_SKILL_REQ,
            ),
            modify_skill_req: crate::ui::resolve_menu_label(
                hrm_strings.as_deref(),
                "techminskill2",
                FALLBACK_MODIFY_SKILL_REQ,
            ),
            modify_maxed: crate::ui::resolve_menu_label(
                hrm_strings.as_deref(),
                "modifyresult3",
                FALLBACK_MODIFY_MAXED,
            ),
            hrm_results: hrm_results(hrm_strings.as_deref()),
            reload_label: crate::ui::resolve_menu_label(
                strings.as_deref(),
                "reload",
                FALLBACK_RELOAD_LABEL,
            ),
            mod_level_label: crate::ui::resolve_menu_label(
                strings.as_deref(),
                "modlevel",
                FALLBACK_MOD_LEVEL_LABEL,
            ),
            weapon_breaks_message: crate::ui::resolve_menu_label(
                strings.as_deref(),
                "weaponbreaks",
                FALLBACK_WEAPON_BREAKS,
            ),
            wrench_on_non_gun: crate::ui::resolve_menu_label(
                strings.as_deref(),
                "wrenchonnongun",
                FALLBACK_WRENCH_ON_NON_GUN,
            ),
            wrench_on_broken: crate::ui::resolve_menu_label(
                strings.as_deref(),
                "wrenchonbroken",
                FALLBACK_WRENCH_ON_BROKEN,
            ),
            wrench_skill_req: crate::ui::resolve_menu_label(
                strings.as_deref(),
                "wrenchskillreq",
                FALLBACK_WRENCH_SKILL_REQ,
            ),
            wrench_unused: crate::ui::resolve_menu_label(
                strings.as_deref(),
                "wrenchunused",
                FALLBACK_WRENCH_UNUSED,
            ),
        }
    }
}

/// The preloaded HUD strings, or the English defaults in a world that has none
/// (the unit-test worlds, and any scene loaded without a strings table).
pub(crate) fn hud_strings(world: &World) -> HudStrings {
    world
        .borrow::<UniqueView<HudStrings>>()
        .map(|s| s.clone())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    /// A localized HRM.STR would silently swap two modes' voices if the table
    /// were filled positionally, so resolve a stub whose nine values name the
    /// slot they belong in and check each lands where it is read back from.
    #[test]
    fn each_mode_result_resolves_into_its_own_slot() {
        let table: HashMap<String, String> = [
            ("hackresult0", "h0"),
            ("hackresult1", "h1"),
            ("hackresult2", "h2"),
            ("repairresult0", "r0"),
            ("repairresult1", "r1"),
            ("repairresult2", "r2"),
            ("modifyresult0", "m0"),
            ("modifyresult1", "m1"),
            ("modifyresult2", "m2"),
        ]
        .into_iter()
        .map(|(key, value)| (key.to_owned(), value.to_owned()))
        .collect();

        let strings = HudStrings {
            hrm_results: hrm_results(Some(&table)),
            ..HudStrings::default()
        };
        for (mode, prefix) in [
            (HrmMode::Hack, 'h'),
            (HrmMode::Repair, 'r'),
            (HrmMode::Modify, 'm'),
        ] {
            for (result, index) in [
                (HrmResult::PlayedOut, 0),
                (HrmResult::Won, 1),
                (HrmResult::Lost, 2),
            ] {
                assert_eq!(strings.hrm_result(mode, result), format!("{prefix}{index}"));
            }
        }
    }
}
