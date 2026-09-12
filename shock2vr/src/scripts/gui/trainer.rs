//! Trainer / upgrade station MFD panels (projects/flat-ui-panels.md §3).
//!
//! Each trainer machine opens one category panel directly (there is no
//! in-panel category chooser - the "category" is which machine was frobbed),
//! mirroring the original engine's trainer overlays 35-38:
//! stats / tech skills / weapon skills / psi. Rows list the current level and
//! the cost of the next level from the gamesys cost tables
//! (`STATCOST`/`WTECHCOST`/`WSKILLCOST`/`PSICOST`, Normal difficulty - see
//! `dark::gamesys::params`). Buying validates in [`upgrade_quote`] (shared
//! with the `Effect::TrainerPurchase` handler, which re-validates and applies
//! atomically since `Gui::handle_msg` must not mutate).
//!
//! Like the retail engine, purchases write player properties directly - no
//! script round-trip: the effect handler bumps `PlayerStats` inside
//! `QuestInfo`, so upgrades persist across save/load and level transitions.
//!
//! Deliberate simplifications (see the PR): the psi panel sells *tier
//! unlocks* only (`PlayerStats::psi_tier`, sequential, from `PSICOST[t][0]`);
//! individual power purchases within a tier are deferred until known-psi-power
//! persistence lands. UNDO is deferred (the shared `Gui` layer has no
//! panel-open/close lifecycle to snapshot against). Costs are Normal
//! difficulty (no difficulty setting exists yet).
//!
//! A stat is only sold when it has a live gameplay consumer. Strength expands
//! the backpack, Endurance raises maximum HP, and Cyber Affinity feeds hacking
//! odds; Psionics and Agility stay visible but unavailable until their
//! advertised systems exist. This prevents a stored-only stat bump from
//! consuming irreplaceable modules.

use cgmath::{Vector2, Vector3, vec2};
use dark::gamesys::TrainerCostTables;
use shipyard::{EntityId, UniqueView, World};

use crate::gui::{Gui, GuiComponent, GuiConfig, GuiCursor};
use crate::player_stats::{PSI_TIER_CAP, PlayerStats, SKILL_CAP, STAT_CAP, Skill, Stat};
use crate::quest_info::QuestInfo;
use crate::scripts::Effect;

use crate::gui;

/// What one trainer row upgrades.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrainerTarget {
    Stat(Stat),
    Skill(Skill),
    /// Unlock psi tier N (1..=5). Tiers are sequential.
    PsiTier(i32),
}

/// Which category panel a machine opens (the four trainer script types).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrainerMode {
    Stats,
    Tech,
    Weapons,
    Psi,
}

/// Whether a stat has a real gameplay consumer and is therefore safe to sell.
/// Keep this gate shared by display, immediate feedback, and authoritative
/// effect handling so a future caller cannot bypass the module-loss guard.
pub fn stat_upgrade_available(stat: Stat) -> bool {
    matches!(
        stat,
        Stat::Strength | Stat::Endurance | Stat::CyberAffinity | Stat::PsionicAbility
    )
}

/// The cost of buying `target`'s next level given the player's current sheet,
/// or `None` if it cannot be bought (unavailable, maxed, or an out-of-order
/// psi tier).
/// Shared by the panel (for display/refusal) and the `TrainerPurchase` effect
/// handler (the authoritative re-validation before mutating).
pub fn upgrade_quote(
    costs: &TrainerCostTables,
    stats: &PlayerStats,
    target: TrainerTarget,
) -> Option<i32> {
    match target {
        TrainerTarget::Stat(stat) => {
            if !stat_upgrade_available(stat) {
                return None;
            }
            let level = stats.stat_level(stat);
            if !(1..STAT_CAP).contains(&level) {
                return None;
            }
            // STATCOST[stat][i]: cost of raising from level i+1 to i+2.
            Some(costs.stat_cost[stat_row(stat)][(level - 1) as usize])
        }
        TrainerTarget::Skill(skill) => {
            let level = stats.skill_level(skill);
            if !(0..SKILL_CAP).contains(&level) {
                return None;
            }
            // Cost tables: cost of raising from level i to i+1.
            Some(match skill_row(skill) {
                SkillRow::Tech(row) => costs.tech_cost[row][level as usize],
                SkillRow::Weapon(row) => costs.weapon_cost[row][level as usize],
            })
        }
        TrainerTarget::PsiTier(tier) => {
            // Only the next tier is purchasable, in order.
            if tier != stats.psi_tier + 1 || tier > PSI_TIER_CAP {
                return None;
            }
            // PSICOST[tier][0] = the tier unlock cost.
            Some(costs.psi_cost[(tier - 1) as usize][0])
        }
    }
}

