//! Player stat / skill model and the station training-tour reward table.
//!
//! System Shock 2's character is created by the earth->station recruit flow:
//! the player picks a service branch (`career.rs`) and then completes three
//! training tours - one per training "year" - on the recruit deck. Each tour
//! grants stats/skills/psi-disciplines hardcoded by (career, year, tour). The
//! retail engine keyed those grants by service+year+tour and stored them in
//! code; the shipped `res/strings/CHARGEN.STR` (`Mission1..Mission27`) is the
//! faithful *display* spec for the same grants. This module mirrors those 27
//! grants as a Rust table ([`tour_reward`]) - the numbers are authored here
//! (retail did the same); the STR strings are only used for the debrief text
//! lookup ([`TourReward::text_key`]), never parsed for the numbers.
//!
//! [`PlayerStats`] is the persistent character sheet. It is stored inside
//! [`crate::quest_info::QuestInfo`] (the same struct that carries career/quest
//! bits), so it rides the existing persistence path for free: it survives level
//! transitions (`QuestInfo` is threaded through `Game::save_active_scene` ->
//! `load_mission_into_scene`) and save/load (`QuestInfo` is embedded in
//! `SaveData::global_data`). This mirrors how hp/psi "persist": those are
//! re-derived from the persisted career quest bits on every mission load
//! (`mission_core.rs`); stats are the same idea but accumulate across tours.
//!
//! Deferred (storage + grants only, per issue #453): career-specific stat
//! baselines (all careers start from the uniform [`PlayerStats::default`]
//! baseline for now), and *derived gameplay effects* - Endurance->maxHP scaling
//! and wiring the granted psi disciplines into `crate::psi` known powers. Those
//! are noted here so a follow-up can pick them up.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::career::Career;

/// The five primary character statistics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stat {
    Strength,
    Endurance,
    Agility,
    PsionicAbility,
    CyberAffinity,
}

/// The nine trainable skills (weapon proficiencies + tech skills).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Skill {
    StandardWeapons,
    EnergyWeapons,
    HeavyWeapons,
    ExoticWeapons,
    Hack,
    Repair,
    Modify,
    Maintenance,
    Research,
}

/// The trainable skill levels. Skills start untrained (0) and are raised by
/// training tours. Named fields (rather than a map) keep the serialized shape
/// explicit and stable for the debug-runtime / SDK introspection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillLevels {
    pub standard_weapons: i32,
    pub energy_weapons: i32,
    pub heavy_weapons: i32,
    pub exotic_weapons: i32,
    pub hack: i32,
    pub repair: i32,
    pub modify: i32,
    pub maintenance: i32,
    pub research: i32,
}

impl SkillLevels {
    const fn zero() -> SkillLevels {
        SkillLevels {
            standard_weapons: 0,
            energy_weapons: 0,
            heavy_weapons: 0,
            exotic_weapons: 0,
            hack: 0,
            repair: 0,
            modify: 0,
            maintenance: 0,
            research: 0,
        }
    }

    fn get_mut(&mut self, skill: Skill) -> &mut i32 {
        match skill {
            Skill::StandardWeapons => &mut self.standard_weapons,
            Skill::EnergyWeapons => &mut self.energy_weapons,
            Skill::HeavyWeapons => &mut self.heavy_weapons,
            Skill::ExoticWeapons => &mut self.exotic_weapons,
            Skill::Hack => &mut self.hack,
            Skill::Repair => &mut self.repair,
            Skill::Modify => &mut self.modify,
            Skill::Maintenance => &mut self.maintenance,
            Skill::Research => &mut self.research,
        }
    }
}

/// The player's persistent character sheet: primary stats, trained skills, and
/// mastered psi disciplines, plus the set of training years already granted
/// (so a tour reward applies exactly once per year even if a tour marker
/// re-fires). Serialized as part of `QuestInfo`; see the module docs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlayerStats {
    pub strength: i32,
    pub endurance: i32,
    pub agility: i32,
    pub psionic_ability: i32,
    pub cyber_affinity: i32,
    pub skills: SkillLevels,
    /// Psi disciplines mastered via OSA training tours, by display name (e.g.
    /// "Cryokinesis"). Storage only for now; wiring these into the active psi
    /// power set is a deferred derived effect (see module docs).
    pub psi_disciplines: Vec<String>,
    /// Training years (1..=3) whose tour reward has already been applied.
    pub granted_years: BTreeSet<u32>,
    /// Cyber modules: the game's upgrade currency. The retail engine stores
    /// these as the stack count of a hidden "fake cookie" inventory object
    /// (`kEquipFakeCookies`); the pragmatic equivalent here is a persistent
    /// counter beside the rest of the character sheet. Awarded by
    /// `Effect::AwardXP` (quest/`PropExp` traps + `expcookie` pickups) and
    /// spent at trainer stations. `#[serde(default)]` keeps saves written
    /// before this field existed loadable (they load with 0 modules).
    #[serde(default)]
    pub cyber_modules: i32,
    /// Highest psi tier unlocked at a psi trainer (0..=5, sequential). Tier
    /// unlocks gate which psi powers can eventually be learned; per-power
    /// purchases are deferred (see `scripts::gui::trainer`).
    #[serde(default)]
    pub psi_tier: i32,
}

