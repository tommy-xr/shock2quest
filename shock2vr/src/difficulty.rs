//! Authored campaign difficulty consumers. The campaign choice lives in QuestInfo.
use crate::{mission::PlayerInfo, player_stats::PlayerStats, quest_info::QuestInfo};
use dark::gamesys::{Difficulty, DifficultyParams, Gamesys, PlayerPoolParams};
use shipyard::{Get, Unique, UniqueView, ViewMut, World};

#[derive(Clone, Debug, Default, Unique)]
pub struct GlobalDifficultyParams {
    pub difficulty: Option<DifficultyParams>,
    pub player_pools: PlayerPoolParams,
}
impl GlobalDifficultyParams {
    pub fn from_gamesys(gamesys: &Gamesys) -> Self {
        Self {
            difficulty: gamesys.difficulty_params().cloned(),
            player_pools: gamesys.player_pool_params().copied().unwrap_or_default(),
        }
    }
    pub fn coefficients(&self, difficulty: Difficulty) -> PlayerPoolParams {
        let Some(p) = &self.difficulty else {
            return self.player_pools;
        };
        let i = difficulty.retail_index();
        let fallback = |value, default| if value == 0 { default } else { value };
        PlayerPoolParams {
            base_hp: fallback(p.base_hp[i], self.player_pools.base_hp),
            hp_per_endurance: fallback(p.hp_per_endurance[i], self.player_pools.hp_per_endurance),
            base_psi: fallback(p.base_psi[i], self.player_pools.base_psi),
            psi_per_psionics: fallback(p.psi_per_psionics[i], self.player_pools.psi_per_psionics),
        }
    }
    pub fn limits(&self, difficulty: Difficulty, stats: &PlayerStats) -> (i32, i32) {
        let p = self.coefficients(difficulty);
        // The current sheet contains base stats; when temporary stat modifiers
        // gain a consumer, only HP should use effective Endurance. Psi uses base PSI.
        let tank = if stats.has_os_trait(crate::scripts::gui::TRAIT_TANK) {
            crate::scripts::gui::TANK_HP_BONUS
        } else {
            0
        };
        let tier = stats.psi_tier.clamp(0, crate::player_stats::PSI_TIER_CAP);
        let hp = p
            .base_hp
            .saturating_add(
                p.hp_per_endurance
                    .saturating_mul(stats.endurance.clamp(1, 8)),
            )
            .saturating_add(tank)
            .max(1);
        let psi = p
            .base_psi
            .saturating_add(
                p.psi_per_psionics
                    .saturating_mul(stats.psionic_ability.clamp(1, 8)),
            )
            .saturating_add(tier * (tier + 1))
            .max(0);
        (hp, psi)
    }
}

/// Recompute after a stat/tier/trait change, preserving the deficit from the old
/// maximum. `fill` is only for construction of a fresh player; saved vitals are
/// restored afterwards. Repeating a recalculation with the same sheet is inert.
pub fn refresh_player_pools(world: &World, fill: bool) {
    let Ok(player) = world.borrow::<UniqueView<PlayerInfo>>() else {
        return;
    };
    let Ok(quests) = world.borrow::<UniqueView<QuestInfo>>() else {
        return;
    };
    let Ok(params) = world.borrow::<UniqueView<GlobalDifficultyParams>>() else {
        return;
    };
    let (max_hp, max_psi) = params.limits(quests.difficulty(), quests.player_stats());
    world.run(
        |mut hp: ViewMut<dark::properties::PropHitPoints>,
         mut maximum: ViewMut<dark::properties::PropMaxHitPoints>,
         mut psi: ViewMut<dark::properties::PropPsiState>| {
            if let (Ok(hp), Ok(maximum)) = (
                (&mut hp).get(player.entity_id),
                (&mut maximum).get(player.entity_id),
            ) {
                let old = maximum.hit_points.min(i32::MAX as u32) as i32;
                hp.hit_points = shifted_pool(hp.hit_points, old, max_hp, fill, true);
                maximum.hit_points = max_hp as u32;
            }
            if let Ok(psi) = (&mut psi).get(player.entity_id) {
                psi.psi_points =
                    shifted_pool(psi.psi_points, psi.max_psi_points, max_psi, fill, false);
                psi.max_psi_points = max_psi;
            }
        },
    );
}
fn shifted_pool(current: i32, old_max: i32, new_max: i32, fill: bool, health: bool) -> i32 {
    if fill {
        return new_max;
    }
    // An already-dead player must not be resurrected by a queued upgrade.
    if health && current <= 0 {
        return 0;
    }
    current
        .saturating_add(new_max.saturating_sub(old_max))
        .clamp(if health { 1 } else { 0 }, new_max)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn retail() -> GlobalDifficultyParams {
        GlobalDifficultyParams {
            difficulty: Some(DifficultyParams {
                trainer_multiplier: [1.0, 0.85, 1.0, 1.4, 1.8, 1.4],
                base_hp: [30, 45, 30, 24, 7, 24],
                hp_per_endurance: [5, 10, 5, 3, 3, 3],
                base_psi: [5, 10, 5, 3, 1, 5],
                psi_per_psionics: [10, 16, 10, 8, 5, 10],
                loot_discard_threshold: [0, 0, 0, 30, 75, 0],
                replicator_multiplier: [1.0, 0.85, 1.0, 1.25, 2.0, 1.0],
            }),
            ..Default::default()
        }
    }
    #[test]
    fn difficulty_pools_follow_stats_traits_and_unlocked_tiers() {
        let params = retail();
        let mut stats = PlayerStats::new();
        for (d, expected) in
            Difficulty::ALL
                .into_iter()
                .zip([(55, 26), (35, 15), (27, 11), (10, 6)])
        {
            assert_eq!(params.limits(d, &stats), expected);
        }
        stats.endurance = 3;
        stats.psionic_ability = 2;
        stats.psi_tier = 3;
        stats.add_os_trait(crate::scripts::gui::TRAIT_TANK);
        assert_eq!(params.limits(Difficulty::Impossible, &stats), (21, 23));
    }
    #[test]
    fn difficulty_zero_coefficients_use_authored_statparam() {
        let mut params = retail();
        let p = params.difficulty.as_mut().unwrap();
        p.base_hp[2] = 0;
        p.hp_per_endurance[2] = 0;
        p.base_psi[2] = 0;
        p.psi_per_psionics[2] = 0;
        params.player_pools = PlayerPoolParams {
            base_hp: 40,
            hp_per_endurance: 7,
            base_psi: 12,
            psi_per_psionics: 9,
        };
        assert_eq!(
            params.limits(Difficulty::Normal, &PlayerStats::new()),
            (47, 21)
        );
    }
    #[test]
    fn difficulty_recalculation_keeps_deficits_and_does_not_revive() {
        assert_eq!(shifted_pool(25, 35, 40, false, true), 30);
        assert_eq!(shifted_pool(30, 40, 40, false, true), 30);
        assert_eq!(shifted_pool(0, 35, 40, false, true), 0);
        assert_eq!(shifted_pool(1, 40, 10, false, true), 1);
        assert_eq!(shifted_pool(2, 15, 6, false, false), 0);
        assert_eq!(shifted_pool(2, 15, 6, true, false), 6);
    }
}