/// Apply a validated purchase to the character sheet (the effect handler's
/// mutation, after `upgrade_quote` + `spend_cyber_modules` succeeded).
pub fn apply_purchase(stats: &mut PlayerStats, target: TrainerTarget) {
    match target {
        TrainerTarget::Stat(stat) => stats.raise_stat(stat),
        TrainerTarget::Skill(skill) => stats.raise_skill(skill),
        TrainerTarget::PsiTier(tier) => stats.psi_tier = tier,
    }
}

// Canonical STATCOST row order (gamesys field order, verified against original
// engine behavior): STR(0), END(1), PSI(2), AGI(3), CYB(4). Note PSI comes
// BEFORE AGI - do not "fix" this back to enum order. Retail shock2.gam happens
// to ship identical costs for all five rows, so tests on retail data cannot
// catch a swap here.
fn stat_row(stat: Stat) -> usize {
    match stat {
        Stat::Strength => 0,
        Stat::Endurance => 1,
        Stat::PsionicAbility => 2,
        Stat::Agility => 3,
        Stat::CyberAffinity => 4,
    }
}

enum SkillRow {
    Tech(usize),
    Weapon(usize),
}

fn skill_row(skill: Skill) -> SkillRow {
    match skill {
        Skill::Hack => SkillRow::Tech(0),
        Skill::Repair => SkillRow::Tech(1),
        Skill::Modify => SkillRow::Tech(2),
        Skill::Maintenance => SkillRow::Tech(3),
        Skill::Research => SkillRow::Tech(4),
        Skill::StandardWeapons => SkillRow::Weapon(0),
        Skill::EnergyWeapons => SkillRow::Weapon(1),
        Skill::HeavyWeapons => SkillRow::Weapon(2),
        Skill::ExoticWeapons => SkillRow::Weapon(3),
    }
}

/// One panel row: display label + upgrade target.
struct TrainerRow {
    label: &'static str,
    target: TrainerTarget,
}

impl TrainerMode {
    fn rows(&self, world: &World) -> Vec<TrainerRow> {
        let row = |label, target| TrainerRow { label, target };
        match self {
            TrainerMode::Stats => vec![
                // Display order matches the original stats panel (PSI third).
                row("Strength", TrainerTarget::Stat(Stat::Strength)),
                row("Endurance", TrainerTarget::Stat(Stat::Endurance)),
                row("Psionics", TrainerTarget::Stat(Stat::PsionicAbility)),
                row("Agility", TrainerTarget::Stat(Stat::Agility)),
                row("Cybernetics", TrainerTarget::Stat(Stat::CyberAffinity)),
            ],
            TrainerMode::Tech => vec![
                row("Hack", TrainerTarget::Skill(Skill::Hack)),
                row("Repair", TrainerTarget::Skill(Skill::Repair)),
                row("Modify", TrainerTarget::Skill(Skill::Modify)),
                row("Maintain", TrainerTarget::Skill(Skill::Maintenance)),
                row("Research", TrainerTarget::Skill(Skill::Research)),
            ],
            TrainerMode::Weapons => {
                let mut rows = vec![
                    row("Standard", TrainerTarget::Skill(Skill::StandardWeapons)),
                    row("Energy", TrainerTarget::Skill(Skill::EnergyWeapons)),
                    row("Heavy", TrainerTarget::Skill(Skill::HeavyWeapons)),
                ];
                // The Exotic row is hidden until the AlienWeapons quest bit is
                // set (the original trainer panel gates its 4th weapon row on
                // the same quest bit).
                let alien_weapons = world
                    .borrow::<UniqueView<QuestInfo>>()
                    .map(|q| q.read_quest_bit_value("AlienWeapons").bits() != 0)
                    .unwrap_or(false);
                if alien_weapons {
                    rows.push(row("Exotic", TrainerTarget::Skill(Skill::ExoticWeapons)));
                }
                rows
            }
            TrainerMode::Psi => (1..=PSI_TIER_CAP)
                .map(|tier| TrainerRow {
                    label: psi_tier_label(tier),
                    target: TrainerTarget::PsiTier(tier),
                })
                .collect(),
        }
    }

