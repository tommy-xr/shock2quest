use std::io;

use bitflags::bitflags;
use num_traits::ToPrimitive;
use shipyard::Component;

use crate::ss2_common::{read_bytes, read_i32, read_string_with_size, read_u32};
use serde::{Deserialize, Serialize};

#[derive(Debug, Component, Clone, Deserialize, Serialize)]
pub struct PropAI(pub String);

// TODO
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AIPriority {
    Default,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AIScriptedActionType {
    Nothing,
    Script,
    Play, // Sound or motion?
    Alert,
    BecomeHostile,
    EnableInvestigate,
    Goto,
    Frob,
    Wait,
    Mprint,
    MetaProperty,
    AddLink,
    RemoveLink,
    Face,
    Signal,
    DestScript,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AIScriptedAction {
    pub action_type: AIScriptedActionType,
    pub action_data: [String; 4],
}

impl AIScriptedAction {
    pub fn read<T: io::Read + io::Seek>(reader: &mut T) -> AIScriptedAction {
        let action_type = read_u32(reader);
        let sz0 = read_string_with_size(reader, 64);
        let sz1 = read_string_with_size(reader, 64);
        let sz2 = read_string_with_size(reader, 64);
        let sz3 = read_string_with_size(reader, 64);
        let action_data = [sz0, sz1, sz2, sz3];

        AIScriptedAction {
            action_type: AIScriptedActionType::Nothing,
            action_data,
        }
    }
}

#[derive(Component, Debug, Clone, Serialize, Deserialize)]
pub struct PropAISignalResponse {
    pub signal: String,
    pub priority: AIPriority,
    pub actions: Vec<AIScriptedAction>,
}

impl PropAISignalResponse {
    pub fn read<T: io::Read + io::Seek>(reader: &mut T, len: u32) -> PropAISignalResponse {
        let signal = read_string_with_size(reader, 32);
        let priority = read_u32(reader);
        let _unk = read_bytes(reader, 16);

        // There can be a variable number of actions here, but we don't know how many (up to 16)
        // Use the length field to figure it out...
        let header_size = 52;
        let actions_size = len - header_size;
        let action_size = 4 + (4 * 64);

        let num_actions = actions_size / action_size;

        let mut actions = Vec::new();
        for _i in 0..num_actions {
            actions.push(AIScriptedAction::read(reader));
        }
        println!(
            "debug!!: got to read PropAISignalResponse: {:?} priority: {}",
            signal, priority
        );
        PropAISignalResponse {
            signal,
            priority: AIPriority::Default,
            actions,
        }
    }
}
