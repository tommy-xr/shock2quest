//! Loader for Dark UI layout files (`*R.BIN`).
//!
//! Each pairs with a `*.PCX` screen — e.g. `LOADING.PCX` + `loadingr.BIN`,
//! `MAIN.PCX` + `MAINR.BIN`, `NEWGAME.PCX` + `NEWGAMER.BIN` — and is a header-less
//! list of LTRB `int16` rectangles in the 640x480 UI canvas, one per widget in a
//! screen-defined order. (Same binary format as the map-position files in
//! `map_position_importer`; kept separate for a clear, reusable UI-layout API.)
//!
//! Load by name and index the rects per the screen's convention, e.g.
//! `asset_cache.get(&UI_LAYOUT_IMPORTER, "loadingr.BIN")` -> `[disc, bar, text]`.

use engine::assets::{
    asset_cache::AssetCache, asset_importer::AssetImporter, asset_paths::ReadableAndSeekable,
};
use once_cell::sync::Lazy;

use crate::map::MapRect;

#[derive(Clone, Debug)]
pub struct RawUiLayout {
    pub bytes: Vec<u8>,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq, Default)]
pub struct UiLayoutOptions {}

pub(crate) fn load_ui_layout(
    _name: String,
    reader: &mut Box<dyn ReadableAndSeekable>,
    _assets: &mut AssetCache,
    _config: &UiLayoutOptions,
) -> RawUiLayout {
    let mut bytes = Vec::new();
    std::io::Read::read_to_end(reader, &mut bytes).unwrap();
    RawUiLayout { bytes }
}

pub(crate) fn process_ui_layout(
    raw: RawUiLayout,
    _assets: &mut AssetCache,
    _config: &UiLayoutOptions,
) -> Vec<MapRect> {
    parse_ui_rects(&raw.bytes)
}

/// Parse a `*R.BIN` byte buffer into LTRB rectangles (8 bytes each).
fn parse_ui_rects(bytes: &[u8]) -> Vec<MapRect> {
    bytes
        .chunks_exact(8)
        .map(|c| {
            MapRect::new(
                i16::from_le_bytes([c[0], c[1]]),
                i16::from_le_bytes([c[2], c[3]]),
                i16::from_le_bytes([c[4], c[5]]),
                i16::from_le_bytes([c[6], c[7]]),
            )
        })
        .collect()
}

pub static UI_LAYOUT_IMPORTER: Lazy<AssetImporter<RawUiLayout, Vec<MapRect>, UiLayoutOptions>> =
    Lazy::new(|| AssetImporter::define(load_ui_layout, process_ui_layout));

#[cfg(test)]
mod tests {
    use super::*;

    fn rect_bytes(ltrb: [i16; 4]) -> Vec<u8> {
        ltrb.iter().flat_map(|v| v.to_le_bytes()).collect()
    }

    #[test]
    fn parses_loadingr_bin_rects() {
        // The three rects from the real loadingr.BIN.
        let mut bytes = rect_bytes([184, 120, 456, 392]); // disc
        bytes.extend(rect_bytes([197, 394, 443, 414])); // bar
        bytes.extend(rect_bytes([118, 69, 512, 97])); // text

        let rects = parse_ui_rects(&bytes);
        assert_eq!(rects.len(), 3);
        assert_eq!((rects[0].ul_x, rects[0].ul_y), (184, 120));
        assert_eq!((rects[0].width(), rects[0].height()), (272, 272)); // disc
        assert_eq!((rects[1].width(), rects[1].height()), (246, 20)); // bar
    }

    #[test]
    fn ignores_trailing_partial_rect() {
        assert_eq!(parse_ui_rects(&[1, 0, 2, 0, 3, 0]).len(), 0);
        assert_eq!(parse_ui_rects(&[]).len(), 0);
    }
}