    fn backdrop(&self) -> &'static str {
        match self {
            TrainerMode::Psi => "psitrain.pcx",
            _ => "train.pcx",
        }
    }
}

fn psi_tier_label(tier: i32) -> &'static str {
    match tier {
        1 => "Psi Tier 1",
        2 => "Psi Tier 2",
        3 => "Psi Tier 3",
        4 => "Psi Tier 4",
        _ => "Psi Tier 5",
    }
}

/// The current level shown for a row (stat/skill level, or the highest
/// unlocked psi tier as seen from this row: "owned"/"locked" text below).
fn row_level(stats: &PlayerStats, target: TrainerTarget) -> i32 {
    match target {
        TrainerTarget::Stat(stat) => stats.stat_level(stat),
        TrainerTarget::Skill(skill) => stats.skill_level(skill),
        TrainerTarget::PsiTier(_) => stats.psi_tier,
    }
}

pub struct TrainerGui {
    mode: TrainerMode,
}

impl TrainerGui {
    pub fn new(mode: TrainerMode) -> TrainerGui {
        TrainerGui { mode }
    }
}

#[derive(Clone, Debug, Default)]
pub struct TrainerGuiState {
    /// Feedback line shown in the description area (purchase result /
    /// refusal), like the original's overlay error text.
    message: Option<String>,
}

#[derive(Clone)]
pub enum TrainerGuiMsg {
    Buy(TrainerTarget),
}

// Row layout on the 188x296 panel (matches the original trainer panel, which
// draws five rows starting at y=21; ELBUTT art is the 138x28 generic row
// plate).
const ROW_X: f32 = 13.0;
const ROW_Y0: f32 = 21.0;
const ROW_PITCH: f32 = 34.0;
const ROW_W: f32 = 138.0;
const ROW_H: f32 = 28.0;

impl Gui<TrainerGuiState, TrainerGuiMsg> for TrainerGui {
    fn get_components(
        &self,
        _cursor: &Option<GuiCursor>,
        _entity_id: EntityId,
        world: &World,
        state: &TrainerGuiState,
    ) -> Vec<GuiComponent<TrainerGuiMsg>> {
        let mut components: Vec<GuiComponent<TrainerGuiMsg>> = vec![
            gui::image(self.mode.backdrop())
                .with_position(vec2(0.0, 0.0))
                .with_size(vec2(188.0, 296.0)),
        ];

        let Ok(quests) = world.borrow::<UniqueView<QuestInfo>>() else {
            return components;
        };
        let stats = quests.player_stats().clone();
        let costs = crate::difficulty::trainer_costs(world);
        drop(quests);

        for (i, row) in self.mode.rows(world).iter().enumerate() {
            let y = ROW_Y0 + ROW_PITCH * i as f32;
            components.push(
                gui::button(TrainerGuiMsg::Buy(row.target))
                    .with_position(vec2(ROW_X, y))
                    .with_size(vec2(ROW_W, ROW_H))
                    .with_image("elbutt0.pcx")
                    .with_label(row.label),
            );
            components.push(
                gui::text(row.label)
                    .with_position(vec2(ROW_X + 8.0, y + 3.0))
                    .with_size(vec2(80.0, 12.0)),
            );
            let level = row_level(&stats, row.target);
            let detail = match &costs {
                // No cost tables (non-retail gamesys): the rows are
                // unavailable, not maxed.
                None => "(offline)".to_string(),
                Some(costs) => match (row.target, upgrade_quote(costs, &stats, row.target)) {
                    (TrainerTarget::Stat(stat), _) if !stat_upgrade_available(stat) => {
                        "unavailable".to_string()
                    }
                    (TrainerTarget::PsiTier(_), Some(cost)) => {
                        format!("unlock: {} cm", cost)
                    }
                    (TrainerTarget::PsiTier(tier), None) if tier <= level => "owned".to_string(),
                    (TrainerTarget::PsiTier(_), None) => "locked".to_string(),
                    (_, Some(cost)) => format!("lvl {} > {}: {} cm", level, level + 1, cost),
                    (_, None) => format!("lvl {} (MAX)", level),
                },
            };
            components.push(
                gui::text(&detail)
                    .with_position(vec2(ROW_X + 8.0, y + 15.0))
                    .with_size(vec2(120.0, 10.0)),
            );
        }

        // Module pool counter (the original panel draws the pool at (68, 195)).
        components.push(
            gui::text(&format!("modules: {}", stats.cyber_modules))
                .with_position(vec2(48.0, 195.0))
                .with_size(vec2(120.0, 12.0)),
        );

        // Purchase feedback in the description area (13, 214).
        if let Some(message) = &state.message {
            components.push(
                gui::text(message)
                    .with_position(vec2(ROW_X, 214.0))
                    .with_size(vec2(160.0, 12.0)),
            );
        }

        components
    }

