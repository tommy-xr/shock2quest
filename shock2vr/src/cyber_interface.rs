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
/// missions retain their normal interface; saved/scripted values take priority.
pub fn initialize(quests: &mut QuestInfo, mission: &str) {
    if mission.eq_ignore_ascii_case("earth.mis")
        && !quests
            .quest_bits()
            .iter()
            .any(|(name, _)| name.eq_ignore_ascii_case(QUEST_NAME))
    {
        quests.set_quest_bit_value(QUEST_NAME, QuestBitValue::INCOMPLETE);
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
}