impl Default for PlayerStats {
    fn default() -> PlayerStats {
        // Baseline: every primary stat starts at the retail floor of 1, skills
        // untrained. Career-specific baselines are deferred (module docs), so
        // all careers currently share this baseline and diverge via their tours.
        PlayerStats {
            strength: 1,
            endurance: 1,
            agility: 1,
            psionic_ability: 1,
            cyber_affinity: 1,
            skills: SkillLevels::zero(),
            psi_disciplines: Vec::new(),
            granted_years: BTreeSet::new(),
            cyber_modules: 0,
            psi_tier: 0,
        }
    }
}

impl PlayerStats {
    pub fn new() -> PlayerStats {
        PlayerStats::default()
    }

    /// Award cyber modules (the upgrade currency). Negative or zero amounts are
    /// ignored - awards only ever add. Returns the new balance.
    pub fn award_cyber_modules(&mut self, amount: i32) -> i32 {
        if amount > 0 {
            self.cyber_modules = self.cyber_modules.saturating_add(amount);
        }
        self.cyber_modules
    }

    /// Attempt to spend `amount` cyber modules (a trainer purchase). Atomic
    /// check-and-decrement: spends and returns `true` only if the balance
    /// covers the cost, otherwise leaves the balance untouched and returns
    /// `false`. `amount <= 0` is a no-op that succeeds.
    pub fn spend_cyber_modules(&mut self, amount: i32) -> bool {
        if amount <= 0 {
            return true;
        }
        if self.cyber_modules >= amount {
            self.cyber_modules -= amount;
            true
        } else {
            false
        }
    }

    /// Current level of a primary stat.
    pub fn stat_level(&self, stat: Stat) -> i32 {
        match stat {
            Stat::Strength => self.strength,
            Stat::Endurance => self.endurance,
            Stat::Agility => self.agility,
            Stat::PsionicAbility => self.psionic_ability,
            Stat::CyberAffinity => self.cyber_affinity,
        }
    }

    /// Current level of a trainable skill.
    pub fn skill_level(&self, skill: Skill) -> i32 {
        match skill {
            Skill::StandardWeapons => self.skills.standard_weapons,
            Skill::EnergyWeapons => self.skills.energy_weapons,
            Skill::HeavyWeapons => self.skills.heavy_weapons,
            Skill::ExoticWeapons => self.skills.exotic_weapons,
            Skill::Hack => self.skills.hack,
            Skill::Repair => self.skills.repair,
            Skill::Modify => self.skills.modify,
            Skill::Maintenance => self.skills.maintenance,
            Skill::Research => self.skills.research,
        }
    }

    /// Raise a primary stat by one level (a trainer purchase).
    pub fn raise_stat(&mut self, stat: Stat) {
        *self.stat_mut(stat) += 1;
    }

    /// Raise a trainable skill by one level (a trainer purchase).
    pub fn raise_skill(&mut self, skill: Skill) {
        *self.skills.get_mut(skill) += 1;
    }

    fn stat_mut(&mut self, stat: Stat) -> &mut i32 {
        match stat {
            Stat::Strength => &mut self.strength,
            Stat::Endurance => &mut self.endurance,
            Stat::Agility => &mut self.agility,
            Stat::PsionicAbility => &mut self.psionic_ability,
            Stat::CyberAffinity => &mut self.cyber_affinity,
        }
    }

    /// Apply a (career, year, tour) training-tour reward, once per training
    /// year. Returns `true` if a reward was applied, `false` if this year was
    /// already granted or no reward exists for the key.
    pub fn apply_tour_reward(&mut self, career: Career, year: u32, tour: u32) -> bool {
        if self.granted_years.contains(&year) {
            return false;
        }
        let Some(reward) = tour_reward(career, year, tour) else {
            return false;
        };
        for (stat, amount) in reward.stats {
            *self.stat_mut(*stat) += *amount;
        }
        for (skill, amount) in reward.skills {
            *self.skills.get_mut(*skill) += *amount;
        }
        for discipline in reward.psi_disciplines {
            self.psi_disciplines.push((*discipline).to_string());
        }
        self.granted_years.insert(year);
        true
    }
}

