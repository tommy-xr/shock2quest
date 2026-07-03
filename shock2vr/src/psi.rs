//! Player psionics: the psi power registry and selection state.
//!
//! Psi powers live in the gamesys as meta-prop templates (`MetaProperty →
//! Psi Powers → Level 1..5`), each carrying a `PropPsiPower` (power id,
//! activation type, psi point cost) and `Projectile` links ordered by the
//! caster's PSI stat (order 1..8). The registry hydrates them once at
//! mission load; the psi amp script casts whichever power the selection
//! unique points at.

use std::collections::HashSet;

use dark::properties::{
    Link, ProjectileOptions, PropPsiPower, PropPsiPowerLearned, PropPsiPowerLearned2,
    PropPsiShield, PropSymName,
};
use dark::ss2_entity_info::{self, SystemShock2EntityInfo};
use shipyard::Unique;

use crate::scripts::script_util::hydrate_template_component;

/// `Projected Cryokinesis` - the power every trained OSA agent starts with,
/// and the default selection.
const CRYOKINESIS_TEMPLATE_ID: i32 = -1143;

/// The `Psi Powers` meta-prop root - every psi power template descends from
/// it (`MetaProperty → Psi Powers → Level 1..5 → power`).
const PSI_POWERS_ROOT_TEMPLATE_ID: i32 = -962;

/// `Inviso` - Photonic Redirection (tier 4 sustained power): while active,
/// the player is invisible to AI and security devices.
pub const INVISO_TEMPLATE_ID: i32 = -3157;

/// `PropPsiPower::activation_type` for sustained/timed self effects - the
/// power activates for a duration given by its `P$PsiShield` data
/// (`duration_base + duration_per_psi × PSI` seconds).
pub const ACTIVATION_TYPE_SUSTAINED: i32 = 1;

/// The powers that support hold-to-overload (per the published gameplay
/// tables), by template id. Comments give the gamesys name and the
/// discipline name players know it by.
const OVERLOADABLE_TEMPLATE_IDS: &[i32] = &[
    // Tier 1
    -1022, // PsiPull (Kinetic Redirection)
    -1143, // Cryokinesis (Projected Cryokinesis)
    -3154, // Codebreaker (Remote Electron Tampering)
    // Tier 2
    -1017, // PsiHeal (Cerebro-Stimulated Regeneration)
    // Tier 3
    -3149, // Fabricate (Molecular Duplication)
    -3151, // ElectroPsi (Electron Cascade)
    -1144, // Pyrokinesis (Projected Pyrokinesis)
    -3156, // Terror (Psionic Hypnogenesis)
    // Tier 4
    -1020, // Electro Dampen (Electron Suppression)
    -1147, // Alchemy (Molecular Transmutation)
    -3159, // CyberHack (Remote Circuitry Manipulation)
    // Tier 5
    -1139, // Major Heal (Advanced Cerebro-Stimulated Regeneration)
    -3160, // SomaDrain (Soma Transference)
    -3161, // PsiCharm (Imposed Neural Restructuring)
    -3162, // ForceWall (Metacreative Barrier)
];

// --- Hold-to-overload tuning -----------------------------------------------
//
// Observed behavior: holding fire fills a bar (faster for higher tiers);
// releasing in the yellow end-zone casts at +2 effective PSI (to a max of
// 10); over-holding past full is a psi burnout - the cast fails and the
// player takes ~3 damage per tier (PSI/Endurance mitigation comes later,
// with player stats). The exact bar speeds are not documented, so the
// durations are tuned approximations.

/// The overload ("yellow") zone: releasing at `fraction >= this` overloads.
/// Fixed for now; the original grows the zone with the PSI stat.
pub const OVERLOAD_ZONE_START: f32 = 0.85;

/// Effective-PSI bonus for a successful overload, and its cap.
pub const OVERLOAD_PSI_BONUS: i32 = 2;
pub const OVERLOAD_MAX_EFFECTIVE_PSI: i32 = 10;

/// Burnout damage: 3 per tier of the burned power.
pub const BURNOUT_DAMAGE_PER_TIER: i32 = 3;

/// How long the meter's result (overload / burnout flash) stays on screen
/// after the charge resolves, in seconds.
pub const CHARGE_RESULT_FLASH_SECS: f32 = 0.6;

/// Seconds of hold for the bar to fill completely: higher tiers charge
/// faster (harder to time). Tier 1 = 2.0s down to tier 5 = 1.0s.
pub fn charge_duration_secs(tier: i32) -> f32 {
    2.0 - 0.25 * (tier.clamp(1, 5) - 1) as f32
}

