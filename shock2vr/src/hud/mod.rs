use engine::assets::asset_cache::AssetCache;
use shipyard::{Unique, UniqueView, World};

mod damage_flash;
pub use damage_flash::*;

mod item_outline;
pub use item_outline::*;

pub(crate) mod wrist;
pub use wrist::*;

pub(crate) mod ammo_panel;
pub(crate) mod readouts;

pub(crate) mod message_line;
pub use message_line::HudMessages;

mod flat_hud;
pub(crate) use flat_hud::*;

/// The English fallback for the reload button, verbatim from the shipped
/// MISC.STR, for a data install that lacks the table.
const FALLBACK_RELOAD_LABEL: &str = "RELOAD";

/// The English fallback for the weapon-settings modification line, verbatim
/// from the shipped MISC.STR (`%d` is the gun's `PropGunState.modification`).
const FALLBACK_MOD_LEVEL_LABEL: &str = "Modification Level %d";

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
}

impl Default for HudStrings {
    fn default() -> Self {
        Self {
            reload_label: FALLBACK_RELOAD_LABEL.to_owned(),
            mod_level_label: FALLBACK_MOD_LEVEL_LABEL.to_owned(),
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
