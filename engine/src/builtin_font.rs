//! A fallback bitmap font compiled into the binary.
//!
//! Every other font in the game is loaded from the retail data (`METAFONT.FON`
//! and friends, out of `intrface.crf` or the 25AE KPFs). That is fine until the
//! thing we need to draw is "your game data is missing" - the screen that has to
//! work when the asset cache has nothing to give. So this font ships as a table
//! of glyph bitmaps compiled into the engine rather than an asset loaded through
//! the cache, and it is the only font guaranteed to be available.
//!
//! # Provenance
//!
//! The glyph bitmaps are the printable-ASCII subset (U+0020..U+007E) of
//! **unscii-8** by Viznut (<http://viznut.fi/unscii/>), which is in the
//! **public domain**. Only the separate `unscii-16-full` variant carries a GPL
//! obligation (it embeds GNU Unifont); the 8x8 variant used here does not.
//! U+007F is included as a blank cell so the table is a full 16x6 grid.
//!
//! The table is the `U+0020`..`U+007F` rows of upstream `unscii-8.hex`
//! verbatim, one 8-byte glyph per row. Each line of that file is
//! `<codepoint>:<16 hex digits>`, so any entry is independently reproducible:
//!
//! ```text
//! $ curl -s http://viznut.fi/unscii/unscii-8.hex | grep '^00041:'
//! 00041:183C66667E666600          # == GLYPHS[0x41 - 0x20], the 'A' row below
//! ```
//!
//! `the_table_matches_upstream_unscii_8` pins specific rows, so an edit to the
//! table fails a test rather than silently drifting from that attribution. The
//! full 95-row table was diffed against upstream when it was added.
//!
//! # Layout
//!
//! Cells are packed row-major into a 16x6 grid, cell index `c - 0x20`. Each cell
//! is the 8x8 glyph plus a 1px transparent border, because
//! [`texture::init_from_memory`](crate::texture::init_from_memory) uploads with
//! `GL_LINEAR` filtering and this font is drawn well above 1:1 - without the
//! border, magnified sampling at a cell edge bleeds in the neighbouring glyph's
//! column. The border is padding only: UVs address the inner 8x8, so the glyph
//! metrics are unchanged by it.
//!
//! The font is monospace: one advance is `GLYPH_PIXELS`, the *unpadded* glyph
//! box (not `CELL_PIXELS`, which includes the border). Unscii left-aligns
//! glyphs and most leave their rightmost column blank, which is what supplies
//! letter spacing - but a few (`*`, `/`, `X`, `Y`, `\`, `_`) do reach column 7,
//! so adjacent glyphs can touch. That is upstream's design, not a packing bug.

use std::rc::Rc;

use crate::font::{Font, FontCharacterInfo};
use crate::texture::{self, Texture, TextureTrait};
use crate::texture_format::{PixelFormat, RawTextureData};

/// Side length of one glyph, in pixels.
const GLYPH_PIXELS: u32 = 8;
/// Transparent border around each glyph, in pixels. See the module docs.
const CELL_PADDING: u32 = 1;
/// Side length of one atlas cell: the glyph plus its border on both sides.
const CELL_PIXELS: u32 = GLYPH_PIXELS + CELL_PADDING * 2;
/// Glyph cells per atlas row.
const ATLAS_COLUMNS: u32 = 16;
/// Rows of glyph cells in the atlas.
const ATLAS_ROWS: u32 = 6;
/// Codepoint of the first cell in the atlas.
const FIRST_CHAR: u32 = 0x20;

/// Atlas width in pixels.
const ATLAS_WIDTH: u32 = ATLAS_COLUMNS * CELL_PIXELS;
/// Atlas height in pixels.
const ATLAS_HEIGHT: u32 = ATLAS_ROWS * CELL_PIXELS;

