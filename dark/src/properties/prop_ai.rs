use std::{io, time::Duration};

use num_derive::FromPrimitive;
use num_traits::FromPrimitive;
use shipyard::Component;

use crate::ss2_common::{read_bytes, read_string_with_size, read_u32};
use serde::{Deserialize, Serialize};

#[derive(Debug, Component, Clone, Deserialize, Serialize)]
pub struct PropAI(pub String);

#[derive(Debug, FromPrimitive, Clone, Serialize, Deserialize)]
pub enum AIPriority {
    None = 0,
    VeryLow = 1,
    Low = 2,
    Normal = 3,
    High = 4,
    VeryHigh = 5,
    Absolute = 6,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum AIScriptedActionType {
    Nothing,               // 0
    ScriptMessage(String), // 1
    Play(String),          // 2 - Sound or motion?
    Alert,                 // 3
    BecomeHostile,         // 4
    EnableInvestigate,     // 5
    Goto {
        waypoint_name: String,
        speed: String,
    }, // 6
    Frob(String),          // 7
    Wait(Duration),        // 8
    Mprint(String),        // 9
    MetaProperty {
        action_type: String,
        arg1: String,
        arg2: String,
    }, // 10
    AddLink {
        link_type: String, // TODO: String to item
        entity_name: String,
    }, // 11
    RemoveLink {
        link_type: String,
        entity_name: String,
    }, // 12
    Face {
        entity_name: String,
    }, // 13
    Signal {
        entity_name: String,
        signal: String,
    }, // 14
    DestScript,            // 15
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AIScriptedAction {
    pub action_type: AIScriptedActionType,
}

impl AIScriptedAction {
    pub fn read<T: io::Read + io::Seek>(reader: &mut T) -> AIScriptedAction {
        let action_type_u32 = read_u32(reader);
        let sz0 = read_string_with_size(reader, 64);
        let sz1 = read_string_with_size(reader, 64);
        let sz2 = read_string_with_size(reader, 64);
        let sz3 = read_string_with_size(reader, 64);

        let action_type = match action_type_u32 {
            0 => AIScriptedActionType::Nothing,
            1 => AIScriptedActionType::ScriptMessage(sz0),
            2 => AIScriptedActionType::Play(sz2),
            4 => AIScriptedActionType::BecomeHostile,
            6 => AIScriptedActionType::Goto {
                waypoint_name: sz0,
                speed: sz1,
            },
            7 => AIScriptedActionType::Frob(sz0),
            8 => {
                let milliseconds = u64::from_str_radix(&sz0, 10).unwrap();
                AIScriptedActionType::Wait(Duration::from_millis(milliseconds))
            }
            9 => AIScriptedActionType::Mprint(sz0),
            10 => AIScriptedActionType::MetaProperty {
                action_type: sz0,
                arg1: sz1,
                arg2: sz2,
            },
            11 => AIScriptedActionType::AddLink {
                link_type: sz0,
                entity_name: sz1,
            },
            12 => AIScriptedActionType::RemoveLink {
                link_type: sz0,
                entity_name: sz1,
            },
            13 => AIScriptedActionType::Face { entity_name: sz0 },
            14 => AIScriptedActionType::Signal {
                signal: sz0,
                entity_name: sz1,
            },
            _ => panic!(
                "Unhandled action type: {} |{}|{}|{}|{}",
                action_type_u32, &sz0, &sz1, &sz2, &sz3
            ),
        };

        AIScriptedAction { action_type }
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
        let priority_u32 = read_u32(reader);
        let priority = AIPriority::from_u32(priority_u32).unwrap();
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
        PropAISignalResponse {
            signal,
            priority,
            actions,
        }
    }
}
