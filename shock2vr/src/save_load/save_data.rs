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
}
