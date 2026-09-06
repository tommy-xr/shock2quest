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

/// Power ids per tier: the tier's capacity marker plus its seven
/// disciplines. See [`PsiPowerInfo::tier`].
pub const POWERS_PER_TIER: i32 = 8;

/// `Projected Cryokinesis` - the power every trained OSA agent starts with,
/// and the default selection.
const CRYOKINESIS_TEMPLATE_ID: i32 = -1143;

/// The `Psi Powers` meta-prop root - every psi power template descends from
/// it (`MetaProperty → Psi Powers → Level 1..5 → power`).
const PSI_POWERS_ROOT_TEMPLATE_ID: i32 = -962;

/// `Inviso` - Photonic Redirection (tier 4 sustained power): while active,
/// the player is invisible to AI and security devices.
pub const INVISO_TEMPLATE_ID: i32 = -3157;

/// `Berserk` - Adrenaline Overproduction (tier 2 sustained power): while
/// active, the player's melee hits do more damage and the adrenaline drains
/// their health (see `scripts::berserk`).
pub const BERSERK_TEMPLATE_ID: i32 = -3155;

/// `Stability` - Anti-entropic Field (tier 3 sustained power): while active,
/// the player's guns neither wear nor break (see `scripts::weapon_script`).
pub const STABILITY_TEMPLATE_ID: i32 = -3148;

/// `PropPsiPower::activation_type` for sustained/timed self effects - the
/// power activates for a duration given by its `P$PsiShield` data
/// (`duration_base + duration_per_psi × PSI` seconds).
pub const ACTIVATION_TYPE_SUSTAINED: i32 = 1;

/// `PropPsiPower::activation_type` for instant/special powers - they resolve
/// immediately on cast, with no duration and no projectile. Self-targeted
/// (the heals, SomaDrain) and remote (CyberHack, ForceWall) powers both land
/// here.
pub const ACTIVATION_TYPE_INSTANT: i32 = 2;

/// `PsiHeal` - Cerebro-stimulated Regeneration (tier 2 instant power): heals
/// the caster.
pub const PSI_HEAL_TEMPLATE_ID: i32 = -1017;

/// `Major Heal` - Advanced Cerebro-stimulated Regeneration (tier 5 instant
/// power): the large version of [`PSI_HEAL_TEMPLATE_ID`] - same script, same
/// `data` shape, bigger numbers.
pub const MAJOR_HEAL_TEMPLATE_ID: i32 = -1139;

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

/// The effective PSI a cast resolves at: the caster's PSI stat, plus the
/// overload bonus when the charge released in the end zone, capped.
pub fn effective_psi_for_cast(psi_stat: i32, overload: bool) -> i32 {
    if overload {
        (psi_stat + OVERLOAD_PSI_BONUS).min(OVERLOAD_MAX_EFFECTIVE_PSI)
    } else {
        psi_stat
    }
}

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
    /// The discipline tier (1..5) this power belongs to.
    ///
    /// Power ids are authored in blocks of eight per tier - id `(tier-1)*8` is
    /// the tier's neural-capacity marker and `(tier-1)*8 + 1..7` are its seven
    /// disciplines - which is also how the selection screen lays them out. The
    /// psi-point *cost* is a separate number that happens to equal the tier in
    /// the shipped data; reading it as the tier is a coincidence, not a rule.
    pub fn tier(&self) -> i32 {
        self.power.power_id / POWERS_PER_TIER + 1
    }

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

/// The tier the psi power selection MFD is currently *browsing*.
///
/// Separate from [`PsiPowerSelection`]: a player can page through tiers on the
/// panel without changing the power the amp will cast, and only a click on a
/// trained power commits. It lives here, beside the selection, rather than in
/// the panel's own GUI state so that the mission can snap it whenever the
/// selection moves (a stick flick, the `CyclePsiPower` key) - a GUI's state is
/// only reachable from its own messages.
#[derive(Unique, Clone, Copy)]
pub struct PsiPanelTier(pub i32);

/// Which axis of the psi selection a step moves along: the AMMOFULL readout's
/// tier arrows move between tiers, its power arrows move within one, and the
/// `CyclePsiPower` key walks every trained power in order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PsiSelectionAxis {
    Tier,
    Power,
    /// Every trained power, tiers included - the single-key cycle.
    Any,
}

