use std::{
    collections::HashMap,
    io::{self, SeekFrom},
};

// Chunk header consists of:
// 12 bytes for name
// u32 (4 bytes) for version_high
// u32 (4 bytes) for version_low
// u32 (4 bytes) for zero
// 24 bytes total
pub const CHUNK_HEADER_SIZE: u32 = 24;

use byteorder::ReadBytesExt;
use tracing::info;

use crate::ss2_common::read_string_with_size;

// Chunk describes a chunk of data for the consumer
#[derive(Debug, Clone)]
pub struct Chunk {
    pub offset: u64, // Offset of the chunk, in bytes
    pub length: u64, // Length of chunk, in bytes
}

#[derive(Debug)]
pub struct ChunkFileTableOfContents {
    // Table of contents, storing where entries can be found
    table_of_contents: HashMap<String, Chunk>,
}

impl ChunkFileTableOfContents {
    pub fn has_chunk(&self, chunk_name: String) -> bool {
        self.table_of_contents.contains_key(&chunk_name)
    }

    pub fn get_chunk(&self, chunk_name: String) -> Option<Chunk> {
        self.table_of_contents.get(&chunk_name).cloned()
    }

    pub fn chunk_names(&self) -> impl Iterator<Item = &String> + '_ {
        self.table_of_contents.keys()
    }
}

pub fn read_table_of_contents<T: io::Read + io::Seek>(reader: &mut T) -> ChunkFileTableOfContents {
    let inv_offset = reader.read_u32::<byteorder::LittleEndian>().unwrap();
    let zero = reader.read_u32::<byteorder::LittleEndian>().unwrap();
    let one = reader.read_u32::<byteorder::LittleEndian>().unwrap();
    let mut buf: [u8; 256] = [0; 256];
    reader.read_exact(&mut buf).unwrap();
    let dead_beef = reader.read_u32::<byteorder::LittleEndian>().unwrap();

    info!(
        "reading chunk table of contents - inv_offset: {} zero: {}, one: {}, zeros: {}, dead_beef: {}, debug: {}",
        inv_offset,
        zero,
        one,
        buf.len(),
        dead_beef,
        0x0EFBEADDE_u32
    );

    reader.seek(SeekFrom::Start(inv_offset as u64)).unwrap();

    let chunk_count = reader.read_u32::<byteorder::LittleEndian>().unwrap();

    let mut dictionary = HashMap::new();
    for _ in 0..chunk_count {
        let chunk_name = read_string_with_size(reader, 12);

        let offset = reader.read_u32::<byteorder::LittleEndian>().unwrap();
        let length = reader.read_u32::<byteorder::LittleEndian>().unwrap();

        dictionary.insert(
            chunk_name,
            Chunk {
                // Always skip the header
                offset: (offset + CHUNK_HEADER_SIZE) as u64,
                length: length as u64,
            },
        );
    }

    ChunkFileTableOfContents {
        table_of_contents: dictionary,
    }
}
