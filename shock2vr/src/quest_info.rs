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

/// A PDA list tab. Retail's PDA also has a Help tab.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PdaKind {
    Email,
    #[default]
    Log,
    /// Per-deck objectives (quest bits `Note_<deck>_<n>`); a read-only checklist.
    Note,
}

/// Objective slots per deck: `Note_<deck>_1` ..= `Note_<deck>_32`.
const NOTES_PER_DECK: u32 = 32;

/// A note bit's checklist state: 1 an open objective, 2 a completed one; any
/// other value keeps it off the list.
fn note_checked(value: QuestBitValue) -> Option<bool> {
    if value == QuestBitValue::INCOMPLETE {
        Some(false)
    } else if value == QuestBitValue::COMPLETE {
        Some(true)
    } else {
        None
    }
}

/// One PDA entry: an email or audio log, keyed like the string tables
/// (`level<deck>.str` `EmailName<index>` / `LogName<index>`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PdaEntry {
    pub kind: PdaKind,
    pub deck: u32,
    pub index: u32,
}

#[derive(Deserialize, Serialize, Unique, Clone, Debug)]
pub struct QuestInfo {
    /// Player\Mini Games unlock bitfield; global across saves and deck changes.
    pub mini_games: u32,
    #[serde(default)]
    pub(crate) overworld: crate::overworld_game::Campaign,
    pub mini_game_scores: HashMap<u8, crate::mini_game_scores::MiniGameScores>,
    pub(crate) breath: crate::swimming::Breath,
    /// Fixed at campaign creation. A typed field keeps invalid quest-bit values
    /// and generic quest mutations from changing the campaign rules.
    difficulty: dark::gamesys::Difficulty,
    /// Player-wide suppression window, retained across saves and deck changes.
    security_hack_seconds_remaining: f32,
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
    /// Help topics unlocked at info terminals, in unlock order. Topic `i` is
    /// `P$HelpText` bit `i`: `infocomp.str` `HelpName{i+1}` / `HelpText{i+1}`.
    /// `#[serde(default)]` keeps older saves loadable.
    #[serde(default)]
    help_topics: Vec<u32>,
    /// Automap locations the player has explored, per mission (lowercase level
    /// file name -> ordered location set). The original's mission-scoped
    /// `EXPLORED[64]` file-var; keyed per mission here so deck re-entry keeps
    /// its reveal state. `#[serde(default)]` keeps older saves loadable.
    #[serde(default)]
    explored_maps: HashMap<String, BTreeSet<i32>>,
    /// Campaign-wide research progress, keyed by stable gamesys archetype.
    #[serde(default)]
    research: ResearchState,
    #[serde(default)]
    pub(crate) horde_battle: crate::horde_stats::HordeBattleStats,
    /// The mission (lowercase) whose PlayerFactory start marker created the
    /// player. Dark clones that marker onto the player only when there is no
    /// player yet - a new game or a direct mission launch; a level transition
    /// carries the existing player across, so the destination's marker never
    /// applies.
    #[serde(default)]
    player_factory_mission: Option<String>,
}

impl QuestInfo {
    pub fn new() -> QuestInfo {
        Self::with_difficulty(dark::gamesys::Difficulty::Normal)
    }

    pub fn with_difficulty(difficulty: dark::gamesys::Difficulty) -> Self {
        QuestInfo {
            mini_games: 0,
            overworld: Default::default(),
            mini_game_scores: HashMap::new(),
            breath: Default::default(),
            difficulty,
            security_hack_seconds_remaining: 0.0,
            quest_bit_values: HashMap::new(),
            played_emails: HashSet::new(),
            key_cards: Vec::new(),
            player_stats: PlayerStats::new(),
            collected_logs: Vec::new(),
            help_topics: Vec::new(),
            explored_maps: HashMap::new(),
            research: ResearchState::default(),
            horde_battle: Default::default(),
            player_factory_mission: None,
        }
    }

    /// Whether `mission`'s PlayerFactory marker seeds the player on this load.
    /// The first mission loaded into a run claims it (a new game or a direct
    /// launch), and reloading that mission - a save or a reset - keeps it. Any
    /// other mission was reached by a transition, so the player arrives with
    /// their own skills and its marker is ignored.
    pub fn claim_player_factory(&mut self, mission: &str) -> bool {
        let mission = crate::save_load::canonical_mission_key(mission);
        *self
            .player_factory_mission
            .get_or_insert_with(|| mission.clone())
            == mission
    }

