use crate::ss2_chunk_file_reader::ChunkFileTableOfContents;
use crate::ss2_common::read_single;
use cgmath::{Vector3, vec3};

use std::f32;
use std::io;
use std::io::SeekFrom;

use crate::ss2_common::read_string_with_size;

#[derive(Debug)]
pub struct RenderParams {
    pub ambient_color: Vector3<f32>,
}

impl RenderParams {
    pub fn read<T: io::Read + io::Seek>(
        table_of_contents: &ChunkFileTableOfContents,
        reader: &mut T,
    ) -> RenderParams {
        let chunk = table_of_contents
            .get_chunk("RENDPARAMS".to_string())
            .unwrap();
        reader.seek(SeekFrom::Start(chunk.offset)).unwrap();

        let _palette = read_string_with_size(reader, 16);
        let ambient = read_single(reader);

        RenderParams {
            ambient_color: vec3(ambient, ambient, ambient),
        }
    }
}
