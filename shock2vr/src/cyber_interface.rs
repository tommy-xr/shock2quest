//! Retail's campaign HideInterface flag gates cybernetic UI, not physical hands.
use dark::properties::QuestBitValue;
use shipyard::{UniqueView, World};

use crate::quest_info::QuestInfo;

pub const QUEST_NAME: &str = "HideInterface";
pub const NOT_INSTALLED: &str = "Cyber Interface software not installed.";

pub fn installed(world: &World) -> bool {
    world
        .borrow::<UniqueView<QuestInfo>>()
        .map(|quests| quests.read_quest_bit_value(QUEST_NAME).bits() == 0)
        .unwrap_or(true)
}

/// Retail new-game setup starts Earth without the HUD. Direct launches of other
/// missions retain their normal interface; saved/scripted values take priority,
/// except that MedSci1 always loads installed.
pub fn initialize(quests: &mut QuestInfo, mission: &str) {
    if mission.eq_ignore_ascii_case("earth.mis")
        && !quests
            .quest_bits()
            .iter()
            .any(|(name, _)| name.eq_ignore_ascii_case(QUEST_NAME))
    {
        quests.set_quest_bit_value(QUEST_NAME, QuestBitValue::INCOMPLETE);
    }
    // Graduation installs the interface, but saves from builds that skipped it
    // carry Earth's flag into MedSci1, where the player can't progress without it.
    if mission.eq_ignore_ascii_case("medsci1.mis")
        && quests.read_quest_bit_value(QUEST_NAME).bits() != 0
    {
        quests.set_quest_bit_value(QUEST_NAME, QuestBitValue::UNKNOWN);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn earth_starts_uninstalled_but_saved_and_scripted_values_survive() {
        let mut quests = QuestInfo::new();
        initialize(&mut quests, "medsci1.mis");
        assert_eq!(quests.read_quest_bit_value(QUEST_NAME).bits(), 0);
        initialize(&mut quests, "EARTH.MIS");
        assert_eq!(quests.read_quest_bit_value(QUEST_NAME).bits(), 1);
        quests.set_quest_bit_value(QUEST_NAME, QuestBitValue::UNKNOWN);
        let json = serde_json::to_string(&quests).unwrap();
        let mut restored: QuestInfo = serde_json::from_str(&json).unwrap();
        initialize(&mut restored, "earth.mis");
        let world = World::new();
        world.add_unique(restored);
        assert!(installed(&world));
    }

    #[test]
    fn medsci1_loads_installed_but_training_stays_hidden() {
        let mut quests = QuestInfo::new();
        quests.set_quest_bit_value(QUEST_NAME, QuestBitValue::INCOMPLETE);
        initialize(&mut quests, "station.mis");
        assert_eq!(quests.read_quest_bit_value(QUEST_NAME).bits(), 1);
        initialize(&mut quests, "MedSci1.mis");
        assert_eq!(quests.read_quest_bit_value(QUEST_NAME).bits(), 0);
    }
}