/// The selection a psi selection step lands on.
///
/// `Tier` steps to the next tier that owns a trained power (empty tiers are
/// skipped) and lands on its first one; `Power` wraps within the current tier;
/// `Any` walks the whole trained list. All wrap, and all leave the selection
/// alone when nothing else is trained, so a step can never select a power the
/// player has not learned.
pub fn step_selection(
    powers: &[PsiPowerInfo],
    known: &HashSet<i32>,
    index: usize,
    axis: PsiSelectionAxis,
    forward: bool,
) -> usize {
    let Some(current) = powers.get(index) else {
        return index;
    };
    // `powers` is sorted by (tier, power id), so a tier's trained powers are a
    // contiguous run and tier order is just the order of the distinct tiers.
    let trained: Vec<(usize, i32)> = powers
        .iter()
        .enumerate()
        .filter(|(_, p)| known.contains(&p.template_id))
        .map(|(i, p)| (i, p.tier()))
        .collect();
    if trained.is_empty() {
        return index;
    }
    // The registry holds untrained powers too, and nothing normalizes the
    // selection when a power is granted - so the selection can be sitting on an
    // untrained power with no neighbour to step from. Recover onto the first
    // trained power rather than leaving every step a no-op.
    if !known.contains(&current.template_id) {
        return trained[0].0;
    }
    let tier = current.tier();
    match axis {
        PsiSelectionAxis::Any => {
            let every: Vec<usize> = trained.iter().map(|(i, _)| *i).collect();
            neighbor(&every, index, forward).unwrap_or(index)
        }
        PsiSelectionAxis::Power => {
            let in_tier: Vec<usize> = trained
                .iter()
                .filter(|(_, t)| *t == tier)
                .map(|(i, _)| *i)
                .collect();
            neighbor(&in_tier, index, forward).unwrap_or(index)
        }
        PsiSelectionAxis::Tier => {
            let mut tiers: Vec<i32> = trained.iter().map(|(_, t)| *t).collect();
            tiers.dedup();
            let Some(next_tier) = neighbor(&tiers, tier, forward).filter(|next| *next != tier)
            else {
                // Only one tier is trained: a tier step has nowhere to go, and
                // must not quietly behave like a power step.
                return index;
            };
            trained
                .iter()
                .find(|(_, t)| *t == next_tier)
                .map(|(i, _)| *i)
                .unwrap_or(index)
        }
    }
}