    fn get_config(&self) -> GuiConfig {
        GuiConfig {
            world_offset: Vector3::new(0.0, 0.0, -0.2),
            screen_size_in_pixels: Vector2::new(188.0, 296.0),
        }
    }

    fn handle_msg(
        &self,
        _entity_id: EntityId,
        world: &World,
        _state: &TrainerGuiState,
        msg: &TrainerGuiMsg,
    ) -> (TrainerGuiState, Effect) {
        let TrainerGuiMsg::Buy(target) = msg;
        // Pre-validate here for immediate panel feedback; the effect handler
        // re-validates and applies atomically (handle_msg must not mutate).
        let quests = world.borrow::<UniqueView<QuestInfo>>().unwrap();
        let stats = quests.player_stats();
        let costs = crate::difficulty::trainer_costs(world);
        let Some(costs) = costs else {
            return (
                TrainerGuiState {
                    message: Some("training offline".to_string()),
                },
                Effect::NoEffect,
            );
        };
        if matches!(target, TrainerTarget::Stat(stat) if !stat_upgrade_available(*stat)) {
            return (
                TrainerGuiState {
                    message: Some("Upgrade unavailable in this build.".to_string()),
                },
                Effect::NoEffect,
            );
        }
        match upgrade_quote(&costs, stats, *target) {
            // A psi tier beyond the next one is locked, not maxed - give the
            // faithful refusal for each.
            None => {
                let locked = matches!(target, TrainerTarget::PsiTier(t) if *t > stats.psi_tier + 1);
                (
                    TrainerGuiState {
                        message: Some(if locked {
                            "Error! Unlock the previous tier first.".to_string()
                        } else {
                            "Error! Already at maximum.".to_string()
                        }),
                    },
                    Effect::NoEffect,
                )
            }
            Some(cost) if cost > stats.cyber_modules => (
                TrainerGuiState {
                    message: Some("Error! Insufficient cyber modules.".to_string()),
                },
                Effect::NoEffect,
            ),
            Some(cost) => {
                let message = if matches!(target, TrainerTarget::Stat(Stat::Endurance)) {
                    let hp_bonus = world
                        .borrow::<UniqueView<crate::difficulty::GlobalDifficultyParams>>()
                        .map(|p| p.coefficients(quests.difficulty()).hp_per_endurance)
                        .unwrap_or(5);
                    format!("Max HP +{} (-{} cm)", hp_bonus, cost)
                } else {
                    format!("Upgrade complete (-{} cm)", cost)
                };
                (
                    TrainerGuiState {
                        message: Some(message),
                    },
                    Effect::TrainerPurchase { target: *target },
                )
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tables() -> TrainerCostTables {
        // The retail shock2.gam values (projects/flat-ui-panels.md §3.2).
        TrainerCostTables {
            stat_cost: [[3, 8, 15, 30, 50]; 5],
            tech_cost: [[10, 5, 8, 12, 25, 50]; 5],
            weapon_cost: [[12, 6, 8, 15, 36, 50]; 4],
            psi_cost: [
                [10, 3, 3, 3, 3, 3, 3, 3],
                [20, 5, 5, 5, 5, 5, 5, 5],
                [30, 8, 8, 8, 8, 8, 8, 8],
                [50, 12, 12, 12, 12, 12, 12, 12],
                [75, 20, 20, 20, 20, 20, 20, 20],
            ],
        }
    }

    #[test]
    fn stat_quotes_follow_statcost_and_cap_at_6() {
        let costs = tables();
        let mut stats = PlayerStats::new(); // endurance 1
        assert_eq!(
            upgrade_quote(&costs, &stats, TrainerTarget::Stat(Stat::Endurance)),
            Some(3)
        );
        stats.raise_stat(Stat::Endurance); // 2
        assert_eq!(
            upgrade_quote(&costs, &stats, TrainerTarget::Stat(Stat::Endurance)),
            Some(8)
        );
        for _ in 0..4 {
            stats.raise_stat(Stat::Endurance);
        } // 6 = cap
        assert_eq!(
            upgrade_quote(&costs, &stats, TrainerTarget::Stat(Stat::Endurance)),
            None
        );
    }

    #[test]
    fn skill_quotes_follow_the_tech_and_weapon_tables() {
        let costs = tables();
        let mut stats = PlayerStats::new(); // all skills 0
        // Tech level 0 -> 1 costs 10 (more than level 2 - the real table).
        assert_eq!(
            upgrade_quote(&costs, &stats, TrainerTarget::Skill(Skill::Hack)),
            Some(10)
        );
        stats.raise_skill(Skill::Hack); // 1
        assert_eq!(
            upgrade_quote(&costs, &stats, TrainerTarget::Skill(Skill::Hack)),
            Some(5)
        );
        // Weapon level 0 -> 1 costs 12.
        assert_eq!(
            upgrade_quote(&costs, &stats, TrainerTarget::Skill(Skill::StandardWeapons)),
            Some(12)
        );
        for _ in 0..6 {
            stats.raise_skill(Skill::Research);
        } // 6 = cap
        assert_eq!(
            upgrade_quote(&costs, &stats, TrainerTarget::Skill(Skill::Research)),
            None
        );
    }

    #[test]
    fn psi_tiers_unlock_sequentially() {
        let costs = tables();
        let mut stats = PlayerStats::new(); // tier 0
        assert_eq!(
            upgrade_quote(&costs, &stats, TrainerTarget::PsiTier(1)),
            Some(10)
        );
        // Tier 2 is locked until tier 1 is owned.
        assert_eq!(
            upgrade_quote(&costs, &stats, TrainerTarget::PsiTier(2)),
            None
        );
        apply_purchase(&mut stats, TrainerTarget::PsiTier(1));
        assert_eq!(stats.psi_tier, 1);
        // Tier 1 is now owned (not repurchasable); tier 2 quotes 20.
        assert_eq!(
            upgrade_quote(&costs, &stats, TrainerTarget::PsiTier(1)),
            None
        );
        assert_eq!(
            upgrade_quote(&costs, &stats, TrainerTarget::PsiTier(2)),
            Some(20)
        );
    }

    #[test]
    fn apply_purchase_raises_the_target() {
        let mut stats = PlayerStats::new();
        apply_purchase(&mut stats, TrainerTarget::Stat(Stat::Endurance));
        assert_eq!(stats.endurance, 2);
        apply_purchase(&mut stats, TrainerTarget::Skill(Skill::Repair));
        assert_eq!(stats.skills.repair, 1);
    }

    #[test]
    fn stat_trainer_only_quotes_upgrades_with_live_gameplay_effects() {
        let costs = tables();
        let stats = PlayerStats::new();

        assert_eq!(
            upgrade_quote(&costs, &stats, TrainerTarget::Stat(Stat::Endurance)),
            Some(3)
        );
        assert_eq!(
            upgrade_quote(&costs, &stats, TrainerTarget::Stat(Stat::CyberAffinity)),
            Some(3)
        );
        assert_eq!(
            upgrade_quote(&costs, &stats, TrainerTarget::Stat(Stat::Strength)),
            Some(3),
            "Strength is purchasable once backpack capacity consumes it"
        );
        assert_eq!(
            upgrade_quote(&costs, &stats, TrainerTarget::Stat(Stat::PsionicAbility)),
            Some(3),
            "PSI is purchasable now that psi casts scale with it"
        );
        assert_eq!(
            upgrade_quote(&costs, &stats, TrainerTarget::Stat(Stat::Agility)),
            None,
            "Agility must not be sold until it has a gameplay effect"
        );
    }
}
