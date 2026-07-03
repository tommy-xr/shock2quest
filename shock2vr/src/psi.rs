//! Player psionics: the psi power registry and selection state.
//!
//! Psi powers live in the gamesys as meta-prop templates (`MetaProperty →
//! Psi Powers → Level 1..5`), each carrying a `PropPsiPower` (power id,
//! activation type, psi point cost) and `Projectile` links ordered by the
//! caster's PSI stat (order 1..8). The registry hydrates them once at
//! mission load; the psi amp script casts whichever power the selection
//! unique points at.

use dark::properties::{Link, ProjectileOptions, PropPsiPower, PropSymName};
use dark::ss2_entity_info::{self, SystemShock2EntityInfo};
use shipyard::Unique;

use crate::scripts::script_util::hydrate_template_component;

/// `Projected Cryokinesis` - the power every trained OSA agent starts with,
/// and the default selection.
const CRYOKINESIS_TEMPLATE_ID: i32 = -1143;

/// The `Psi Powers` meta-prop root - every psi power template descends from
/// it (`MetaProperty → Psi Powers → Level 1..5 → power`).
const PSI_POWERS_ROOT_TEMPLATE_ID: i32 = -962;

/// One usable psi power, hydrated from its gamesys meta-prop template.
#[derive(Debug, Clone)]
pub struct PsiPowerInfo {
    pub template_id: i32,
    /// The gamesys symbolic name (e.g. "Cryokinesis", "Inviso").
    pub name: String,
    pub power: PropPsiPower,
    /// The power's `Projectile` links, sorted by `order` (the required PSI
    /// stat level, 1..8). Empty for non-projectile powers.
    pub projectiles: Vec<(i32, ProjectileOptions)>,
}

impl PsiPowerInfo {
    /// The projectile template for a caster with the given PSI stat: the
    /// highest link whose `order` (required PSI level) the stat meets,
    /// falling back to the weakest link for stats below the lowest order.
    pub fn projectile_for_psi_stat(&self, psi_stat: i32) -> Option<i32> {
        self.projectiles
            .iter()
            .rev()
            .find(|(_, opts)| opts.order <= psi_stat)
            .or_else(|| self.projectiles.first())
            .map(|(template_id, _)| *template_id)
    }
}

/// All psi powers from the gamesys, ordered by tier (psi cost) then power id.
#[derive(Unique, Clone)]
pub struct GlobalPsiPowers(pub Vec<PsiPowerInfo>);

/// The player's currently selected psi power - an index into
/// [`GlobalPsiPowers`].
#[derive(Unique, Clone)]
pub struct PsiPowerSelection {
    pub index: usize,
}

/// Hydrate every psi power template (those carrying `P$PsiPower`) into a
/// registry, plus the default selection (Projected Cryokinesis).
pub fn build_psi_power_registry(
    entity_info: &SystemShock2EntityInfo,
) -> (GlobalPsiPowers, PsiPowerSelection) {
    let mut powers = Vec::new();

    let hierarchy = ss2_entity_info::get_hierarchy(entity_info);
    for template_id in entity_info.entity_to_properties.keys() {
        // Powers are gamesys templates (negative ids) under the `Psi Powers`
        // meta-prop root - pre-filter on the (cheap) hierarchy walk so only
        // the ~35 power templates pay for property hydration.
        if *template_id >= 0
            || !ss2_entity_info::get_ancestors(hierarchy, template_id)
                .contains(&PSI_POWERS_ROOT_TEMPLATE_ID)
        {
            continue;
        }
        let Some(power) = hydrate_template_component::<PropPsiPower>(*template_id, entity_info)
        else {
            continue;
        };
        let name = hydrate_template_component::<PropSymName>(*template_id, entity_info)
            .map(|n| n.0)
            .unwrap_or_else(|| format!("Psi Power {}", power.power_id));

        let mut projectiles = template_projectile_links(*template_id, entity_info);
        projectiles.sort_by_key(|(_, opts)| opts.order);

        powers.push(PsiPowerInfo {
            template_id: *template_id,
            name,
            power,
            projectiles,
        });
    }

    powers.sort_by_key(|p| (p.power.psi_cost, p.power.power_id));

    let default_index = powers
        .iter()
        .position(|p| p.template_id == CRYOKINESIS_TEMPLATE_ID)
        .unwrap_or(0);

    (
        GlobalPsiPowers(powers),
        PsiPowerSelection {
            index: default_index,
        },
    )
}

/// A template's `Projectile` links (with data), walking the inheritance
/// chain - the template-side analog of
/// `script_util::get_all_links_with_template`, for templates that are never
/// instantiated as entities.
fn template_projectile_links(
    template_id: i32,
    entity_info: &SystemShock2EntityInfo,
) -> Vec<(i32, ProjectileOptions)> {
    let hierarchy = ss2_entity_info::get_hierarchy(entity_info);
    let mut ancestors = ss2_entity_info::get_ancestors(hierarchy, &template_id);
    ancestors.push(template_id);

    let mut projectiles = Vec::new();
    for ancestor in ancestors {
        let Some(links) = entity_info.template_to_links.get(&ancestor) else {
            continue;
        };
        for to_link in &links.to_links {
            if let Link::Projectile(options) = &to_link.link {
                projectiles.push((to_link.to_template_id, *options));
            }
        }
    }
    projectiles
}