    /// Scene constructors seed their own test stats; retain those while selecting
    /// the difficulty of the newly-created debug scene. Not a live setting.
    pub(crate) fn for_debug_difficulty(mut self, difficulty: dark::gamesys::Difficulty) -> Self {
        self.difficulty = difficulty;
        self
    }

    pub fn difficulty(&self) -> dark::gamesys::Difficulty {
        self.difficulty
    }

    pub fn activate_security_hack(&mut self, seconds: f32) {
        self.security_hack_seconds_remaining = if seconds.is_finite() {
            seconds.max(0.0)
        } else {
            0.0
        };
    }

    pub fn advance_security_hack(&mut self, seconds: f32) {
        if seconds.is_finite() {
            self.security_hack_seconds_remaining =
                (self.security_hack_seconds_remaining - seconds.max(0.0)).max(0.0);
        }
    }

    pub fn security_hack_active(&self) -> bool {
        self.security_hack_seconds_remaining > 0.0
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

    /// Unlocked help topics, oldest first.
    pub fn help_topics(&self) -> &[u32] {
        &self.help_topics
    }

    /// Unlock every topic set in an info terminal's `P$HelpText` mask, keeping
    /// first-unlock order. Returns the topic the terminal shows - its lowest
    /// set bit, whether or not it was already unlocked - or `None` for `0`.
    pub fn unlock_help(&mut self, bits: u32) -> Option<u32> {
        for topic in (0..u32::BITS).filter(|topic| bits & (1 << topic) != 0) {
            if !self.help_topics.contains(&topic) {
                self.help_topics.push(topic);
            }
        }
        (bits != 0).then(|| bits.trailing_zeros())
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

    /// Received emails as `(deck, email)`, parsed back from their `EMddnn`
    /// sound names.
    fn collected_emails(&self) -> impl Iterator<Item = (u32, u32)> + '_ {
        self.played_emails.iter().filter_map(|name| {
            let digits = name.strip_prefix("EM")?;
            let deck = digits.get(..2)?.parse().ok()?;
            let email = digits.get(2..)?.parse().ok()?;
            Some((deck, email))
        })
    }

    /// One deck's PDA list, oldest first like retail. Logs keep pickup order;
    /// emails keep no receipt time, so they list in index (story) order.
    pub fn pda_entries(&self, kind: PdaKind, deck: u32) -> Vec<u32> {
        match kind {
            PdaKind::Log => self
                .collected_logs
                .iter()
                .filter(|entry| entry.deck == deck)
                .map(|entry| entry.log)
                .collect(),
            PdaKind::Email => {
                let mut emails: Vec<u32> = self
                    .collected_emails()
                    .filter(|(d, _)| *d == deck)
                    .map(|(_, email)| email)
                    .collect();
                emails.sort_unstable();
                emails
            }
            // Index order; the list orders notes by `NoteOrder` from notes.str.
            PdaKind::Note => self.pda_notes(deck).into_iter().map(|(n, _)| n).collect(),
        }
    }

    /// One deck's visible notes as `(index, checked)`, in index order.
    pub fn pda_notes(&self, deck: u32) -> Vec<(u32, bool)> {
        (1..=NOTES_PER_DECK)
            .filter_map(|n| {
                note_checked(self.read_quest_bit_value(&format!("note_{deck}_{n}")))
                    .map(|checked| (n, checked))
            })
            .collect()
    }

    /// Decks with a visible note, parsed back from the `note_<deck>_<n>` bits.
    fn note_decks(&self) -> impl Iterator<Item = u32> + '_ {
        self.quest_bit_values.iter().filter_map(|(name, value)| {
            note_checked(*value)?;
            let (deck, n) = name.strip_prefix("note_")?.split_once('_')?;
            let (deck, n): (u32, u32) = (deck.parse().ok()?, n.parse().ok()?);
            // Only the exact names `pda_notes` reads: `note_05_3` is not deck 5.
            ((1..=NOTES_PER_DECK).contains(&n) && *name == format!("note_{deck}_{n}"))
                .then_some(deck)
        })
    }

