//! Player career branching for the station recruit deck.
//!
//! System Shock 2 opens on the recruitment intro (`earth.mis`), where the player
//! picks one of three service branches - Marine (combat), Navy (tech), or OSA
//! (psi) - by walking through the matching career door, then rides through the
//! recruit station and deploys to the Von Braun (MedSci 1) with a career-
//! appropriate build. Choosing a branch fires that door's `ChooseServiceScript`
//! marker (which carries a `P$Service` value), registering the career as a
//! persisted quest bit so it survives every level transition, then it is applied
//! to the player on each mission load.
//!
//! The per-career attribute table here is a faithful-in-spirit starting point,
//! not the exact retail OS-unit numbers: it emphasises each branch (Marines are
//! tanky, OSA are psionic) so the three careers arrive observably different.
//! A full skill/stat/cyber-module system is deferred (issue #424).

use dark::properties::QuestBitValue;

use crate::quest_info::QuestInfo;
use crate::scripts::Effect;

/// `PsiPull` (Kinetic Redirection) - a tier-1 offensive psi power. OSA recruits
/// deploy trained in it on top of the default Projected Cryokinesis.
const PSIPULL_TEMPLATE_ID: i32 = -1022;

/// The three service branches a recruit can enlist in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Career {
    Marine,
    Navy,
    Osa,
}

/// The starting attributes a career deploys with. Applied as absolute values
/// (not deltas) so re-applying on every level load is idempotent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CareerLoadout {
    /// Current and maximum hit points on deployment.
    pub max_hit_points: i32,
    /// Current and maximum psi points on deployment.
    pub max_psi_points: i32,
    /// An extra psi power the career starts trained in (beyond the default
    /// Cryokinesis), by template id. `None` for non-psi careers.
    pub extra_psi_power: Option<i32>,
}

impl Career {
    pub const ALL: [Career; 3] = [Career::Marine, Career::Navy, Career::Osa];

    /// Map a `P$Service` value (0 = Marines, 1 = Navy, 2 = OSA) to a career.
    pub fn from_service(service: u32) -> Career {
        match service {
            1 => Career::Navy,
            2 => Career::Osa,
            _ => Career::Marine,
        }
    }

    /// The persisted quest bit that records this career choice.
    pub fn quest_bit(&self) -> &'static str {
        match self {
            Career::Marine => "career_marine",
            Career::Navy => "career_navy",
            Career::Osa => "career_osa",
        }
    }

    /// The career the player selected, read from the persisted quest bits, or
    /// `None` if no branch was chosen (the player template defaults stand).
    pub fn from_quest_info(quest_info: &QuestInfo) -> Option<Career> {
        Career::ALL
            .into_iter()
            .find(|c| quest_info.read_quest_bit_value(c.quest_bit()) == QuestBitValue::COMPLETE)
    }

    /// The career-appropriate starting attributes applied on deployment.
    pub fn loadout(&self) -> CareerLoadout {
        match self {
            // Marines: front-line combat - tanky, minimal psi.
            Career::Marine => CareerLoadout {
                max_hit_points: 45,
                max_psi_points: 20,
                extra_psi_power: None,
            },
            // Navy: technical - balanced (tech/repair skills are deferred).
            Career::Navy => CareerLoadout {
                max_hit_points: 35,
                max_psi_points: 35,
                extra_psi_power: None,
            },
            // OSA: psi operative - frail but psionically potent, deploys with an
            // extra offensive power.
            Career::Osa => CareerLoadout {
                max_hit_points: 30,
                max_psi_points: 60,
                extra_psi_power: Some(PSIPULL_TEMPLATE_ID),
            },
        }
    }

    /// Effects that register this career choice: set its bit `COMPLETE` and
    /// clear the other two, so the three branches stay mutually exclusive.
    pub fn select_effects(&self) -> Vec<Effect> {
        Career::ALL
            .into_iter()
            .map(|c| Effect::SetQuestBit {
                quest_bit_name: c.quest_bit().to_string(),
                quest_bit_value: if c == *self {
                    QuestBitValue::COMPLETE
                } else {
                    QuestBitValue::UNKNOWN
                },
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn service_maps_to_career() {
        assert_eq!(Career::from_service(0), Career::Marine);
        assert_eq!(Career::from_service(1), Career::Navy);
        assert_eq!(Career::from_service(2), Career::Osa);
        assert_eq!(Career::from_service(99), Career::Marine);
    }

    #[test]
    fn careers_arrive_observably_different() {
        let marine = Career::Marine.loadout();
        let navy = Career::Navy.loadout();
        let osa = Career::Osa.loadout();

        // Hit points and psi points differ across all three branches.
        assert_ne!(marine.max_hit_points, navy.max_hit_points);
        assert_ne!(navy.max_hit_points, osa.max_hit_points);
        assert_ne!(marine.max_hit_points, osa.max_hit_points);
        assert_ne!(marine.max_psi_points, navy.max_psi_points);
        assert_ne!(navy.max_psi_points, osa.max_psi_points);

        // Only OSA deploys with an extra psi power.
        assert!(marine.extra_psi_power.is_none());
        assert!(osa.extra_psi_power.is_some());
    }

    #[test]
    fn select_makes_careers_mutually_exclusive() {
        let mut quest_info = QuestInfo::new();
        for effect in Career::Osa.select_effects() {
            if let Effect::SetQuestBit {
                quest_bit_name,
                quest_bit_value,
            } = effect
            {
                quest_info.set_quest_bit_value(&quest_bit_name, quest_bit_value);
            }
        }

        assert_eq!(Career::from_quest_info(&quest_info), Some(Career::Osa));

        // Switching branch clears the previous one.
        for effect in Career::Marine.select_effects() {
            if let Effect::SetQuestBit {
                quest_bit_name,
                quest_bit_value,
            } = effect
            {
                quest_info.set_quest_bit_value(&quest_bit_name, quest_bit_value);
            }
        }

        assert_eq!(Career::from_quest_info(&quest_info), Some(Career::Marine));
    }

    #[test]
    fn no_choice_reads_as_none() {
        assert_eq!(Career::from_quest_info(&QuestInfo::new()), None);
    }
}
