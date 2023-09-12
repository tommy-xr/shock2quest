use std::io;

use shipyard::Component;

use crate::ss2_common::*;

use serde::{Deserialize, Serialize};

#[derive(Debug, Component, Clone, Deserialize, Serialize)]
pub struct PropLog {
    // Deck for the log
    pub deck: u32,
    pub email: u32,
    pub log: u32,
    pub note: u32,
    pub video: u32,
}

impl PropLog {
    pub fn read_log<R: io::Read + io::Seek>(deck: u32, reader: &mut R) -> PropLog {
        let email = read_u32(reader);
        let log = read_u32(reader);
        let note = read_u32(reader);
        let video = read_u32(reader);
        PropLog {
            deck,
            email: email.trailing_zeros() + 1,
            log: log.trailing_zeros() + 1,
            note,
            video,
        }
    }

    pub fn read_deck1<R: io::Read + io::Seek>(reader: &mut R, _len: u32) -> PropLog {
        Self::read_log(1, reader)
    }
    pub fn read_deck2<R: io::Read + io::Seek>(reader: &mut R, _len: u32) -> PropLog {
        Self::read_log(2, reader)
    }

    pub fn read_deck3<R: io::Read + io::Seek>(reader: &mut R, _len: u32) -> PropLog {
        Self::read_log(3, reader)
    }

    pub fn read_deck4<R: io::Read + io::Seek>(reader: &mut R, _len: u32) -> PropLog {
        Self::read_log(4, reader)
    }

    pub fn read_deck5<R: io::Read + io::Seek>(reader: &mut R, _len: u32) -> PropLog {
        Self::read_log(5, reader)
    }

    pub fn read_deck6<R: io::Read + io::Seek>(reader: &mut R, _len: u32) -> PropLog {
        Self::read_log(6, reader)
    }

    pub fn read_deck7<R: io::Read + io::Seek>(reader: &mut R, _len: u32) -> PropLog {
        Self::read_log(7, reader)
    }

    pub fn read_deck8<R: io::Read + io::Seek>(reader: &mut R, _len: u32) -> PropLog {
        Self::read_log(8, reader)
    }

    pub fn read_deck9<R: io::Read + io::Seek>(reader: &mut R, _len: u32) -> PropLog {
        Self::read_log(9, reader)
    }
}
