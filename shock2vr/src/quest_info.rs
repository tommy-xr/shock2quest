///
/// quest_info.rs
///
/// Module keeping track of various quest-related items for player
///
use std::collections::{HashMap, HashSet};

use dark::properties::{KeyCard, QuestBitValue};
use serde::{Deserialize, Serialize};
use shipyard::Unique;

#[derive(Deserialize, Serialize, Unique, Clone, Debug)]
pub struct QuestInfo {
    quest_bit_values: HashMap<String, QuestBitValue>,
    played_emails: HashSet<String>,
    key_cards: Vec<KeyCard>,
}

impl QuestInfo {
    pub fn new() -> QuestInfo {
        QuestInfo {
            quest_bit_values: HashMap::new(),
            played_emails: HashSet::new(),
            key_cards: Vec::new(),
        }
    }

    pub fn add_key_card(&mut self, key_card: KeyCard) {
        self.key_cards.push(key_card)
    }

    pub fn can_unlock(&self, key_dst: &KeyCard) -> bool {
        let key_cards = &self.key_cards;
        key_cards.iter().any(|key| key.can_unlock(key_dst))
    }

    pub fn read_quest_bit_value(&self, quest_name: &str) -> QuestBitValue {
        *self
            .quest_bit_values
            .get(&quest_name.to_ascii_lowercase())
            .unwrap_or(&QuestBitValue::UNKNOWN)
    }

    pub fn set_quest_bit_value(&mut self, quest_name: &str, quest_value: QuestBitValue) {
        self.quest_bit_values
            .insert(quest_name.to_ascii_lowercase(), quest_value);
    }

    pub fn has_played_email(&self, email: &str) -> bool {
        self.played_emails.contains(email)
    }

    pub fn mark_email_as_played(&mut self, email: &str) {
        self.played_emails.insert(email.to_owned());
    }
}
