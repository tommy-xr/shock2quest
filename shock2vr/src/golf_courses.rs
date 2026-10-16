//! Retain authored terrain indices separately from the GPU course textures.
use crate::golf_game::{Course, HEIGHT, WIDTH};
use engine::assets::asset_cache::AssetCache;
use shipyard::Unique;
use std::{io::Read, sync::Arc};

#[derive(Unique)]
pub struct GolfCourses(pub Option<Arc<[Course; 9]>>);
impl GolfCourses {
    pub fn load(assets: &AssetCache) -> Self {
        let load = || -> Option<Arc<[Course; 9]>> {
            let mut courses = Vec::new();
            for hole in 1..=9 {
                let reader = assets.get_raw_reader(&format!("gfhol00{hole}.pcx"))?;
                let mut bytes = Vec::new();
                reader.borrow_mut().read_to_end(&mut bytes).ok()?;
                courses.push(decode(&bytes)?);
            }
            Some(Arc::new(courses.try_into().ok()?))
        };
        let courses = load();
        if courses.is_none() {
            tracing::warn!("Golf course terrain could not be loaded");
        }
        Self(courses)
    }
}
fn decode(bytes: &[u8]) -> Option<Course> {
    let mut reader = pcx::Reader::new(bytes).ok()?;
    if !reader.is_paletted() || reader.width() as usize != WIDTH || reader.height() != 276 {
        return None;
    }
    let mut pixels = vec![0; WIDTH * HEIGHT];
    for row in pixels.chunks_exact_mut(WIDTH) {
        reader.next_row_paletted(row).ok()?;
    }
    Course::from_pixels(pixels)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn indexed_course_decode_preserves_markers_and_excludes_button_strip() {
        let mut bytes = Vec::new();
        {
            let mut writer = pcx::WriterPaletted::new(&mut bytes, (140, 276), (72, 72)).unwrap();
            for y in 0..276 {
                let mut row = vec![86; 140];
                if y == 200 {
                    row[70] = 32;
                }
                if y == 20 {
                    row[30] = 183;
                }
                if y == 195 {
                    row[1] = 8;
                    row[114] = 14;
                }
                if y >= 252 {
                    row.fill(183);
                }
                writer.write_row(&row).unwrap();
            }
            writer.write_palette(&[0; 768]).unwrap();
        }
        let course = decode(&bytes).unwrap();
        assert_eq!(course.tee, [70.0, 200.0]);
        assert_eq!(course.hole, [30.0, 20.0]);
        assert_eq!(course.score_marker, [1.0, 195.0]);
        assert!(course.three_lines);
        assert_eq!(course.terrain(0.0, 252.0), 0);
        assert!(decode(&bytes[..128]).is_none());
        assert!(decode(&[]).is_none());
    }
}
