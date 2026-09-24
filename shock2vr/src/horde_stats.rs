//! Persisted battle totals. Enemy damage is aggregate HP lost, not attacker attribution.
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct HordeBattleStats {
    pub enemies_killed: u32,
    pub damage_taken: u64,
    pub enemy_hp_lost: u64,
}
impl HordeBattleStats {
    pub fn record(&mut self, player: bool, previous: i32, current: i32) {
        let lost = previous.max(0).saturating_sub(current.max(0)).max(0) as u64;
        if player {
            self.damage_taken = self.damage_taken.saturating_add(lost);
        } else {
            self.enemy_hp_lost = self.enemy_hp_lost.saturating_add(lost);
            if previous > 0 && current <= 0 {
                self.enemies_killed = self.enemies_killed.saturating_add(1);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn counts_actual_loss_without_healing_overkill_or_corpse_credit() {
        let mut stats = HordeBattleStats::default();
        stats.record(true, 40, 32);
        stats.record(true, 32, 40);
        stats.record(false, 12, 7);
        stats.record(false, 7, 0);
        stats.record(false, 0, 0);
        stats.record(false, 0, -90);
        assert_eq!(
            stats,
            HordeBattleStats {
                enemies_killed: 1,
                damage_taken: 8,
                enemy_hp_lost: 12
            }
        );
        let loaded: HordeBattleStats =
            serde_json::from_str(&serde_json::to_string(&stats).unwrap()).unwrap();
        assert_eq!(stats, loaded);
    }
}