/// One usable psi power, hydrated from its gamesys meta-prop template.
#[derive(Debug, Clone)]
pub struct PsiPowerInfo {
    pub template_id: i32,
    /// The gamesys symbolic name (e.g. "Cryokinesis", "Inviso").
    pub name: String,
    /// The player-facing discipline name (e.g. "Projected Cryokinesis",
    /// "Photonic Redirection"), from `psihelp.str` (`Psi<power_id>` entries).
    /// `None` if the strings file lacks the power.
    pub display_name: Option<String>,
    pub power: PropPsiPower,
    /// The power's `Projectile` links, sorted by `order` (the required PSI
    /// stat level, 1..8). Empty for non-projectile powers.
    pub projectiles: Vec<(i32, ProjectileOptions)>,
    /// Whether the power supports hold-to-overload.
    pub overloadable: bool,
    /// The sustained-power duration formula (`P$PsiShield`:
    /// `duration_base + duration_per_psi × PSI` seconds). `None` for powers
    /// without timed data.
    pub duration: Option<PropPsiShield>,
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

/// The psi powers the player has been trained in, by power template id -
/// the only powers selectable (`Effect::CyclePsiPower`) and castable
/// (`PsiAmpScript`). Grown at runtime via `Effect::GrantPsiPower`.
#[derive(Unique, Clone)]
pub struct PlayerPsiKnownPowers(pub HashSet<i32>);

/// Seed the trained-power set at mission load.
///
/// - Debug scenes (`all_known`) unlock every power in the registry so the
///   `debug_psi` scene keeps exercising everything.
/// - Real missions read the learned-power bits from the player template's
///   `P$PsiPowerD`/`P$PsiPower2` dwords (authored `0x00000000` in the retail
///   gamesys - the original grants powers at runtime), then union the
///   default OSA loadout: Projected Cryokinesis, which every trained OSA
///   agent starts with.
pub fn build_known_powers(
    entity_info: &SystemShock2EntityInfo,
    powers: &GlobalPsiPowers,
    player_template_id: i32,
    all_known: bool,
) -> PlayerPsiKnownPowers {
    if all_known {
        return PlayerPsiKnownPowers(powers.0.iter().map(|p| p.template_id).collect());
    }

    let dword1 = hydrate_template_component::<PropPsiPowerLearned>(player_template_id, entity_info)
        .map(|p| p.bits)
        .unwrap_or(0);
    let dword2 =
        hydrate_template_component::<PropPsiPowerLearned2>(player_template_id, entity_info)
            .map(|p| p.bits)
            .unwrap_or(0);

    let mut known: HashSet<i32> = powers
        .0
        .iter()
        .filter(|p| learned_bit_set(dword1, dword2, p.power.power_id))
        .map(|p| p.template_id)
        .collect();
    known.insert(CRYOKINESIS_TEMPLATE_ID);
    PlayerPsiKnownPowers(known)
}

/// Whether a power id's learned bit is set across the two learned-power
/// dwords. Layout: **bit index = power id** - dword 1 (`P$PsiPowerD`)
/// covers ids 0..=31, dword 2 (`P$PsiPower2`) covers ids 32..=63 as bit
/// `power_id - 32`. Verified against the shipped data: `earth.mis` entity
/// 243 (`Starting_Location`) authors `P$PsiPowerD = 0x49` = bits {0, 3, 6}
/// = {First Tier Neural Capacity, Kinetic Redirection, Projected
/// Cryokinesis} under this layout - a coherent psi starting loadout
/// (id-keyed discipline names from `psihelp.str`, whose `Psi<id>` keys also
/// place the five tier-capacity pseudo-disciplines at the power-id gaps
/// 0/8/16/24/32).
fn learned_bit_set(dword1: u32, dword2: u32, power_id: i32) -> bool {
    match power_id {
        0..=31 => dword1 & (1u32 << power_id) != 0,
        32..=63 => dword2 & (1u32 << (power_id - 32)) != 0,
        _ => false,
    }
}

/// One currently-active sustained psi power on the player.
#[derive(Clone, Debug)]
pub struct ActivePsiPower {
    pub template_id: i32,
    /// The gamesys symbolic name (e.g. "Inviso").
    pub name: String,
    pub remaining_secs: f32,
}

/// The player's active sustained psi powers, ticked down each frame by
/// `MissionCore::update` and removed on expiry. Re-casting an active power
/// refreshes its duration. (Not yet persisted across save/load or level
/// transitions - like the psi pool and selection.)
#[derive(Unique, Clone, Default)]
pub struct ActivePsiPowers(pub Vec<ActivePsiPower>);

impl ActivePsiPowers {
    pub fn is_active(&self, template_id: i32) -> bool {
        self.0.iter().any(|p| p.template_id == template_id)
    }
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
            display_name: None,
            power,
            projectiles,
            overloadable: OVERLOADABLE_TEMPLATE_IDS.contains(template_id),
            duration: hydrate_template_component::<PropPsiShield>(*template_id, entity_info),
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

/// Fill in player-facing discipline names from the `psihelp.str` string
/// table: each power's entry is keyed `Psi<power_id>` and its first line is
/// the discipline name (the rest is the help text).
pub fn apply_display_names(
    powers: &mut GlobalPsiPowers,
    strings: &std::collections::HashMap<String, String>,
) {
    for power in &mut powers.0 {
        // The strings importer lowercases keys ("Psi6:" -> "psi6").
        let key = format!("psi{}", power.power.power_id);
        power.display_name = strings
            .get(&key)
            .and_then(|text| text.lines().next())
            .map(|line| line.trim().to_owned())
            .filter(|line| !line.is_empty());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn learned_bits_split_across_dwords_by_power_id() {
        // Dword 1 covers power ids 0..=31 as bit `power_id`. The reference
        // value from the shipped data: earth.mis Starting_Location authors
        // 0x49 = {tier-1 capacity (0), Kinetic Redirection (3), Projected
        // Cryokinesis (6)}.
        assert!(learned_bit_set(0x49, 0, 0));
        assert!(learned_bit_set(0x49, 0, 3));
        assert!(learned_bit_set(0x49, 0, 6));
        assert!(!learned_bit_set(0x49, 0, 1));
        assert!(learned_bit_set(1 << 31, 0, 31));
        assert!(!learned_bit_set(0, 1 << 6, 6));
        // Dword 2 covers power ids 32..=63 as bit `power_id - 32`.
        assert!(learned_bit_set(0, 0b10, 33));
        assert!(learned_bit_set(0, 1 << 7, 39));
        assert!(!learned_bit_set(0, 0, 39));
    }
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
