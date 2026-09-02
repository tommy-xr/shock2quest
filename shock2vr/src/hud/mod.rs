use engine::assets::asset_cache::AssetCache;
use shipyard::{Unique, UniqueView, World};

mod damage_flash;
pub use damage_flash::*;

mod item_outline;
pub use item_outline::*;

pub(crate) mod virtual_arms;
pub use virtual_arms::*;

pub(crate) mod ammo_panel;

mod flat_hud;
pub(crate) use flat_hud::*;

/// The English fallback for the reload button, verbatim from the shipped
/// MISC.STR, for a data install that lacks the table.
const FALLBACK_RELOAD_LABEL: &str = "RELOAD";

/// HUD label strings preloaded from MISC.STR, stored as a world `Unique`
/// because the readout layout has no `AssetCache` at draw time (the
/// `ElevatorContext` pattern).
#[derive(Unique, Clone, Debug)]
pub struct HudStrings {
    /// MISC.STR `Reload` ("RELOAD"), the AMMOFULL reload button's label.
    pub reload_label: String,
}

impl Default for HudStrings {
    fn default() -> Self {
        Self {
            reload_label: FALLBACK_RELOAD_LABEL.to_owned(),
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