/// The entry after (`forward`) or before `current` in `items`, wrapping.
/// `None` when `current` is not in `items`.
fn neighbor<T: Copy + PartialEq>(items: &[T], current: T, forward: bool) -> Option<T> {
    let at = items.iter().position(|item| *item == current)?;
    let len = items.len();
    let next = if forward {
        (at + 1) % len
    } else {
        (at + len - 1) % len
    };
    Some(items[next])
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

/// The `psihelp.str` table with its escaped line breaks made real.
///
/// The strings files write a paragraph break as the two characters `\` and
/// `n`, and nothing downstream un-escapes them - so an entry read as-is draws
/// the escape ("Screen\n\nProtects you...") and its "first line" is the whole
/// entry. Both readers of this table want the breaks: the selection MFD wraps
/// the help paragraphs, and [`apply_display_names`] takes line 1 as the
/// discipline name.
pub fn normalize_help_strings(
    strings: &std::collections::HashMap<String, String>,
) -> std::collections::HashMap<String, String> {
    strings
        .iter()
        .map(|(key, value)| (key.clone(), value.replace("\\n", "\n")))
        .collect()
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

    /// A help entry's escaped breaks become real ones, so its first line is
    /// the discipline name rather than the whole paragraph.
    #[test]
    fn help_strings_unescape_their_line_breaks() {
        let raw = std::collections::HashMap::from([(
            "psi6".to_owned(),
            "Projected Cryokinesis\\n\\nLaunches a projectile.".to_owned(),
        )]);
        let normalized = normalize_help_strings(&raw);
        let entry = &normalized["psi6"];
        assert!(!entry.contains('\\'), "{entry:?} still carries an escape");
        assert_eq!(entry.lines().next(), Some("Projected Cryokinesis"));
    }

    #[test]
    fn overload_adds_the_bonus_and_caps() {
        assert_eq!(effective_psi_for_cast(2, false), 2);
        assert_eq!(effective_psi_for_cast(2, true), 2 + OVERLOAD_PSI_BONUS);
        assert_eq!(
            effective_psi_for_cast(OVERLOAD_MAX_EFFECTIVE_PSI - 1, true),
            OVERLOAD_MAX_EFFECTIVE_PSI
        );
    }

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

    /// A registry in the shape `build_psi_power_registry` produces: sorted by
    /// (tier, power id). `(template_id, tier)` pairs; the power id is authored
    /// inside the tier's block of eight, the way the shipped data does it, so
    /// `PsiPowerInfo::tier` reads back the tier the fixture asked for. The psi
    /// cost is deliberately NOT the tier, so a test that passes cannot be
    /// reading the cost by accident.
    fn registry(powers: &[(i32, i32)]) -> Vec<PsiPowerInfo> {
        let mut slot_in_tier = std::collections::HashMap::new();
        powers
            .iter()
            .map(|(template_id, tier)| {
                let slot = slot_in_tier.entry(*tier).or_insert(0);
                *slot += 1;
                PsiPowerInfo {
                    template_id: *template_id,
                    name: format!("Power {template_id}"),
                    display_name: None,
                    power: PropPsiPower {
                        power_id: (tier - 1) * POWERS_PER_TIER + *slot,
                        activation_type: 0,
                        psi_cost: 99,
                        data: [0.0; 4],
                    },
                    projectiles: Vec::new(),
                    overloadable: false,
                    duration: None,
                }
            })
            .collect()
    }

    /// The tier comes from the power id's block of eight, never from the psi
    /// cost - those agree in the shipped data but are different numbers.
    #[test]
    fn a_powers_tier_is_read_off_its_power_id() {
        let powers = registry(&[(-1, 1), (-2, 4)]);
        assert_eq!(powers[0].tier(), 1);
        assert_eq!(powers[1].tier(), 4);
        assert_ne!(
            powers[1].power.psi_cost, 4,
            "the fixture's cost is not its tier"
        );
    }

    /// Tiers 1, 1, 3 - tier 2 is authored but untrained, so it must be skipped.
    fn tiered() -> (Vec<PsiPowerInfo>, HashSet<i32>) {
        let powers = registry(&[(-1, 1), (-2, 1), (-3, 2), (-4, 3), (-5, 3)]);
        (powers, HashSet::from([-1, -2, -4, -5]))
    }

    #[test]
    fn power_arrows_wrap_within_the_current_tier() {
        let (powers, known) = tiered();
        let step = |index, forward| {
            step_selection(&powers, &known, index, PsiSelectionAxis::Power, forward)
        };
        assert_eq!(step(0, true), 1);
        // ...and wraps rather than spilling into the next tier.
        assert_eq!(step(1, true), 0);
        assert_eq!(step(0, false), 1);
        // Tier 3's pair is independent of tier 1's.
        assert_eq!(step(3, true), 4);
        assert_eq!(step(4, true), 3);
    }

    #[test]
    fn tier_arrows_skip_tiers_with_no_trained_power() {
        let (powers, known) = tiered();
        let step = |index, forward| {
            step_selection(&powers, &known, index, PsiSelectionAxis::Tier, forward)
        };
        // Tier 1 -> tier 3: tier 2 is untrained, so it is not a stop.
        assert_eq!(step(0, true), 3);
        // ...and back again, wrapping.
        assert_eq!(step(3, true), 0);
        assert_eq!(step(0, false), 3);
        // A tier step lands on that tier's FIRST trained power, not the one at
        // the same offset.
        assert_eq!(step(4, true), 0);
    }

    #[test]
    fn a_selection_sitting_on_an_untrained_power_recovers() {
        // The registry holds every power, trained or not, and the default
        // selection is index 0 - so a player whose training starts at tier 3
        // begins on an untrained power. Both axes must escape it, not stall.
        let (powers, _) = tiered();
        let known = HashSet::from([-4, -5]);
        for axis in [
            PsiSelectionAxis::Tier,
            PsiSelectionAxis::Power,
            PsiSelectionAxis::Any,
        ] {
            assert_eq!(
                step_selection(&powers, &known, 0, axis, true),
                3,
                "{axis:?} should recover onto the first trained power"
            );
        }
    }

    #[test]
    fn an_arrow_never_selects_an_untrained_power() {
        let (powers, _) = tiered();
        // Only the tier-1 pair is trained: both axes stay inside it.
        let known = HashSet::from([-1, -2]);
        for axis in [PsiSelectionAxis::Tier, PsiSelectionAxis::Power] {
            for forward in [true, false] {
                let next = step_selection(&powers, &known, 0, axis, forward);
                assert!(
                    known.contains(&powers[next].template_id),
                    "{axis:?}/{forward} selected an untrained power"
                );
            }
        }
        // A lone trained power has nowhere to go, on any axis.
        let single = HashSet::from([-1]);
        for axis in [
            PsiSelectionAxis::Tier,
            PsiSelectionAxis::Power,
            PsiSelectionAxis::Any,
        ] {
            assert_eq!(
                step_selection(&powers, &single, 0, axis, true),
                0,
                "{axis:?}"
            );
        }
        // Nor does a tier step become a power step when the trained powers all
        // share one tier.
        let one_tier = HashSet::from([-1, -2]);
        assert_eq!(
            step_selection(&powers, &one_tier, 1, PsiSelectionAxis::Tier, true),
            1,
            "one trained tier means the tier arrow is inert"
        );
    }

    #[test]
    fn an_empty_or_out_of_range_selection_is_left_alone() {
        let (powers, known) = tiered();
        assert_eq!(
            step_selection(&powers, &known, 99, PsiSelectionAxis::Tier, true),
            99
        );
        assert_eq!(
            step_selection(&[], &known, 0, PsiSelectionAxis::Power, true),
            0
        );
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
