//! Named LGMD v6 shape deltas. The corner indices are one-based; zero means
//! no deformation. This is separate from the skinned PMNM extension.
use cgmath::{Vector2, Vector3};
use std::io::{Read, Seek, SeekFrom};

#[derive(Clone, Debug)]
pub struct ShapeDelta {
    pub position: Vector3<f32>,
    pub normal: Vector3<f32>,
    pub uv: Vector2<f32>,
}
#[derive(Clone, Debug)]
pub struct ObjectShapes {
    pub names: Vec<String>,
    pub deltas: Vec<Vec<ShapeDelta>>,
    pub corner_indices: Vec<u16>,
}

pub fn read(reader: &mut (impl Read + Seek), corner_count: usize) -> Option<ObjectShapes> {
    reader.seek(SeekFrom::Start(0)).ok()?;
    let mut bytes = Vec::new();
    reader.take(64 * 1024 * 1024).read_to_end(&mut bytes).ok()?;
    parse(&bytes, corner_count)
}

fn parse(bytes: &[u8], corner_count: usize) -> Option<ObjectShapes> {
    if bytes.get(..8)? != b"LGMD\x06\0\0\0" {
        return None;
    }
    let word = |at: usize| -> Option<usize> {
        Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?) as usize)
    };
    let indices = word(134)?;
    let data = word(138)?;
    let names_offset = word(142)?;
    let count = word(146)?;
    let vertices = word(150)?;
    if count == 0
        || count > 256
        || vertices == 0
        || indices.checked_add(corner_count.checked_mul(2)?)? != data
        || data.checked_add(count.checked_mul(vertices)?.checked_mul(32)?)? != names_offset
    {
        return None;
    }
    let name_bytes = bytes.get(names_offset..names_offset.checked_add(count.checked_mul(16)?)?)?;
    let mut names = Vec::new();
    for name in name_bytes.chunks_exact(16) {
        let end = name.iter().position(|b| *b == 0).unwrap_or(16);
        if end == 0
            || !name[..end]
                .iter()
                .all(|b| b.is_ascii_graphic() || *b == b' ')
        {
            return None;
        }
        names.push(std::str::from_utf8(&name[..end]).ok()?.to_owned());
    }
    let corner_indices = bytes
        .get(indices..data)?
        .chunks_exact(2)
        .map(|b| u16::from_le_bytes([b[0], b[1]]))
        .collect::<Vec<_>>();
    if corner_indices.iter().any(|i| *i as usize > vertices) {
        return None;
    }
    let mut deltas = Vec::new();
    for shape in bytes.get(data..names_offset)?.chunks_exact(vertices * 32) {
        let mut shape_deltas = Vec::new();
        for record in shape.chunks_exact(32) {
            let f = |i| f32::from_le_bytes(record[i..i + 4].try_into().unwrap());
            let values: [f32; 8] = std::array::from_fn(|i| f(i * 4));
            if values.iter().any(|f| !f.is_finite()) {
                return None;
            }
            shape_deltas.push(ShapeDelta {
                position: Vector3::new(-values[0], values[2], values[1]) / crate::SCALE_FACTOR,
                normal: Vector3::new(-values[3], values[5], values[4]),
                uv: Vector2::new(values[6], values[7]),
            });
        }
        deltas.push(shape_deltas);
    }
    Some(ObjectShapes {
        names,
        deltas,
        corner_indices,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    #[test]
    fn reads_named_one_based_corner_deltas() {
        let mut bytes = vec![0u8; 320];
        bytes[..8].copy_from_slice(b"LGMD\x06\0\0\0");
        for (at, value) in [(134, 154u32), (138, 160), (142, 288), (146, 2), (150, 2)] {
            bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
        }
        bytes[154..160].copy_from_slice(&[1, 0, 2, 0, 0, 0]);
        bytes[224..228].copy_from_slice(&2f32.to_le_bytes());
        bytes[288..293].copy_from_slice(b"Basis");
        bytes[304..314].copy_from_slice(b"motion_one");
        let shapes = read(&mut Cursor::new(bytes), 3).expect("LGMD v6 shapes must be retained");
        assert_eq!(shapes.names, ["Basis", "motion_one"]);
        assert_eq!(shapes.corner_indices, [1, 2, 0]);
        assert_eq!(shapes.deltas[1][0].position.x, -2.0 / crate::SCALE_FACTOR);
    }
    #[test]
    fn malformed_extension_falls_back_without_allocating_from_untrusted_counts() {
        for bytes in [vec![], b"LGMD\x04\0\0\0".to_vec(), vec![255; 154]] {
            assert!(parse(&bytes, 3).is_none());
        }
        let mut bytes = vec![0u8; 154];
        bytes[..8].copy_from_slice(b"LGMD\x06\0\0\0");
        bytes[146..150].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(parse(&bytes, usize::MAX).is_none());
    }

    #[test]
    #[ignore = "requires locally extracted owned remaster model files"]
    fn retail_plant_shape_corpus() {
        let root = std::env::var("SHOCK2_LGMD_TEST_DIR").expect("owned model directory");
        for (name, count) in [
            ("FLOWER", 4),
            ("PLANT1", 4),
            ("PLANTW", 4),
            ("PLANTT", 4),
            ("TALPLANT", 4),
            ("NERVEST", 4),
            ("NERVEU", 4),
            ("ORIFICE", 2),
        ] {
            let bytes = std::fs::read(format!("{root}/{name}.bin")).unwrap();
            let word = |at| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()) as usize;
            let corners = (word(138) - word(134)) / 2;
            let shape = parse(&bytes, corners).unwrap_or_else(|| panic!("{name}"));
            assert_eq!(shape.names.len(), count);
            assert_eq!(shape.names[0], "Basis");
            assert!(
                shape.deltas[0]
                    .iter()
                    .all(|d| d.position == Vector3::new(0.0, 0.0, 0.0))
            );
            assert!(
                shape
                    .deltas
                    .iter()
                    .skip(1)
                    .flatten()
                    .any(|d| d.position != Vector3::new(0.0, 0.0, 0.0))
            );
            let mut reader = std::io::Cursor::new(bytes);
            let header = crate::ss2_bin_header::read(&mut reader);
            let mesh = crate::ss2_bin_obj_loader::read(&mut reader, &header);
            assert_eq!(mesh.shapes.as_ref().unwrap().corner_indices.len(), corners);
            assert!(!crate::ss2_bin_obj_loader::to_vertices(&mesh).is_empty());
        }
    }
}