/// One byte per pixel row, most significant bit leftmost.
#[rustfmt::skip]
const GLYPHS: [[u8; GLYPH_PIXELS as usize]; (ATLAS_COLUMNS * ATLAS_ROWS) as usize] = [
    [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00], // U+0020 ' '
    [0x18, 0x18, 0x18, 0x18, 0x18, 0x00, 0x18, 0x00], // U+0021 '!'
    [0x66, 0x66, 0x66, 0x00, 0x00, 0x00, 0x00, 0x00], // U+0022 '"'
    [0x6C, 0x6C, 0xFE, 0x6C, 0xFE, 0x6C, 0x6C, 0x00], // U+0023 '#'
    [0x18, 0x3E, 0x60, 0x3C, 0x06, 0x7C, 0x18, 0x00], // U+0024 '$'
    [0x00, 0xC6, 0xCC, 0x18, 0x30, 0x66, 0xC6, 0x00], // U+0025 '%'
    [0x38, 0x6C, 0x38, 0x76, 0xDC, 0xCC, 0x76, 0x00], // U+0026 '&'
    [0x18, 0x18, 0x30, 0x00, 0x00, 0x00, 0x00, 0x00], // U+0027 "'"
    [0x0C, 0x18, 0x30, 0x30, 0x30, 0x18, 0x0C, 0x00], // U+0028 '('
    [0x30, 0x18, 0x0C, 0x0C, 0x0C, 0x18, 0x30, 0x00], // U+0029 ')'
    [0x00, 0x66, 0x3C, 0xFF, 0x3C, 0x66, 0x00, 0x00], // U+002A '*'
    [0x00, 0x18, 0x18, 0x7E, 0x18, 0x18, 0x00, 0x00], // U+002B '+'
    [0x00, 0x00, 0x00, 0x00, 0x00, 0x18, 0x18, 0x30], // U+002C ','
    [0x00, 0x00, 0x00, 0x7E, 0x00, 0x00, 0x00, 0x00], // U+002D '-'
    [0x00, 0x00, 0x00, 0x00, 0x00, 0x18, 0x18, 0x00], // U+002E '.'
    [0x03, 0x06, 0x0C, 0x18, 0x30, 0x60, 0xC0, 0x00], // U+002F '/'
    [0x3C, 0x66, 0x6E, 0x76, 0x66, 0x66, 0x3C, 0x00], // U+0030 '0'
    [0x18, 0x38, 0x18, 0x18, 0x18, 0x18, 0x7E, 0x00], // U+0031 '1'
    [0x3C, 0x66, 0x0C, 0x18, 0x30, 0x60, 0x7E, 0x00], // U+0032 '2'
    [0x3C, 0x66, 0x06, 0x1C, 0x06, 0x66, 0x3C, 0x00], // U+0033 '3'
    [0x1C, 0x3C, 0x6C, 0xCC, 0xFE, 0x0C, 0x0C, 0x00], // U+0034 '4'
    [0x7E, 0x60, 0x7C, 0x06, 0x06, 0x66, 0x3C, 0x00], // U+0035 '5'
    [0x1C, 0x30, 0x60, 0x7C, 0x66, 0x66, 0x3C, 0x00], // U+0036 '6'
    [0x7E, 0x06, 0x06, 0x0C, 0x18, 0x18, 0x18, 0x00], // U+0037 '7'
    [0x3C, 0x66, 0x66, 0x3C, 0x66, 0x66, 0x3C, 0x00], // U+0038 '8'
    [0x3C, 0x66, 0x66, 0x3E, 0x06, 0x0C, 0x38, 0x00], // U+0039 '9'
    [0x00, 0x18, 0x18, 0x00, 0x00, 0x18, 0x18, 0x00], // U+003A ':'
    [0x00, 0x18, 0x18, 0x00, 0x00, 0x18, 0x18, 0x30], // U+003B ';'
    [0x0C, 0x18, 0x30, 0x60, 0x30, 0x18, 0x0C, 0x00], // U+003C '<'
    [0x00, 0x00, 0x7E, 0x00, 0x7E, 0x00, 0x00, 0x00], // U+003D '='
    [0x60, 0x30, 0x18, 0x0C, 0x18, 0x30, 0x60, 0x00], // U+003E '>'
    [0x3C, 0x66, 0x06, 0x0C, 0x18, 0x00, 0x18, 0x00], // U+003F '?'
    [0x7C, 0xC6, 0xDE, 0xDE, 0xDE, 0xC0, 0x7C, 0x00], // U+0040 '@'
    [0x18, 0x3C, 0x66, 0x66, 0x7E, 0x66, 0x66, 0x00], // U+0041 'A'
    [0x7C, 0x66, 0x66, 0x7C, 0x66, 0x66, 0x7C, 0x00], // U+0042 'B'
    [0x3C, 0x66, 0x60, 0x60, 0x60, 0x66, 0x3C, 0x00], // U+0043 'C'
    [0x78, 0x6C, 0x66, 0x66, 0x66, 0x6C, 0x78, 0x00], // U+0044 'D'
    [0x7E, 0x60, 0x60, 0x7C, 0x60, 0x60, 0x7E, 0x00], // U+0045 'E'
    [0x7E, 0x60, 0x60, 0x7C, 0x60, 0x60, 0x60, 0x00], // U+0046 'F'
    [0x3C, 0x66, 0x60, 0x6E, 0x66, 0x66, 0x3E, 0x00], // U+0047 'G'
    [0x66, 0x66, 0x66, 0x7E, 0x66, 0x66, 0x66, 0x00], // U+0048 'H'
    [0x7E, 0x18, 0x18, 0x18, 0x18, 0x18, 0x7E, 0x00], // U+0049 'I'
    [0x06, 0x06, 0x06, 0x06, 0x06, 0x66, 0x3C, 0x00], // U+004A 'J'
    [0xC6, 0xCC, 0xD8, 0xF0, 0xD8, 0xCC, 0xC6, 0x00], // U+004B 'K'
    [0x60, 0x60, 0x60, 0x60, 0x60, 0x60, 0x7E, 0x00], // U+004C 'L'
    [0xC6, 0xEE, 0xFE, 0xD6, 0xC6, 0xC6, 0xC6, 0x00], // U+004D 'M'
    [0xC6, 0xE6, 0xF6, 0xDE, 0xCE, 0xC6, 0xC6, 0x00], // U+004E 'N'
    [0x3C, 0x66, 0x66, 0x66, 0x66, 0x66, 0x3C, 0x00], // U+004F 'O'
    [0x7C, 0x66, 0x66, 0x7C, 0x60, 0x60, 0x60, 0x00], // U+0050 'P'
    [0x3C, 0x66, 0x66, 0x66, 0x66, 0x6C, 0x36, 0x00], // U+0051 'Q'
    [0x7C, 0x66, 0x66, 0x7C, 0x6C, 0x66, 0x66, 0x00], // U+0052 'R'
    [0x3C, 0x66, 0x60, 0x3C, 0x06, 0x66, 0x3C, 0x00], // U+0053 'S'
    [0x7E, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x00], // U+0054 'T'
    [0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x3C, 0x00], // U+0055 'U'
    [0x66, 0x66, 0x66, 0x66, 0x66, 0x3C, 0x18, 0x00], // U+0056 'V'
    [0xC6, 0xC6, 0xC6, 0xD6, 0xFE, 0xEE, 0xC6, 0x00], // U+0057 'W'
    [0xC3, 0x66, 0x3C, 0x18, 0x3C, 0x66, 0xC3, 0x00], // U+0058 'X'
    [0xC3, 0x66, 0x3C, 0x18, 0x18, 0x18, 0x18, 0x00], // U+0059 'Y'
    [0x7E, 0x06, 0x0C, 0x18, 0x30, 0x60, 0x7E, 0x00], // U+005A 'Z'
    [0x3C, 0x30, 0x30, 0x30, 0x30, 0x30, 0x3C, 0x00], // U+005B '['
    [0xC0, 0x60, 0x30, 0x18, 0x0C, 0x06, 0x03, 0x00], // U+005C '\\'
    [0x3C, 0x0C, 0x0C, 0x0C, 0x0C, 0x0C, 0x3C, 0x00], // U+005D ']'
    [0x10, 0x38, 0x6C, 0xC6, 0x00, 0x00, 0x00, 0x00], // U+005E '^'
    [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xFF], // U+005F '_'
    [0x18, 0x0C, 0x06, 0x00, 0x00, 0x00, 0x00, 0x00], // U+0060 '`'
    [0x00, 0x00, 0x3C, 0x06, 0x3E, 0x66, 0x3E, 0x00], // U+0061 'a'
    [0x60, 0x60, 0x7C, 0x66, 0x66, 0x66, 0x7C, 0x00], // U+0062 'b'
    [0x00, 0x00, 0x3C, 0x60, 0x60, 0x60, 0x3C, 0x00], // U+0063 'c'
    [0x06, 0x06, 0x3E, 0x66, 0x66, 0x66, 0x3E, 0x00], // U+0064 'd'
    [0x00, 0x00, 0x3C, 0x66, 0x7E, 0x60, 0x3C, 0x00], // U+0065 'e'
    [0x1C, 0x30, 0x7C, 0x30, 0x30, 0x30, 0x30, 0x00], // U+0066 'f'
    [0x00, 0x00, 0x3E, 0x66, 0x66, 0x3E, 0x06, 0x7C], // U+0067 'g'
    [0x60, 0x60, 0x7C, 0x66, 0x66, 0x66, 0x66, 0x00], // U+0068 'h'
    [0x18, 0x00, 0x38, 0x18, 0x18, 0x18, 0x1E, 0x00], // U+0069 'i'
    [0x0C, 0x00, 0x0C, 0x0C, 0x0C, 0x0C, 0x0C, 0x78], // U+006A 'j'
    [0x60, 0x60, 0x66, 0x6C, 0x78, 0x6C, 0x66, 0x00], // U+006B 'k'
    [0x38, 0x18, 0x18, 0x18, 0x18, 0x18, 0x1E, 0x00], // U+006C 'l'
    [0x00, 0x00, 0xCC, 0xFE, 0xD6, 0xD6, 0xC6, 0x00], // U+006D 'm'
    [0x00, 0x00, 0x7C, 0x66, 0x66, 0x66, 0x66, 0x00], // U+006E 'n'
    [0x00, 0x00, 0x3C, 0x66, 0x66, 0x66, 0x3C, 0x00], // U+006F 'o'
    [0x00, 0x00, 0x7C, 0x66, 0x66, 0x7C, 0x60, 0x60], // U+0070 'p'
    [0x00, 0x00, 0x3E, 0x66, 0x66, 0x3E, 0x06, 0x06], // U+0071 'q'
    [0x00, 0x00, 0x7C, 0x66, 0x60, 0x60, 0x60, 0x00], // U+0072 'r'
    [0x00, 0x00, 0x3E, 0x60, 0x3C, 0x06, 0x7C, 0x00], // U+0073 's'
    [0x30, 0x30, 0x7E, 0x30, 0x30, 0x30, 0x1E, 0x00], // U+0074 't'
    [0x00, 0x00, 0x66, 0x66, 0x66, 0x66, 0x3E, 0x00], // U+0075 'u'
    [0x00, 0x00, 0x66, 0x66, 0x66, 0x3C, 0x18, 0x00], // U+0076 'v'
    [0x00, 0x00, 0xC6, 0xC6, 0xD6, 0x7C, 0x6C, 0x00], // U+0077 'w'
    [0x00, 0x00, 0xC6, 0x6C, 0x38, 0x6C, 0xC6, 0x00], // U+0078 'x'
    [0x00, 0x00, 0x66, 0x66, 0x66, 0x3E, 0x06, 0x3C], // U+0079 'y'
    [0x00, 0x00, 0x7E, 0x0C, 0x18, 0x30, 0x7E, 0x00], // U+007A 'z'
    [0x0E, 0x18, 0x18, 0x70, 0x18, 0x18, 0x0E, 0x00], // U+007B '{'
    [0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x00], // U+007C '|'
    [0x70, 0x18, 0x18, 0x0E, 0x18, 0x18, 0x70, 0x00], // U+007D '}'
    [0x76, 0xDC, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00], // U+007E '~'
    [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00], // U+007F DEL (blank)
];

