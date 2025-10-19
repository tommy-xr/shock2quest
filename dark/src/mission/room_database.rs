use crate::{mission::room::Room, ss2_chunk_file_reader::ChunkFileTableOfContents};
use byteorder::ReadBytesExt;

use tracing::trace;

use std::io;
use std::io::SeekFrom;

#[derive(Debug)]
pub struct RoomDatabase {
    pub rooms: Vec<Room>,
}

impl RoomDatabase {
    // Read the TXLIST chunk to get a list of the textures used
    pub fn read<T: io::Read + io::Seek>(
        table_of_contents: &ChunkFileTableOfContents,
        reader: &mut T,
    ) -> RoomDatabase {
        let mut rooms = Vec::new();
        let txlist = table_of_contents
            .get_chunk("ROOM_DB".to_string())
            .unwrap()
            .offset;
        reader.seek(SeekFrom::Start(txlist)).unwrap();

        let _unk = reader.read_u32::<byteorder::LittleEndian>().unwrap();
        assert!(_unk != 0);

        let count = reader.read_u32::<byteorder::LittleEndian>().unwrap();
        trace!("room db - count: {}", count);

        for _ in 0..count {
            rooms.push(Room::read(reader));
        }

        RoomDatabase { rooms }
    }
}
