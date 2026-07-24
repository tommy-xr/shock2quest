use std::io;

use serde::{Deserialize, Serialize};
use shipyard::Component;

use crate::ss2_common::read_i32;

/// Persistent Dark object state (`P$ObjState` / `eObjState`).
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub enum ObjectState {
    #[default]
    Normal,
    Broken,
    Destroyed,
    Unresearched,
    Locked,
    Hacked,
    /// Preserve an unfamiliar retail/mod value instead of losing it on save.
    Unknown(i32),
}

impl ObjectState {
    pub fn from_raw(value: i32) -> Self {
        match value {
            0 => Self::Normal,
            1 => Self::Broken,
            2 => Self::Destroyed,
            3 => Self::Unresearched,
            4 => Self::Locked,
            5 => Self::Hacked,
            value => Self::Unknown(value),
        }
    }

    pub fn raw(self) -> i32 {
        match self {
            Self::Normal => 0,
            Self::Broken => 1,
            Self::Destroyed => 2,
            Self::Unresearched => 3,
            Self::Locked => 4,
            Self::Hacked => 5,
            Self::Unknown(value) => value,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize, Component)]
pub struct PropObjState(pub ObjectState);

impl PropObjState {
    pub fn read<T: io::Read + io::Seek>(reader: &mut T, len: u32) -> Self {
        assert_eq!(len, 4, "P$ObjState must contain one signed 32-bit state");
        Self(ObjectState::from_raw(read_i32(reader)))
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    #[test]
    fn reads_all_retail_states_and_preserves_unknown_values() {
        for raw in 0_i32..=5 {
            let state = PropObjState::read(&mut Cursor::new(raw.to_le_bytes()), 4);
            assert_eq!(state.0.raw(), raw);
        }
        let unknown = PropObjState::read(&mut Cursor::new(17_i32.to_le_bytes()), 4);
        assert_eq!(unknown, PropObjState(ObjectState::Unknown(17)));
    }

    #[test]
    fn broken_and_hacked_round_trip_through_save_json() {
        for state in [ObjectState::Broken, ObjectState::Hacked] {
            let json = serde_json::to_value(PropObjState(state)).unwrap();
            assert_eq!(
                serde_json::from_value::<PropObjState>(json).unwrap(),
                PropObjState(state)
            );
        }
    }
}