/// Top-left corner of `character`'s glyph (inside its padding), in atlas pixels.
///
/// `None` only for a codepoint outside the table. A blank-but-present glyph
/// (the space) still returns its cell: the text renderer skips a character
/// whose info is `None` *without advancing the pen*, so returning `None` for a
/// space would collapse it.
fn glyph_origin(character: char) -> Option<(u32, u32)> {
    let index = (character as u32).checked_sub(FIRST_CHAR)?;
    if index >= ATLAS_COLUMNS * ATLAS_ROWS {
        return None;
    }
    let column = index % ATLAS_COLUMNS;
    let row = index / ATLAS_COLUMNS;
    Some((
        column * CELL_PIXELS + CELL_PADDING,
        row * CELL_PIXELS + CELL_PADDING,
    ))
}

/// Metrics for `character`: the UV rect of its glyph and how far it advances
/// the pen. Pure, so the metrics are testable without a GL context.
fn character_info(character: char) -> Option<FontCharacterInfo> {
    let (origin_x, origin_y) = glyph_origin(character)?;
    // Exact texel boundaries, with no half-texel inset: magnified linear
    // sampling reaches at most half a texel past the rect, and what it reaches
    // into is this glyph's own transparent border, so no neighbour can bleed
    // in.
    Some(FontCharacterInfo {
        min_uv_x: origin_x as f32 / ATLAS_WIDTH as f32,
        max_uv_x: (origin_x + GLYPH_PIXELS) as f32 / ATLAS_WIDTH as f32,
        min_uv_y: origin_y as f32 / ATLAS_HEIGHT as f32,
        max_uv_y: (origin_y + GLYPH_PIXELS) as f32 / ATLAS_HEIGHT as f32,
        // Self-spacing square cells, so the advance is the glyph box.
        advance: GLYPH_PIXELS as f32,
    })
}

