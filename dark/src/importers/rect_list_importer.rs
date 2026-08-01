//! Loader for Dark's header-less lists of LTRB `int16` rectangles.
//!
//! The format is shared by automap position files (`P001RA.BIN` and
//! `P001XA.BIN`) and UI layout files (`*R.BIN`). Each rectangle occupies eight
//! bytes in little-endian upper-left/lower-right order.

use std::io::Read;

use engine::assets::{
    asset_cache::AssetCache, asset_importer::AssetImporter, asset_paths::ReadableAndSeekable,
};
use once_cell::sync::Lazy;

use crate::map::MapRect;

#[derive(Clone, Debug)]
pub struct RawRectList {
    pub bytes: Vec<u8>,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq, Default)]
pub struct RectListOptions {}

pub(crate) fn load_rect_list(
    _name: String,
    reader: &mut Box<dyn ReadableAndSeekable>,
    _assets: &mut AssetCache,
    _config: &RectListOptions,
) -> RawRectList {
    let mut bytes = Vec::new();
    reader.read_to_end(&mut bytes).unwrap();
    RawRectList { bytes }
}

pub(crate) fn process_rect_list(
    raw: RawRectList,
    _assets: &mut AssetCache,
    _config: &RectListOptions,
) -> Vec<MapRect> {
    parse_rects(&raw.bytes)
}

fn parse_rects(bytes: &[u8]) -> Vec<MapRect> {
    bytes
        .chunks_exact(8)
        .map(|chunk| {
            MapRect::new(
                i16::from_le_bytes([chunk[0], chunk[1]]),
                i16::from_le_bytes([chunk[2], chunk[3]]),
                i16::from_le_bytes([chunk[4], chunk[5]]),
                i16::from_le_bytes([chunk[6], chunk[7]]),
            )
        })
        .collect()
}

pub static RECT_LIST_IMPORTER: Lazy<AssetImporter<RawRectList, Vec<MapRect>, RectListOptions>> =
    Lazy::new(|| AssetImporter::define(load_rect_list, process_rect_list));

// Keep the purpose-specific public names while routing both through one
// importer registration (and therefore one AssetCache key space).
pub use RECT_LIST_IMPORTER as MAP_POSITION_IMPORTER;
pub use RECT_LIST_IMPORTER as UI_LAYOUT_IMPORTER;

// Preserve the existing public data/config names for downstream callers.
pub type RawMapPositionData = RawRectList;
pub type MapPositionOptions = RectListOptions;
pub type RawUiLayout = RawRectList;
pub type UiLayoutOptions = RectListOptions;

#[cfg(test)]
mod tests {
    use super::*;

    fn rect_bytes(ltrb: [i16; 4]) -> Vec<u8> {
        ltrb.iter().flat_map(|value| value.to_le_bytes()).collect()
    }

    #[test]
    fn parses_little_endian_rect_list() {
        let mut bytes = rect_bytes([184, 120, 456, 392]);
        bytes.extend(rect_bytes([-7, 394, 443, 414]));

        let rects = parse_rects(&bytes);

        assert_eq!(
            rects,
            vec![
                MapRect::new(184, 120, 456, 392),
                MapRect::new(-7, 394, 443, 414),
            ]
        );
    }

    #[test]
    fn ignores_trailing_partial_rect() {
        let mut bytes = rect_bytes([118, 69, 512, 97]);
        bytes.extend([1, 0, 2, 0, 3, 0]);

        assert_eq!(parse_rects(&bytes), vec![MapRect::new(118, 69, 512, 97)]);
        assert!(parse_rects(&[]).is_empty());
    }
}
