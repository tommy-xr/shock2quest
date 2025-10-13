use std::io;

use crate::{
    ss2_chunk_file_reader::ChunkFileTableOfContents,
    ss2_common::{read_bytes, read_u32},
    NameMap, TagDatabase,
};

#[derive(Debug, Clone)]
pub struct Voice {
    pub tag_maps: Vec<TagDatabase>,
}

impl Voice {
    pub fn read<T: io::Read + io::Seek>(reader: &mut T, num_concepts: usize) -> Voice {
        let mut tag_maps = Vec::new();
        for _ in 0..num_concepts {
            let tag_database = TagDatabase::read(reader);
            tag_maps.push(tag_database)
        }

        Voice { tag_maps }
    }
}

#[derive(Clone, Debug)]
pub struct SpeechDB {
    pub concept_map: NameMap,
    pub tag_map: NameMap,
    pub value_map: NameMap,
    pub voices: Vec<Voice>,
}

impl SpeechDB {
    pub fn read<T: io::Read + io::Seek>(
        table_of_contents: &ChunkFileTableOfContents,
        reader: &mut T,
    ) -> SpeechDB {
        // Read SchSamp chunk
        let schema_chunk = table_of_contents.get_chunk("Speech_DB".to_owned()).unwrap();
        let _end = schema_chunk.offset + schema_chunk.length;
        let _ = reader.seek(io::SeekFrom::Start(schema_chunk.offset));

        let concept_map = NameMap::read(reader);
        let tag_map = NameMap::read(reader);
        let value_map = NameMap::read(reader);

        // Read priority
        let priority_size = read_u32(reader) * 4;
        let _priority = read_bytes(reader, priority_size as usize);

        let flags_size = read_u32(reader) * 4;
        let _flags = read_bytes(reader, flags_size as usize);

        let num_voices = read_u32(reader);

        let num_concepts = concept_map.count();

        let mut voices = Vec::new();
        for _idx in 0..num_voices {
            let voice = Voice::read(reader, num_concepts);
            voices.push(voice);
        }

        SpeechDB {
            concept_map,
            tag_map,
            value_map,
            voices,
        }
    }
}