/// Expand the glyph table into an RGBA atlas: every texel is white, and
/// coverage lives entirely in the alpha channel.
///
/// The colour matters even where nothing is drawn. Blending is
/// non-premultiplied (`SRC_ALPHA, ONE_MINUS_SRC_ALPHA`) and the text shader
/// multiplies the tint by the sample, so a `GL_LINEAR` tap halfway between a
/// lit and an unlit texel returns the average of both channels. Were the unlit
/// texels transparent *black*, that tap would come back at half alpha and half
/// brightness, fringing every magnified glyph edge with a dark halo. Keeping
/// RGB white everywhere makes the interpolation touch alpha only. This is the
/// same convention the `.FON` loader uses (`dark/src/font.rs`).
///
/// Pure - no GL - so the packing is unit-testable headlessly.
fn atlas_texture_data() -> RawTextureData {
    let mut bytes = vec![255u8; (ATLAS_WIDTH * ATLAS_HEIGHT * 4) as usize];
    for texel in bytes.chunks_exact_mut(4) {
        texel[3] = 0;
    }
    for (index, bitmap) in GLYPHS.iter().enumerate() {
        let column = index as u32 % ATLAS_COLUMNS;
        let row = index as u32 / ATLAS_COLUMNS;
        let origin_x = column * CELL_PIXELS + CELL_PADDING;
        let origin_y = row * CELL_PIXELS + CELL_PADDING;
        for (bitmap_row, pixels) in bitmap.iter().enumerate() {
            for bitmap_column in 0..GLYPH_PIXELS {
                if pixels >> (GLYPH_PIXELS - 1 - bitmap_column) & 1 == 0 {
                    continue;
                }
                let x = origin_x + bitmap_column;
                let y = origin_y + bitmap_row as u32;
                // RGB is already white; coverage is alpha alone.
                let offset = ((y * ATLAS_WIDTH + x) * 4) as usize;
                bytes[offset + 3] = 255;
            }
        }
    }

    RawTextureData {
        bytes,
        width: ATLAS_WIDTH,
        height: ATLAS_HEIGHT,
        format: PixelFormat::RGBA,
    }
}

