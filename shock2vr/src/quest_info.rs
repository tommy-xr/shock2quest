///
/// quest_info.rs
///
/// Module keeping track of various quest-related items for player
///
use std::collections::{BTreeSet, HashMap, HashSet};

use dark::properties::{KeyCard, QuestBitValue};
use serde::{Deserialize, Serialize};
use shipyard::Unique;

use crate::player_stats::PlayerStats;

/// One audio log the player has collected (frobbed), keyed by its per-deck
/// identity - the original stored these as `Logs<deck>` bitmasks; we keep the
/// lightweight `(deck, log)` identity and re-resolve the transcript/portrait
/// from the string tables when the reader shows it. Persisted in `QuestInfo`
/// so the collection survives level transitions and save/load.
#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub struct CollectedLog {
    pub deck: u32,
    pub log: u32,
}

#[derive(Deserialize, Serialize, Unique, Clone, Debug)]
pub struct QuestInfo {
    quest_bit_values: HashMap<String, QuestBitValue>,
    played_emails: HashSet<String>,
    key_cards: Vec<KeyCard>,
    /// The player's persistent character sheet (primary stats, skills, mastered
    /// psi disciplines). Lives here so it survives level transitions and
    /// save/load on the same path as the career/quest bits. `#[serde(default)]`
    /// keeps older save files (written before this field existed) loadable.
    #[serde(default)]
    player_stats: PlayerStats,
    /// Audio logs the player has collected, in pickup order (the original's
    /// `LOGTIMES` sort). `#[serde(default)]` keeps pre-collection saves loadable.
    #[serde(default)]
    collected_logs: Vec<CollectedLog>,
    /// Automap locations the player has explored, per mission (lowercase level
    /// file name -> ordered location set). The original's mission-scoped
    /// `EXPLORED[64]` file-var; keyed per mission here so deck re-entry keeps
    /// its reveal state. `#[serde(default)]` keeps older saves loadable.
    #[serde(default)]
    explored_maps: HashMap<String, BTreeSet<i32>>,
}

impl QuestInfo {
    pub fn new() -> QuestInfo {
        QuestInfo {
            quest_bit_values: HashMap::new(),
            played_emails: HashSet::new(),
            key_cards: Vec::new(),
            player_stats: PlayerStats::new(),
            collected_logs: Vec::new(),
            explored_maps: HashMap::new(),
        }
    }

    /// Mark an automap location explored for `mission` (lowercase level file
    /// name, e.g. "medsci1.mis"). Returns `true` when newly revealed.
    pub fn reveal_map_location(&mut self, mission: &str, location: i32) -> bool {
        self.explored_maps
            .entry(mission.to_ascii_lowercase())
            .or_default()
            .insert(location)
    }

    /// The explored automap locations for `mission`, ascending.
    pub fn explored_map_locations(&self, mission: &str) -> Vec<i32> {
        self.explored_maps
            .get(&mission.to_ascii_lowercase())
            .map(|set| set.iter().copied().collect())
            .unwrap_or_default()
    }

    /// Record an audio log into the collection (no-op if already collected).
    /// Returns `true` when it was newly added.
    pub fn collect_log(&mut self, deck: u32, log: u32) -> bool {
        if self.has_collected_log(deck, log) {
            return false;
        }
        self.collected_logs.push(CollectedLog { deck, log });
        true
    }

    pub fn has_collected_log(&self, deck: u32, log: u32) -> bool {
        self.collected_logs
            .iter()
            .any(|c| c.deck == deck && c.log == log)
    }

    /// The player's collected audio logs, in pickup order.
    pub fn collected_logs(&self) -> &[CollectedLog] {
        &self.collected_logs
    }

    /// The player's persistent character sheet.
    pub fn player_stats(&self) -> &PlayerStats {
        &self.player_stats
    }

    /// Mutable access to the player's character sheet (e.g. to apply a training
    /// tour reward).
    pub fn player_stats_mut(&mut self) -> &mut PlayerStats {
        &mut self.player_stats
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

    /// Remove a quest bit, resetting it to the pristine `UNKNOWN` state (absent
    /// from the map, so it no longer appears in `quest_bits()`). Reads still
    /// return `UNKNOWN`, same as a never-set bit. Used by debug tooling to reset
    /// an objective; game scripts use `set_quest_bit_value` directly.
    pub fn clear_quest_bit_value(&mut self, quest_name: &str) {
        self.quest_bit_values
            .remove(&quest_name.to_ascii_lowercase());
    }

    /// All quest bits the game has touched, as `(name, value)` pairs. Bits never
    /// referenced are absent (they read as `UNKNOWN`). Used by debug tooling to
    /// snapshot objective progress. Order is unspecified (HashMap iteration).
    pub fn quest_bits(&self) -> Vec<(String, QuestBitValue)> {
        self.quest_bit_values
            .iter()
            .map(|(name, value)| (name.clone(), *value))
            .collect()
    }

    pub fn has_played_email(&self, email: &str) -> bool {
        self.played_emails.contains(email)
    }

    pub fn mark_email_as_played(&mut self, email: &str) {
        self.played_emails.insert(email.to_owned());
    }
}
