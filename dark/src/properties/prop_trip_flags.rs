use std::io;

use bitflags::bitflags;
use shipyard::Component;

use serde::{Deserialize, Serialize};

use crate::ss2_common::read_u32;
bitflags! {
    #[derive(Serialize, Deserialize)]
    pub struct TripFlags: u32 {
        const Enter = 1 << 0; //1
        const Exit = 1 << 1; //2
        const Mono = 1 << 2; // 4
        const Once = 1 << 3; //8
        const Invert = 1 << 4; //16
        const Player = 1 << 5; //32
        const Alarm = 1 << 6;
        const Shove = 1 << 7;
        const Zap  = 1 << 8; // ??
        const EasterEgg = 1 << 9;
        const Default = Self::Enter.bits | Self::Once.bits | Self::Player.bits;
    }
}

#[derive(Debug, Component, Clone, Serialize, Deserialize)]
pub struct PropTripFlags {
    pub trip_flags: TripFlags,
}

impl PropTripFlags {
    pub const fn default() -> PropTripFlags {
        PropTripFlags {
            trip_flags: TripFlags::Default,
        }
    }

    pub fn read<T: io::Read + io::Seek>(reader: &mut T, _len: u32) -> PropTripFlags {
        let trip_flags = read_u32(reader);
        let t = TripFlags::from_bits(trip_flags).unwrap_or(TripFlags::Default);
        PropTripFlags { trip_flags: t }
    }
}
