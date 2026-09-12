use engine::assets::asset_cache::AssetCache;
use shipyard::{Unique, UniqueView, World};

mod damage_flash;
pub use damage_flash::*;

mod item_outline;
pub use item_outline::*;

pub(crate) mod virtual_arms;
pub use virtual_arms::*;

pub(crate) mod alarm_panel;
pub(crate) mod ammo_panel;
pub(crate) mod readouts;

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
        }
    }
}

impl HudStrings {
    pub fn load(asset_cache: &mut AssetCache) -> HudStrings {
        let strings = asset_cache.get_opt(&dark::importers::STRINGS_IMPORTER, "misc.str");
        HudStrings {
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

pub(crate) mod hazards;