/// The compiled-in fallback font. See the module docs.
pub struct BuiltinFont {
    texture: Rc<Texture>,
}

// No `Default`: constructing this uploads a texture, which needs a current GL
// context - not something a `Default::default()` caller would expect.
#[allow(clippy::new_without_default)]
impl BuiltinFont {
    /// Upload the glyph atlas and build the font. Requires a current GL context.
    pub fn new() -> BuiltinFont {
        BuiltinFont {
            texture: Rc::new(texture::init_from_memory(atlas_texture_data())),
        }
    }
}

impl Font for BuiltinFont {
    fn get_texture(&self) -> Rc<dyn TextureTrait> {
        self.texture.clone()
    }

    fn base_height(&self) -> f32 {
        GLYPH_PIXELS as f32
    }

    fn get_half_pixel(&self) -> f32 {
        0.5 / ATLAS_WIDTH as f32
    }

    fn get_character_info(&self, c: char) -> Option<FontCharacterInfo> {
        character_info(c)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pixel(data: &RawTextureData, x: u32, y: u32) -> [u8; 4] {
        let offset = ((y * data.width + x) * 4) as usize;
        data.bytes[offset..offset + 4].try_into().unwrap()
    }

    /// These rows are upstream `unscii-8.hex` verbatim. This detects an *edit* to
    /// the table; it does not re-derive it from upstream, so it cannot catch a
    /// wholesale swap to a differently-licensed font. Re-check by hand against
    /// the URL in the module docs if the provenance is ever in question.
    #[test]
    fn the_table_matches_upstream_unscii_8() {
        // 00041:183C66667E666600
        assert_eq!(
            GLYPHS[('A' as usize) - 0x20],
            [0x18, 0x3C, 0x66, 0x66, 0x7E, 0x66, 0x66, 0x00]
        );
        // 00020: blank
        assert_eq!(GLYPHS[0], [0x00; 8]);
        // 0007A:00007E0C18307E00
        assert_eq!(
            GLYPHS[('z' as usize) - 0x20],
            [0x00, 0x00, 0x7E, 0x0C, 0x18, 0x30, 0x7E, 0x00]
        );
    }

    #[test]
    fn atlas_is_a_full_grid_of_padded_cells() {
        let data = atlas_texture_data();
        assert_eq!((data.width, data.height), (160, 60));
        assert_eq!(data.bytes.len(), (160 * 60 * 4) as usize);
        assert_eq!(GLYPHS.len(), (ATLAS_COLUMNS * ATLAS_ROWS) as usize);
    }

    /// The whole point of the padding: with `GL_LINEAR` magnification, a lit
    /// pixel at a glyph's edge must not sit against the next cell's glyph.
    ///
    /// Scans the whole atlas rather than sampling each cell's leading edge, so
    /// an origin or stride error shows up as a lit pixel in a border.
    #[test]
    fn no_glyph_pixel_escapes_its_padded_cell() {
        let data = atlas_texture_data();
        for y in 0..ATLAS_HEIGHT {
            for x in 0..ATLAS_WIDTH {
                let inside_x = (x % CELL_PIXELS) >= CELL_PADDING
                    && (x % CELL_PIXELS) < CELL_PADDING + GLYPH_PIXELS;
                let inside_y = (y % CELL_PIXELS) >= CELL_PADDING
                    && (y % CELL_PIXELS) < CELL_PADDING + GLYPH_PIXELS;
                if !(inside_x && inside_y) {
                    assert_eq!(
                        pixel(&data, x, y),
                        [255, 255, 255, 0],
                        "lit or non-white texel in the border at ({x}, {y})"
                    );
                }
            }
        }
    }

    /// Unlit texels must be white with zero alpha, not transparent black:
    /// blending is non-premultiplied, so black would darken every magnified
    /// glyph edge as `GL_LINEAR` interpolates toward it.
    #[test]
    fn unlit_texels_are_white_with_zero_alpha() {
        let data = atlas_texture_data();
        let (origin_x, origin_y) = glyph_origin('A').unwrap();
        // 'A' row 0 is 0x18 - columns 3 and 4 lit, so column 0 is unlit.
        assert_eq!(pixel(&data, origin_x, origin_y), [255, 255, 255, 0]);
        for texel in data.bytes.chunks_exact(4) {
            assert_eq!(&texel[0..3], &[255, 255, 255], "every texel is white");
        }
    }

    /// Bit order: the most significant bit of a row byte is its leftmost pixel.
    #[test]
    fn glyph_bits_expand_most_significant_bit_first() {
        let data = atlas_texture_data();
        // 'A' is 0x18 on its top row: bits 3 and 4 set, so within the glyph the
        // lit pixels of that row are columns 3 and 4.
        let (origin_x, origin_y) = glyph_origin('A').unwrap();
        assert_eq!(pixel(&data, origin_x + 3, origin_y), [255, 255, 255, 255]);
        assert_eq!(pixel(&data, origin_x + 4, origin_y), [255, 255, 255, 255]);
        assert_eq!(pixel(&data, origin_x + 2, origin_y), [255, 255, 255, 0]);
    }

    /// A space draws nothing but must still advance, because the text renderer
    /// drops a `None` character *without* moving the pen.
    #[test]
    fn space_has_metrics_so_it_is_not_collapsed() {
        let space = character_info(' ').expect("space must have metrics");
        assert_eq!(space.advance, GLYPH_PIXELS as f32);
    }

    #[test]
    fn characters_outside_printable_ascii_have_no_metrics() {
        assert!(character_info('\u{00E9}').is_none());
        assert!(character_info('\u{0080}').is_none());
        assert!(character_info('\n').is_none());
    }

    #[test]
    fn glyph_uvs_stay_inside_their_own_cell() {
        let a = character_info('A').unwrap();
        // One glyph box wide/tall, exactly.
        let width = (a.max_uv_x - a.min_uv_x) * ATLAS_WIDTH as f32;
        let height = (a.max_uv_y - a.min_uv_y) * ATLAS_HEIGHT as f32;
        assert!((width - GLYPH_PIXELS as f32).abs() < 1e-4);
        assert!((height - GLYPH_PIXELS as f32).abs() < 1e-4);
        // 'A' is cell 33: column 1, row 2. Its glyph starts one pixel in.
        assert_eq!(
            glyph_origin('A'),
            Some((1 * CELL_PIXELS + 1, 2 * CELL_PIXELS + 1))
        );
    }
}
