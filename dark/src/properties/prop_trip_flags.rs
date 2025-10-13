use std::io;

use bitflags::bitflags;
use shipyard::Component;

use serde::{Deserialize, Serialize};

use crate::ss2_common::read_u32;
bitflags! {
    #[derive(Serialize, Deserialize)]
    pub struct TripFlags: u32 {
        const ENTER = 1 << 0; //1
        const EXIT = 1 << 1; //2
        const MONO = 1 << 2; // 4
        const ONCE = 1 << 3; //8
        const INVERT = 1 << 4; //16
        const PLAYER = 1 << 5; //32
        const ALARM = 1 << 6;
        const SHOVE = 1 << 7;
        const ZAP  = 1 << 8; // ??
        const EASTER_EGG = 1 << 9;
        const DEFAULT = Self::ENTER.bits | Self::ONCE.bits | Self::PLAYER.bits;
    }
}

#[derive(Debug, Component, Clone, Serialize, Deserialize)]
pub struct PropTripFlags {
    pub trip_flags: TripFlags,
}

impl PropTripFlags {
    pub const fn default() -> PropTripFlags {
        PropTripFlags {
            trip_flags: TripFlags::DEFAULT,
        }
    }

    pub fn read<T: io::Read + io::Seek>(reader: &mut T, _len: u32) -> PropTripFlags {
        let trip_flags = read_u32(reader);
        let t = TripFlags::from_bits(trip_flags).unwrap_or(TripFlags::DEFAULT);
        PropTripFlags { trip_flags: t }
    }
}
