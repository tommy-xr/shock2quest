/**
 * SaveData
 *
 * Data type for information we serialize to load/save the game
 */
use super::{EntitySaveData, HeldItemSaveData};
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
    pub active_mission: String,
    /// Whether the player was crouched at save time. Defaults false for
    /// saves that predate crouch.
    #[serde(default)]
    pub is_crouched: bool,
}