    /// Decks with any email, log or note, ascending: the PDA's deck arrows
    /// cycle these. Retail skipped note-only decks; listing them keeps every
    /// objective reachable.
    pub fn pda_decks(&self) -> Vec<u32> {
        let decks: BTreeSet<u32> = self
            .collected_logs
            .iter()
            .map(|entry| entry.deck)
            .chain(self.collected_emails().map(|(deck, _)| deck))
            .chain(self.note_decks())
            .collect();
        decks.into_iter().collect()
    }

    /// Where the PDA opens: the newest log's deck, else the latest email deck,
    /// else the latest deck with notes.
    pub fn pda_default_view(&self) -> Option<(PdaKind, u32)> {
        self.collected_logs
            .last()
            .map(|entry| (PdaKind::Log, entry.deck))
            .or_else(|| {
                self.collected_emails()
                    .map(|(deck, _)| deck)
                    .max()
                    .map(|deck| (PdaKind::Email, deck))
            })
            .or_else(|| self.note_decks().max().map(|deck| (PdaKind::Note, deck)))
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

    /// Collected access credentials; pickups no longer exist in the inventory.
    pub fn key_cards(&self) -> &[KeyCard] {
        &self.key_cards
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
        if quest_name.eq_ignore_ascii_case("difficulty") {
            return;
        }
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
    fn horde_battle_totals_round_trip_with_campaign_state() {
        let mut quests = QuestInfo::new();
        quests.horde_battle.record(true, 40, 30);
        quests.horde_battle.record(false, 12, 0);
        let restored: QuestInfo =
            serde_json::from_str(&serde_json::to_string(&quests).unwrap()).unwrap();
        assert_eq!(restored.horde_battle, quests.horde_battle);
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

    /// Every collected log and played email stays listed per deck, not just the
    /// newest one the reader control reaches.
    #[test]
    fn pda_lists_every_collected_entry_per_deck_oldest_first() {
        let mut quest_info = quest_info_with(&[(2, 20), (3, 1), (2, 4)]);
        for (deck, email) in [(2, 12), (2, 3), (4, 1)] {
            quest_info.mark_email_as_played(&crate::util::get_email_sound_file(deck, email));
        }

        assert_eq!(quest_info.pda_entries(PdaKind::Log, 2), vec![20, 4]);
        assert_eq!(quest_info.pda_entries(PdaKind::Log, 3), vec![1]);
        assert_eq!(quest_info.pda_entries(PdaKind::Email, 2), vec![3, 12]);
        assert_eq!(quest_info.pda_decks(), vec![2, 3, 4]);
        assert_eq!(quest_info.pda_default_view(), Some((PdaKind::Log, 2)));

        let mut emails_only = QuestInfo::new();
        emails_only.mark_email_as_played(&crate::util::get_email_sound_file(1, 2));
        assert_eq!(emails_only.pda_default_view(), Some((PdaKind::Email, 1)));
        assert_eq!(QuestInfo::new().pda_default_view(), None);
    }

    /// `Note_<deck>_<n>` = 1 lists an open objective, 2 a checked one; unset,
    /// 0, other values and slots past 32 stay hidden. A deck with only notes
    /// still gets a deck-arrow stop.
    #[test]
    fn note_bits_list_open_and_checked_objectives_per_deck() {
        let mut quest_info = QuestInfo::new();
        quest_info.set_quest_bit_value("Note_5_3", QuestBitValue::COMPLETE);
        quest_info.set_quest_bit_value("note_5_1", QuestBitValue::INCOMPLETE);
        quest_info.set_quest_bit_value("Note_5_2", QuestBitValue::UNKNOWN);
        quest_info.set_quest_bit_value("Note_5_33", QuestBitValue::INCOMPLETE);
        quest_info.set_quest_bit_value("Note_6_1", QuestBitValue::UNKNOWN);
        quest_info.set_quest_bit_value("Note_7_1", QuestBitValue::all());
        quest_info.set_quest_bit_value("Note_08_1", QuestBitValue::INCOMPLETE);

        assert_eq!(quest_info.pda_notes(5), vec![(1, false), (3, true)]);
        assert_eq!(quest_info.pda_notes(6), vec![]);
        assert_eq!(quest_info.pda_decks(), vec![5]);
        assert_eq!(quest_info.pda_default_view(), Some((PdaKind::Note, 5)));

        quest_info.collect_log(2, 1);
        assert_eq!(quest_info.pda_decks(), vec![2, 5]);
        assert_eq!(quest_info.pda_default_view(), Some((PdaKind::Log, 2)));
    }

    #[test]
    fn help_unlocks_keep_first_unlock_order_and_show_the_lowest_bit() {
        let mut quests = QuestInfo::new();
        assert_eq!(quests.unlock_help(1 << 7), Some(7));
        assert_eq!(quests.unlock_help((1 << 2) | (1 << 9)), Some(2));
        // Re-frobbing a terminal shows its topic again without reordering.
        assert_eq!(quests.unlock_help(1 << 7), Some(7));
        assert_eq!(quests.help_topics(), &[7, 2, 9]);
    }

    #[test]
    fn an_empty_help_mask_unlocks_nothing() {
        let mut quests = QuestInfo::new();
        assert_eq!(quests.unlock_help(0), None);
        assert!(quests.help_topics().is_empty());
    }

    #[test]
    fn saves_without_help_topics_still_load() {
        let mut json = serde_json::to_value(QuestInfo::new()).unwrap();
        json.as_object_mut().unwrap().remove("help_topics");
        let restored: QuestInfo = serde_json::from_value(json).expect("older save shape loads");
        assert!(restored.help_topics().is_empty());
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
mod difficulty_tests {
    use super::*;
    #[test]
    fn difficulty_is_sticky_and_round_trips_with_campaign() {
        for difficulty in dark::gamesys::Difficulty::ALL {
            let mut q = QuestInfo::with_difficulty(difficulty);
            q.set_quest_bit_value("Difficulty", QuestBitValue::COMPLETE);
            q.clear_quest_bit_value("difficulty");
            let saved = serde_json::to_string(&q).unwrap();
            let restored: QuestInfo = serde_json::from_str(&saved).unwrap();
            assert_eq!(restored.difficulty(), difficulty);
            assert_eq!(restored.clone().difficulty(), difficulty);
        }
        let mut value = serde_json::to_value(QuestInfo::new()).unwrap();
        value["difficulty"] = serde_json::json!("multiplayer");
        assert!(serde_json::from_value::<QuestInfo>(value).is_err());
    }
}

#[cfg(test)]
mod security_hack_tests {
    use super::*;

    #[test]
    fn saved_suppression_resumes_its_remaining_window() {
        let mut quests = QuestInfo::new();
        quests.activate_security_hack(30.0);
        quests.advance_security_hack(12.0);
        let mut restored: QuestInfo =
            serde_json::from_str(&serde_json::to_string(&quests).unwrap()).unwrap();
        restored.advance_security_hack(17.0);
        assert!(restored.security_hack_active());
        restored.advance_security_hack(1.0);
        assert!(!restored.security_hack_active());
    }

    #[test]
    fn another_hack_replaces_the_window_and_an_alarm_can_cancel_it() {
        let mut quests = QuestInfo::new();
        quests.activate_security_hack(100.0);
        quests.activate_security_hack(10.0);
        quests.advance_security_hack(10.0);
        assert!(!quests.security_hack_active());
        quests.activate_security_hack(30.0);
        quests.activate_security_hack(0.0);
        assert!(!quests.security_hack_active());
    }

    /// A new game claims earth's marker; earth saves keep it, and the decks a
    /// transition reaches never apply theirs (hydro1's marker authors all 6s).
    #[test]
    fn only_the_mission_that_created_the_player_applies_its_marker() {
        let mut quests = QuestInfo::new();
        assert!(quests.claim_player_factory("earth.mis"));
        assert!(quests.claim_player_factory("EARTH.MIS"));
        assert!(!quests.claim_player_factory("hydro1.mis"));

        let reloaded: QuestInfo =
            serde_json::from_value(serde_json::to_value(&quests).unwrap()).unwrap();
        let mut reloaded = reloaded;
        assert!(reloaded.claim_player_factory("earth.mis"));
        assert!(!reloaded.claim_player_factory("station.mis"));
    }
}
