/**
 * SaveData
 *
 * Data type for information we serialize to load/save the game
 */
use super::{EntitySaveData, HeldItemSaveData, PlayerVitals};
use crate::quest_info::QuestInfo;
use cgmath::{Quaternion, Vector3};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct SaveData {
    // The global state of the game (what level we're in, where the player is, what they're carrying, etc.)
    pub global_data: GlobalData,

    // The state of individual levels
    pub level_data: HashMap<String, EntitySaveData>,
}

impl SaveData {
    pub fn write<T: std::io::Write>(&self, writer: &mut T) {
        let save_data_json = serde_json::to_string(&self).unwrap();
        writer.write_all(save_data_json.as_bytes()).unwrap();
    }

    pub fn read<T: std::io::Read>(reader: &mut T) -> SaveData {
        let mut save_data_json = String::new();
        reader.read_to_string(&mut save_data_json).unwrap();
        let save_data: SaveData = serde_json::from_str(&save_data_json).unwrap();
        save_data
    }
}

/// Dark mission names are case-insensitive even though the save container uses
/// ordinary Rust strings as map keys. New snapshots use this representation so
/// a scene loaded as `Rick2.mis` cannot be missed later as `rick2.mis`.
pub fn canonical_mission_key(mission: &str) -> String {
    mission.trim().to_ascii_lowercase()
}

/// Resolve one mission snapshot while preserving saves written before mission
/// keys were canonicalized. Prefer the exact active-mission spelling first: a
/// broken legacy save can contain both an older lowercase visit and the newer
/// mixed-case active snapshot, and the latter is authoritative.
pub fn mission_snapshot<'a>(
    level_data: &'a HashMap<String, EntitySaveData>,
    active_mission: &str,
) -> Option<&'a EntitySaveData> {
    level_data.get(active_mission).or_else(|| {
        let canonical = canonical_mission_key(active_mission);
        level_data.get(&canonical).or_else(|| {
            level_data
                .iter()
                .filter(|(key, _)| canonical_mission_key(key) == canonical)
                .min_by(|(left, _), (right, _)| left.cmp(right))
                .map(|(_, value)| value)
        })
    })
}

/// Insert a snapshot under its canonical mission key and remove legacy casing
/// aliases, preventing stale case-insensitive duplicates in newly written saves.
pub fn insert_mission_snapshot(
    level_data: &mut HashMap<String, EntitySaveData>,
    mission: &str,
    snapshot: EntitySaveData,
) {
    let canonical = canonical_mission_key(mission);
    level_data.retain(|key, _| canonical_mission_key(key) != canonical);
    level_data.insert(canonical, snapshot);
}

