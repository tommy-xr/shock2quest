use crate::ss2_chunk_file_reader::ChunkFileTableOfContents;

use std::io;
use std::io::SeekFrom;

use crate::ss2_common::read_string_with_size;

#[derive(Debug)]
pub struct SongParams {
    pub song: String,
}

impl SongParams {
    pub fn read<T: io::Read + io::Seek>(
        table_of_contents: &ChunkFileTableOfContents,
        reader: &mut T,
    ) -> SongParams {
        let chunk = table_of_contents
            .get_chunk("SONGPARAMS".to_string())
            .unwrap();
        reader.seek(SeekFrom::Start(chunk.offset)).unwrap();

        let song = read_string_with_size(reader, 32);

        SongParams { song }
    }
}