/// One training-tour reward: the grants plus the `CHARGEN.STR` key of its
/// debrief text (so a future UI can show the "You've gained..." message).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TourReward {
    /// The `res/strings/CHARGEN.STR` key for this tour's debrief text
    /// (`Mission1..Mission27`). Display only; the grants below are authoritative.
    pub text_key: &'static str,
    pub stats: &'static [(Stat, i32)],
    pub skills: &'static [(Skill, i32)],
    /// Mastered psi disciplines, by display name (OSA year-1/3 tours).
    pub psi_disciplines: &'static [&'static str],
}

const fn reward(
    text_key: &'static str,
    stats: &'static [(Stat, i32)],
    skills: &'static [(Skill, i32)],
    psi_disciplines: &'static [&'static str],
) -> TourReward {
    TourReward {
        text_key,
        stats,
        skills,
        psi_disciplines,
    }
}

use Skill::*;
use Stat::*;

/// The 27 training-tour rewards, indexed `[career][year-1][tour]`. Career order
/// (Marine, Navy, OSA) matches `Career::from_service` (0/1/2) and the
/// CHARGEN.STR block order (`Mission1..9` Marine, `10..18` Navy, `19..27` OSA).
/// Each entry is annotated with its `MissionN` string.
#[rustfmt::skip]
static REWARDS: [[[TourReward; 3]; 3]; 3] = [
    // ===== Marine (career 0) =====
    [
        // Year 1
        [
            reward("Mission1", &[(Strength, 2)], &[], &[]),   // +2 Strength
            reward("Mission2", &[(Endurance, 2)], &[], &[]),  // +2 Endurance
            reward("Mission3", &[(Agility, 2)], &[], &[]),    // +2 Agility
        ],
        // Year 2
        [
            reward("Mission4", &[(CyberAffinity, 1)], &[(EnergyWeapons, 1)], &[]), // +1 Energy Weapons, +1 Cyber Affinity
            reward("Mission5", &[(CyberAffinity, 1)], &[(HeavyWeapons, 1)], &[]),  // +1 Heavy Weapons, +1 Cyber Affinity
            reward("Mission6", &[], &[(StandardWeapons, 2)], &[]),                 // +2 Standard Weapons
        ],
        // Year 3
        [
            reward("Mission7", &[], &[(Maintenance, 1)], &[]), // +1 Maintenance
            reward("Mission8", &[], &[(Modify, 1)], &[]),      // +1 Modify
            reward("Mission9", &[], &[(Repair, 1)], &[]),      // +1 Repair
        ],
    ],
    // ===== Navy (career 1) =====
    [
        // Year 1
        [
            reward("Mission10", &[(Strength, 1)], &[(Hack, 1)], &[]),   // +1 Hack, +1 Strength
            reward("Mission11", &[(Strength, 1)], &[(Repair, 1)], &[]), // +1 Repair, +1 Strength
            reward("Mission12", &[(Strength, 1)], &[(Modify, 1)], &[]), // +1 Modify, +1 Strength
        ],
        // Year 2
        [
            reward("Mission13", &[(CyberAffinity, 2)], &[], &[]),          // +2 Cyber Affinity
            reward("Mission14", &[], &[(Maintenance, 1)], &[]),            // +1 Maintenance
            reward("Mission15", &[], &[(StandardWeapons, 2)], &[]),        // +2 Standard Weapons
        ],
        // Year 3
        [
            reward("Mission16", &[], &[(Research, 1)], &[]),  // +1 Research
            reward("Mission17", &[(Endurance, 2)], &[], &[]), // +2 Endurance
            reward("Mission18", &[(Agility, 2)], &[], &[]),   // +2 Agility
        ],
    ],
    // ===== OSA (career 2) =====
    [
        // Year 1 - psi disciplines. Each Mission19-21 string also grants
        // "Level Two Psi disciplines" (Tier 2 access) on top of the two named
        // powers. That tier unlock is intentionally NOT stored here: it is a psi
        // capability gate with no reader until the deferred psi-discipline
        // gameplay wiring lands (see module docs) - representing it now would be
        // storage with no consumer. Only the two named mastered powers are
        // recorded, which is what the character sheet surfaces today.
        [
            reward("Mission19", &[], &[], &["Cryokinesis", "Psychogenic Cyber Affinity"]), // mastered Cryokinesis + Psychogenic Cyber Affinity
            reward("Mission20", &[], &[], &["Cryokinesis", "Kinetic Redirection"]),        // mastered Cryokinesis + Kinetic Redirection
            reward("Mission21", &[], &[], &["Cryokinesis", "Psycho-reflective Screen"]),   // mastered Cryokinesis + Psycho-reflective Screen
        ],
        // Year 2
        [
            reward("Mission22", &[(PsionicAbility, 2)], &[], &[]), // +2 Psionic Ability
            reward("Mission23", &[], &[(Research, 1)], &[]),       // +1 Research
            reward("Mission24", &[(Endurance, 2)], &[], &[]),      // +2 Endurance
        ],
        // Year 3 - +1 STR/+1 AGI/+1 CYB plus a mastered psi discipline
        [
            reward("Mission25", &[(Strength, 1), (Agility, 1), (CyberAffinity, 1)], &[], &["Psychogenic Agility"]),      // + Psychogenic Agility
            reward("Mission26", &[(Strength, 1), (Agility, 1), (CyberAffinity, 1)], &[], &["Neuro-Reflex Dampening"]),   // + Neuro-Reflex Dampening
            reward("Mission27", &[(Strength, 1), (Agility, 1), (CyberAffinity, 1)], &[], &["Remote Electron Tampering"]), // + Remote Electron Tampering
        ],
    ],
];