/// Canonicalize a full campaign map before persistence. If a legacy map has
/// multiple case-insensitive aliases, an already-canonical entry wins for an
/// inactive mission; the caller inserts the freshly captured active snapshot
/// afterward, so that exact live state always wins for the current mission.
pub fn canonicalized_mission_snapshots(
    level_data: &HashMap<String, EntitySaveData>,
) -> HashMap<String, EntitySaveData> {
    let mut canonicalized = HashMap::new();
    let mut entries = level_data.iter().collect::<Vec<_>>();
    entries.sort_by(|(left, _), (right, _)| left.cmp(right));

    for (key, snapshot) in &entries {
        let canonical = canonical_mission_key(key);
        if *key == &canonical {
            canonicalized.insert(canonical, (*snapshot).clone());
        }
    }
    for (key, snapshot) in entries {
        canonicalized
            .entry(canonical_mission_key(key))
            .or_insert_with(|| snapshot.clone());
    }

    canonicalized
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct GlobalData {
    /// Player collider center, normalized to STANDING height: a game saved
    /// while crouched stores `center + crouch shift` (plus `is_crouched`),
    /// so load always creates the standing capsule here - never one embedded
    /// in the floor - and then re-applies the crouch.
    pub position: Vector3<f32>,
    pub rotation: Quaternion<f32>,
    pub quest_info: QuestInfo,
    pub held_items: HeldItemSaveData,
    /// Exact live player HP/PSI pools. Older saves omit this field and retain
    /// the pre-existing template/career initialization behavior.
    #[serde(default)]
    pub player_vitals: Option<PlayerVitals>,
    pub active_mission: String,
    /// Whether the player was crouched at save time. Defaults false for
    /// saves that predate crouch.
    #[serde(default)]
    pub is_crouched: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::save_load::{PlayerVitalPool, PlayerVitals};
    use cgmath::{Quaternion, vec3};

    fn global_data(player_vitals: Option<PlayerVitals>) -> GlobalData {
        GlobalData {
            position: vec3(1.0, 2.0, 3.0),
            rotation: Quaternion::new(1.0, 0.0, 0.0, 0.0),
            quest_info: QuestInfo::new(),
            held_items: HeldItemSaveData::empty(),
            player_vitals,
            active_mission: "earth.mis".to_owned(),
            is_crouched: false,
        }
    }

    fn sample_vitals() -> PlayerVitals {
        PlayerVitals {
            hit_points: PlayerVitalPool {
                current: 27,
                maximum: 35,
            },
            psi_points: PlayerVitalPool {
                current: 4,
                maximum: 60,
            },
        }
    }

    #[test]
    fn player_vitals_round_trip_current_and_maximum_values() {
        let original = global_data(Some(sample_vitals()));
        let decoded: GlobalData =
            serde_json::from_value(serde_json::to_value(&original).unwrap()).unwrap();

        assert_eq!(decoded.player_vitals, Some(sample_vitals()));
    }

    #[test]
    fn older_save_without_player_vitals_uses_existing_load_defaults() {
        let mut legacy = serde_json::to_value(global_data(Some(sample_vitals()))).unwrap();
        legacy
            .as_object_mut()
            .unwrap()
            .remove("player_vitals")
            .unwrap();

        let decoded: GlobalData = serde_json::from_value(legacy).unwrap();

        assert_eq!(decoded.player_vitals, None);
    }

    #[test]
    fn mixed_case_mission_snapshot_prefers_exact_active_legacy_key() {
        let lowercase = EntitySaveData {
            all_entities: vec![1],
            ..EntitySaveData::empty()
        };
        let active = EntitySaveData {
            all_entities: vec![2],
            ..EntitySaveData::empty()
        };
        let mut levels = HashMap::from([
            ("rick2.mis".to_owned(), lowercase),
            ("Rick2.mis".to_owned(), active),
        ]);

        assert_eq!(
            mission_snapshot(&levels, "Rick2.mis").unwrap().all_entities,
            vec![2],
            "the active exact-case snapshot must outrank a stale lowercase visit",
        );

        insert_mission_snapshot(
            &mut levels,
            "RICK2.MIS",
            EntitySaveData {
                all_entities: vec![3],
                ..EntitySaveData::empty()
            },
        );
        assert_eq!(levels.keys().collect::<Vec<_>>(), vec!["rick2.mis"]);
        assert_eq!(
            mission_snapshot(&levels, "Rick2.mis").unwrap().all_entities,
            vec![3],
        );

        let canonicalized = canonicalized_mission_snapshots(&HashMap::from([
            (
                "RICK1.MIS".to_owned(),
                EntitySaveData {
                    all_entities: vec![4],
                    ..EntitySaveData::empty()
                },
            ),
            (
                "rick1.mis".to_owned(),
                EntitySaveData {
                    all_entities: vec![5],
                    ..EntitySaveData::empty()
                },
            ),
        ]));
        assert_eq!(canonicalized.keys().collect::<Vec<_>>(), vec!["rick1.mis"]);
        assert_eq!(canonicalized["rick1.mis"].all_entities, vec![5]);
    }

    #[test]
    fn mixed_case_lookup_recovers_single_legacy_alias() {
        let levels = HashMap::from([(
            "rIcK2.MiS".to_owned(),
            EntitySaveData {
                all_entities: vec![7],
                ..EntitySaveData::empty()
            },
        )]);

        assert_eq!(
            mission_snapshot(&levels, "Rick2.mis").unwrap().all_entities,
            vec![7],
        );
        assert!(mission_snapshot(&levels, "Rick3.mis").is_none());
    }

    #[test]
    fn mission_aliases_are_deterministic_and_whitespace_is_deduplicated() {
        let snapshot = |entity| EntitySaveData {
            all_entities: vec![entity],
            ..EntitySaveData::empty()
        };
        let mut aliases = HashMap::from([
            ("Rick2.MIS ".to_owned(), snapshot(8)),
            (" RICK2.mis".to_owned(), snapshot(9)),
            ("rick1.mis".to_owned(), snapshot(10)),
        ]);

        assert_eq!(
            mission_snapshot(&aliases, "rIcK2.MiS")
                .unwrap()
                .all_entities,
            vec![9],
            "alias-only recovery must choose the lexicographically first raw key",
        );

        let canonicalized = canonicalized_mission_snapshots(&aliases);
        assert_eq!(canonicalized.len(), 2);
        assert_eq!(canonicalized["rick2.mis"].all_entities, vec![9]);
        assert_eq!(canonicalized["rick1.mis"].all_entities, vec![10]);

        insert_mission_snapshot(&mut aliases, " Rick2.mis ", snapshot(11));
        assert_eq!(
            aliases
                .keys()
                .filter(|key| canonical_mission_key(key) == "rick2.mis")
                .count(),
            1,
        );
        assert_eq!(aliases["rick2.mis"].all_entities, vec![11]);
        assert_eq!(aliases["rick1.mis"].all_entities, vec![10]);
    }
}
