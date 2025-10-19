use std::io;

use tracing::trace;

use crate::{
    ss2_chunk_file_reader::ChunkFileTableOfContents,
    ss2_common::{read_bytes, read_u32},
    TagDatabase,
};

pub struct EnvMap {}

impl EnvMap {
    pub fn read<T: io::Read + io::Seek>(
        table_of_contents: &ChunkFileTableOfContents,
        reader: &mut T,
    ) -> TagDatabase {
        let test = table_of_contents.get_chunk("ENV_SOUND".to_owned()).unwrap();

        let _ = reader.seek(io::SeekFrom::Start(test.offset));
        let local_required_size = read_u32(reader);

        trace!("local required size: {}", &local_required_size);
        let _local_required = read_bytes(reader, local_required_size as usize);

        // for _ in 0..count {
        //     let c0 = read_char(reader);
        //     let c1 = read_char(reader);
        //     trace!("-- read tag: {c0} {c1}");
        // }

        // trace!("tag_database: {:#?}", tag_database);
        TagDatabase::read(reader)
    }
}
