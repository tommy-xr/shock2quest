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
use crate::research::ResearchState;

/// One audio log the player has collected (frobbed), keyed by its per-deck
/// identity - the original stored these as `Logs<deck>` bitmasks; we keep the
/// lightweight `(deck, log)` identity and re-resolve the transcript/portrait
/// from the string tables when the reader shows it. Persisted in `QuestInfo`
/// so the collection survives level transitions and save/load.
#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub struct CollectedLog {
    pub deck: u32,
    pub log: u32,
    /// Whether the player has actually played this log back. Collecting a disc
    /// only files it in the PDA; the reader is opened explicitly (the
    /// `ReadLastUnreadLog` action). `#[serde(default)]` keeps saves written
    /// before read tracking loadable - their logs restore as unread.
    #[serde(default)]
    pub read: bool,
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
    /// Campaign-wide research progress, keyed by stable gamesys archetype.
    #[serde(default)]
    research: ResearchState,
    /// The gamesys weapon archetype of the last weapon the player stowed over
    /// a shoulder, so an empty grip there draws it back
    /// (`body_frame::AnchorGesture::Draw`). A canonical *class* template id
    /// rather than an entity: the concrete weapon is re-found in the pack with
    /// `carried_weapon_by_class`, which survives save/load and level
    /// transitions where a runtime entity id would not.
    #[serde(default)]
    last_stowed_weapon: Option<i32>,
    /// The gamesys weapon archetype sitting in the hip holster, if any. A
    /// *class* template id for the same reason `last_stowed_weapon` is one: the
    /// concrete weapon stays in the pack (so its ammo and condition are the
    /// live entity's, never re-minted), and is re-found by class after a save or
    /// a level change, where a runtime entity id would not survive.
    ///
    /// The cost of keying by class: the slot names a *kind* of weapon, not one
    /// weapon. Losing the holstered pistol empties the thigh, but acquiring
    /// another pistol later fills it again without a dock, and with two in the
    /// pack the one drawn is whichever the carried list yields first.
    #[serde(default)]
    holstered_weapon: Option<i32>,
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
            research: ResearchState::default(),
            last_stowed_weapon: None,
            holstered_weapon: None,
        }
    }

    pub fn research(&self) -> &ResearchState {
        &self.research
    }

    pub fn research_mut(&mut self) -> &mut ResearchState {
        &mut self.research
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
        self.collected_logs.push(CollectedLog {
            deck,
            log,
            read: false,
        });
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

    /// The most recently collected log the player has not played back yet.
    ///
    /// The original gathers every unread log across all decks, sorts them by
    /// acquisition time ascending and plays the *last* one - i.e. the newest
    /// unread disc, not the oldest backlog entry. `collected_logs` is kept in
    /// pickup order, so the newest unread is the last matching entry.
    pub fn last_unread_log(&self) -> Option<&CollectedLog> {
        self.collected_logs.iter().rev().find(|entry| !entry.read)
    }

    /// Candidates for the reader/replay control, in preference order: retail's
    /// newest-unread-first ordering, then the already-read entries (newest
    /// first) so the same control remains a replay affordance once everything
    /// is read.
    ///
    /// This is a sequence rather than a single target because the caller has
    /// to resolve each candidate's localized reader strings, which can fail
    /// (a log collected on an earlier deck, a missing transcript). Offering
    /// only the front entry would let one unresolvable log wedge the reader
    /// forever; the caller skips to the next candidate instead.
    pub fn logs_for_reader(&self) -> impl Iterator<Item = &CollectedLog> {
        let unread = self.collected_logs.iter().rev().filter(|entry| !entry.read);
        let read = self.collected_logs.iter().rev().filter(|entry| entry.read);
        unread.chain(read)
    }

    /// Mark a collected log as played back. Returns `true` when this flipped it
    /// from unread (re-reading an already-read log is a no-op).
    pub fn mark_log_read(&mut self, deck: u32, log: u32) -> bool {
        let Some(entry) = self
            .collected_logs
            .iter_mut()
            .find(|entry| entry.deck == deck && entry.log == log)
        else {
            return false;
        };
        let was_unread = !entry.read;
        entry.read = true;
        was_unread
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

    /// Whether the player carries any credential at all - what decides whether
    /// the belt card exists (it stands in for the whole collected set).
    pub fn has_key_cards(&self) -> bool {
        !self.key_cards.is_empty()
    }

    /// The weapon archetype an empty grip at a shoulder draws back.
    pub fn last_stowed_weapon(&self) -> Option<i32> {
        self.last_stowed_weapon
    }

    pub fn holstered_weapon(&self) -> Option<i32> {
        self.holstered_weapon
    }

    pub fn set_holstered_weapon(&mut self, class_template_id: Option<i32>) {
        self.holstered_weapon = class_template_id;
    }

    pub fn set_last_stowed_weapon(&mut self, class_template_id: Option<i32>) {
        self.last_stowed_weapon = class_template_id;
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

#[cfg(test)]
mod log_tests {
    use super::*;

    fn quest_info_with(logs: &[(u32, u32)]) -> QuestInfo {
        let mut quest_info = QuestInfo::new();
        for (deck, log) in logs {
            quest_info.collect_log(*deck, *log);
        }
        quest_info
    }

    #[test]
    fn collected_logs_start_unread() {
        let quest_info = quest_info_with(&[(2, 20)]);

        assert_eq!(quest_info.collected_logs()[0].read, false);
    }

    /// The original sorts unread logs by acquisition time and plays the *last*
    /// one - the newest unread disc, not the oldest outstanding backlog entry.
    #[test]
    fn the_newest_unread_log_is_the_one_played_back() {
        let quest_info = quest_info_with(&[(1, 1), (2, 2), (3, 3)]);

        let unread = quest_info.last_unread_log().expect("nothing read yet");

        assert_eq!((unread.deck, unread.log), (3, 3));
    }

    #[test]
    fn reading_the_newest_falls_back_to_the_next_newest_unread() {
        let mut quest_info = quest_info_with(&[(1, 1), (2, 2), (3, 3)]);

        assert!(quest_info.mark_log_read(3, 3));
        let unread = quest_info.last_unread_log().expect("two still unread");

        assert_eq!((unread.deck, unread.log), (2, 2));
    }

    #[test]
    fn reader_replays_the_latest_collected_log_once_everything_is_read() {
        let mut quest_info = quest_info_with(&[(1, 1), (2, 2)]);

        quest_info.mark_log_read(1, 1);
        quest_info.mark_log_read(2, 2);

        assert!(quest_info.last_unread_log().is_none());
        let replay = quest_info
            .logs_for_reader()
            .next()
            .expect("latest log remains replayable");
        assert_eq!((replay.deck, replay.log), (2, 2));
    }

    #[test]
    fn reader_prefers_a_newer_unread_entry_before_replay_fallback() {
        let mut quest_info = quest_info_with(&[(1, 1), (2, 2), (3, 3)]);
        quest_info.mark_log_read(3, 3);

        let target = quest_info
            .logs_for_reader()
            .next()
            .expect("older unread log remains");

        assert_eq!((target.deck, target.log), (2, 2));
    }

    /// A log whose reader strings do not resolve here (collected on an earlier
    /// deck, missing localized transcript) must not wedge the control forever:
    /// the reader offers every collected entry in preference order so the
    /// caller can skip to the next resolvable one.
    #[test]
    fn an_unresolvable_log_does_not_wedge_the_reader() {
        let mut quest_info = quest_info_with(&[(1, 1), (2, 2), (3, 3)]);
        quest_info.mark_log_read(3, 3);

        let candidates: Vec<(u32, u32)> = quest_info
            .logs_for_reader()
            .map(|entry| (entry.deck, entry.log))
            .collect();

        // Unread newest-first, then the already-read entries newest-first as
        // the replay fallback. Every collected log is reachable.
        assert_eq!(candidates, vec![(2, 2), (1, 1), (3, 3)]);
    }

    /// Re-reading is a no-op, and an unknown log is not silently inserted.
    #[test]
    fn marking_read_reports_only_the_first_transition() {
        let mut quest_info = quest_info_with(&[(2, 20)]);

        assert!(quest_info.mark_log_read(2, 20));
        assert!(!quest_info.mark_log_read(2, 20));
        assert!(!quest_info.mark_log_read(9, 9));
        assert_eq!(quest_info.collected_logs().len(), 1);
    }

    /// Saves written before read tracking restore their logs as unread.
    #[test]
    fn logs_from_older_saves_deserialize_as_unread() {
        let restored: CollectedLog =
            serde_json::from_str(r#"{"deck":2,"log":20}"#).expect("older save shape should load");

        assert_eq!(
            restored,
            CollectedLog {
                deck: 2,
                log: 20,
                read: false
            }
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The holster slot is campaign state, not presentation: it has to come
    /// back off a save (and out of a level transition, which carries the same
    /// struct) still holding the weapon the player put there.
    #[test]
    fn the_holstered_weapon_survives_a_round_trip() {
        let mut quests = QuestInfo::new();
        assert_eq!(
            quests.holstered_weapon(),
            None,
            "nothing holstered to start"
        );
        quests.set_holstered_weapon(Some(-1893));

        let restored: QuestInfo =
            serde_json::from_str(&serde_json::to_string(&quests).unwrap()).unwrap();
        assert_eq!(restored.holstered_weapon(), Some(-1893));
    }
}