/// Career index into [`REWARDS`], matching `Career::from_service` (0/1/2) and
/// the CHARGEN.STR block order.
fn career_index(career: Career) -> usize {
    match career {
        Career::Marine => 0,
        Career::Navy => 1,
        Career::Osa => 2,
    }
}

/// The reward for completing tour `tour` (0..=2) of training year `year`
/// (1..=3) for the given career, or `None` if the indices are out of range.
pub fn tour_reward(career: Career, year: u32, tour: u32) -> Option<&'static TourReward> {
    if year < 1 || year > 3 || tour > 2 {
        return None;
    }
    Some(&REWARDS[career_index(career)][(year - 1) as usize][tour as usize])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_is_fully_populated_with_unique_text_keys() {
        let mut keys = BTreeSet::new();
        for career in Career::ALL {
            for year in 1..=3 {
                for tour in 0..=2 {
                    let r = tour_reward(career, year, tour).unwrap_or_else(|| {
                        panic!("missing reward {:?} y{} t{}", career, year, tour)
                    });
                    assert!(keys.insert(r.text_key), "duplicate text key {}", r.text_key);
                    // Every entry grants something (stats, skills, or disciplines).
                    let total = r.stats.len() + r.skills.len() + r.psi_disciplines.len();
                    assert!(total > 0, "empty reward for {}", r.text_key);
                }
            }
        }
        assert_eq!(keys.len(), 27, "expected all 27 MissionN keys");
        // Keys should be exactly Mission1..=Mission27.
        for n in 1..=27 {
            assert!(keys.contains(format!("Mission{}", n).as_str()));
        }
    }

    #[test]
    fn out_of_range_returns_none() {
        assert!(tour_reward(Career::Marine, 0, 0).is_none());
        assert!(tour_reward(Career::Marine, 4, 0).is_none());
        assert!(tour_reward(Career::Marine, 1, 3).is_none());
    }

    #[test]
    fn spot_check_exact_grants() {
        // Marine Y1 T0 (Mission1): +2 Strength.
        let r = tour_reward(Career::Marine, 1, 0).unwrap();
        assert_eq!(r.text_key, "Mission1");
        assert_eq!(r.stats, &[(Stat::Strength, 2)]);

        // Marine Y2 T0 (Mission4): +1 Energy Weapons, +1 Cyber Affinity.
        let r = tour_reward(Career::Marine, 2, 0).unwrap();
        assert_eq!(r.text_key, "Mission4");
        assert_eq!(r.stats, &[(Stat::CyberAffinity, 1)]);
        assert_eq!(r.skills, &[(Skill::EnergyWeapons, 1)]);

        // Marine Y3 T0 (Mission7): +1 Maintenance.
        let r = tour_reward(Career::Marine, 3, 0).unwrap();
        assert_eq!(r.text_key, "Mission7");
        assert_eq!(r.skills, &[(Skill::Maintenance, 1)]);

        // Navy Y1 T0 (Mission10): +1 Hack, +1 Strength.
        let r = tour_reward(Career::Navy, 1, 0).unwrap();
        assert_eq!(r.text_key, "Mission10");
        assert_eq!(r.stats, &[(Stat::Strength, 1)]);
        assert_eq!(r.skills, &[(Skill::Hack, 1)]);

        // OSA Y1 T1 (Mission20): Cryokinesis + Kinetic Redirection.
        let r = tour_reward(Career::Osa, 1, 1).unwrap();
        assert_eq!(r.text_key, "Mission20");
        assert_eq!(r.psi_disciplines, &["Cryokinesis", "Kinetic Redirection"]);

        // OSA Y3 T2 (Mission27): +1 STR/+1 AGI/+1 CYB + Remote Electron Tampering.
        let r = tour_reward(Career::Osa, 3, 2).unwrap();
        assert_eq!(r.text_key, "Mission27");
        assert_eq!(
            r.stats,
            &[
                (Stat::Strength, 1),
                (Stat::Agility, 1),
                (Stat::CyberAffinity, 1)
            ]
        );
        assert_eq!(r.psi_disciplines, &["Remote Electron Tampering"]);
    }

    #[test]
    fn apply_marine_tour0_chain_accumulates() {
        // The Marine chain the station-flow e2e walks: tour 0 of each year.
        let mut stats = PlayerStats::new();
        let base_str = stats.strength;
        let base_cyb = stats.cyber_affinity;

        assert!(stats.apply_tour_reward(Career::Marine, 1, 0)); // +2 STR
        assert_eq!(stats.strength, base_str + 2);

        assert!(stats.apply_tour_reward(Career::Marine, 2, 0)); // +1 EnergyWeapons, +1 CYB
        assert_eq!(stats.skills.energy_weapons, 1);
        assert_eq!(stats.cyber_affinity, base_cyb + 1);

        assert!(stats.apply_tour_reward(Career::Marine, 3, 0)); // +1 Maintenance
        assert_eq!(stats.skills.maintenance, 1);

        assert_eq!(stats.granted_years, BTreeSet::from([1, 2, 3]));
    }

    #[test]
    fn tour_reward_applies_once_per_year() {
        let mut stats = PlayerStats::new();
        assert!(stats.apply_tour_reward(Career::Marine, 1, 0)); // +2 STR
        assert_eq!(stats.strength, 3);
        // Re-firing the same year (even a different tour) is a no-op.
        assert!(!stats.apply_tour_reward(Career::Marine, 1, 1));
        assert!(!stats.apply_tour_reward(Career::Marine, 1, 0));
        assert_eq!(stats.strength, 3);
    }

    #[test]
    fn cyber_modules_award() {
        let mut stats = PlayerStats::new();
        assert_eq!(stats.cyber_modules, 0);
        assert_eq!(stats.award_cyber_modules(4), 4);
        assert_eq!(stats.award_cyber_modules(3), 7);
        // Non-positive awards are ignored.
        assert_eq!(stats.award_cyber_modules(0), 7);
        assert_eq!(stats.award_cyber_modules(-5), 7);
    }

    #[test]
    fn cyber_modules_survive_serde_and_default_for_old_saves() {
        let mut stats = PlayerStats::new();
        stats.award_cyber_modules(12);
        let json = serde_json::to_string(&stats).unwrap();
        let back: PlayerStats = serde_json::from_str(&json).unwrap();
        assert_eq!(back.cyber_modules, 12);
        // A save written before this field existed (no `cyber_modules` key)
        // loads with the serde default of 0.
        let legacy = r#"{"strength":1,"endurance":1,"agility":1,"psionic_ability":1,"cyber_affinity":1,"skills":{"standard_weapons":0,"energy_weapons":0,"heavy_weapons":0,"exotic_weapons":0,"hack":0,"repair":0,"modify":0,"maintenance":0,"research":0},"psi_disciplines":[],"granted_years":[]}"#;
        let loaded: PlayerStats = serde_json::from_str(legacy).unwrap();
        assert_eq!(loaded.cyber_modules, 0);
    }

    #[test]
    fn stats_round_trip_through_serde() {
        let mut stats = PlayerStats::new();
        stats.apply_tour_reward(Career::Osa, 1, 0);
        stats.apply_tour_reward(Career::Osa, 3, 0);
        let json = serde_json::to_string(&stats).unwrap();
        let back: PlayerStats = serde_json::from_str(&json).unwrap();
        assert_eq!(stats, back);
        assert_eq!(
            back.psi_disciplines,
            vec![
                "Cryokinesis".to_string(),
                "Psychogenic Cyber Affinity".to_string(),
                "Psychogenic Agility".to_string()
            ]
        );
    }
}
