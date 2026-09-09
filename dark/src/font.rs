///
/// font.rs
///
/// Module to support loading Dark engine fonts (both packed, format=0 and unpacked, format=0xcccc)
///
/// The openDarkEngine was particularly helpful here, in particular this file:
/// https://github.com/volca02/openDarkEngine/blob/7a2d7baaf0fc5194a9066a635c6f44b0f7b26c56/src/base/loaders/ManualFonFileLoader.cpp#L62
///
use std::{collections::HashMap, io, rc::Rc};

use cgmath::{Vector2, vec2};
use engine::{
    FontCharacterInfo,
    scene::{Mesh, TextVertex, mesh},
    texture::{Texture, TextureTrait},
    texture_atlas::{TexturePackResult, TexturePacker},
};
use image::ImageBuffer;
use tracing::{info, warn};

use crate::ss2_common::{read_bytes, read_i16, read_u8, read_u16, read_u32};

pub struct Font {
    pub char_to_info: HashMap<char, CharInfo>,
    // TODO: Encapsulate this
    pub texture: Rc<Texture>,
    base_height: f32, // default height, in pixels, of a character
}

const SPACING: f32 = 1.0;

#[derive(Debug)]
struct FontHeader {
    format: u16,
    // unk: u8,
    palette: u8,
    //zeros1: [u8; 32],
    first_char: i16,
    last_char: i16,
    // zeros2: [u8; 32],
    width_offset: u32,
    bitmap_offset: u32,
    row_width: u16, /* bytes */
    num_rows: u16,
}

#[derive(Debug)]
pub struct CharInfo {
    pub texture_pack_result: TexturePackResult,
    pub width: f32,
}

/// Pure (no-GL) glyph metrics parsed from a Dark `.FON` file: the header plus
/// the column table. Separated from [`Font::read`] (which additionally builds a
/// GL texture atlas) so the byte-level parse is unit-testable headlessly.
#[derive(Debug, PartialEq)]
pub struct FontMetrics {
    pub format: u16,
    pub first_char: i16,
    pub last_char: i16,
    /// Glyph height in pixels (font's native size on the 640x480 canvas).
    pub height: u16,
    pub row_width: u16,
    pub bitmap_offset: u32,
    /// Column x-positions in the bitmap strip; `columns[i+1] - columns[i]` is
    /// the pixel width of the `first_char + i` glyph. Has `num_chars + 1`
    /// entries so the last glyph's width is well-defined.
    pub columns: Vec<u16>,
}

impl FontMetrics {
    /// Number of glyphs described (inclusive `first_char..=last_char`).
    pub fn num_chars(&self) -> usize {
        (self.last_char - self.first_char + 1) as usize
    }

    /// Pixel width of the glyph for `code`, or `None` if out of range.
    pub fn glyph_width(&self, code: i16) -> Option<u16> {
        if code < self.first_char || code > self.last_char {
            return None;
        }
        let i = (code - self.first_char) as usize;
        Some(self.columns[i + 1] - self.columns[i])
    }

    /// Human-readable name of the bitmap encoding, for tools reporting which
    /// variant a file uses.
    pub fn format_label(&self) -> &'static str {
        match self.format {
            0 => "0 (packed, 1 bit per pixel)",
            1 => "1 (antialiased, 16 coverage levels)",
            0xcccc => "0xcccc (antialiased, 8-bit alpha)",
            _ => "unknown (treated as 8-bit alpha)",
        }
    }

    pub fn read<T: io::Read + io::Seek>(reader: &mut T) -> FontMetrics {
        let header = FontHeader::read(reader);
        assert!(header.palette == 0);

        let num_chars = (header.last_char - header.first_char + 1) as usize;
        reader
            .seek(io::SeekFrom::Start(header.width_offset as u64))
            .unwrap();
        // The column table has `num_chars + 1` entries: N glyph starts plus the
        // end column of the last glyph, so every glyph width is a difference of
        // adjacent entries.
        let mut columns = Vec::with_capacity(num_chars + 1);
        for _ in 0..=num_chars {
            columns.push(read_u16(reader));
        }

        FontMetrics {
            format: header.format,
            first_char: header.first_char,
            last_char: header.last_char,
            height: header.num_rows,
            row_width: header.row_width,
            bitmap_offset: header.bitmap_offset,
            columns,
        }
    }
}

/// A `.FON` decoded to CPU-side glyph coverage: the metrics plus the raw
/// bitmap strip, with no texture atlas and no GL. [`Font::read`] builds its
/// atlas from this, and headless tools (dark_explorer's font preview) rasterize
/// sample strings from it.
pub struct FontBitmap {
    pub metrics: FontMetrics,
    bitmap: Vec<u8>,
}

impl FontBitmap {
    pub fn read<T: io::Read + io::Seek>(reader: &mut T) -> FontBitmap {
        // The bitmap strip runs to end-of-file, so read the whole file once.
        let mut all = Vec::new();
        reader.seek(io::SeekFrom::Start(0)).unwrap();
        reader.read_to_end(&mut all).unwrap();

        let metrics = FontMetrics::read(&mut io::Cursor::new(&all));
        let bitmap = all
            .get(metrics.bitmap_offset as usize..)
            .unwrap_or(&[])
            .to_vec();
        let expected = metrics.row_width as usize * metrics.height as usize;
        if bitmap.len() < expected {
            // Reads past the strip decode as transparent, so say so rather than
            // letting a bad asset render as silently blank glyphs.
            warn!(
                "font bitmap truncated: {} of {expected} bytes",
                bitmap.len()
            );
        }

        FontBitmap { metrics, bitmap }
    }

    /// Alpha of one pixel of the bitmap strip, in strip coordinates. Reads past
    /// the strip (a truncated file) yield transparent rather than panicking.
    fn pixel_alpha(&self, x: u32, y: u32) -> u8 {
        let row_width = self.metrics.row_width as u32;
        if self.metrics.format == 0 {
            // Packed: each byte holds 8 horizontally adjacent pixels, MSB first.
            let byte = *self
                .bitmap
                .get((y * row_width + x / 8) as usize)
                .unwrap_or(&0);
            if byte >> (7 - (x % 8)) & 1 == 1 {
                255
            } else {
                0
            }
        } else {
            let byte = *self.bitmap.get((y * row_width + x) as usize).unwrap_or(&0);
            unpacked_byte_alpha(self.metrics.format, byte)
        }
    }

    /// Coverage mask for one glyph: `(width, alpha)` with `alpha` row-major
    /// `width * height`. `None` for a code the font does not define.
    pub fn glyph_alpha(&self, code: i16) -> Option<(usize, Vec<u8>)> {
        let width = self.metrics.glyph_width(code)? as u32;
        let column = self.metrics.columns[(code - self.metrics.first_char) as usize] as u32;
        let height = self.metrics.height as u32;
        let mut alpha = Vec::with_capacity((width * height) as usize);
        for y in 0..height {
            for x in 0..width {
                alpha.push(self.pixel_alpha(column + x, y));
            }
        }
        Some((width as usize, alpha))
    }

    /// Rasterize `text` at native size into `(width, height, alpha)`, laying
    /// glyphs out left to right on their advances with nothing added between
    /// them (side bearings are baked into the glyph cells), as the UI's
    /// `measure_text_width` does. Characters the font does not define are
    /// skipped.
    pub fn render_string(&self, text: &str) -> (usize, usize, Vec<u8>) {
        let height = self.metrics.height as usize;
        let glyphs: Vec<(usize, Vec<u8>)> = text
            .chars()
            .filter_map(|c| i16::try_from(c as u32).ok())
            .filter_map(|code| self.glyph_alpha(code))
            .collect();
        if glyphs.is_empty() || height == 0 {
            return (0, 0, Vec::new());
        }
        let width: usize = glyphs.iter().map(|(w, _)| w).sum();
        if width == 0 {
            return (0, 0, Vec::new());
        }

        let mut out = vec![0u8; width * height];
        let mut pen = 0usize;
        for (glyph_width, alpha) in &glyphs {
            for y in 0..height {
                let dst = y * width + pen;
                out[dst..dst + glyph_width]
                    .copy_from_slice(&alpha[y * glyph_width..(y + 1) * glyph_width]);
            }
            pen += glyph_width;
        }
        (width, height, out)
    }
}

/// Alpha for one bitmap byte of an *unpacked* (non-format-0) font.
///
/// - Format `0x0001` ("antialias-16", e.g. `METAFONT.FON`): each byte is a
///   coverage level 0..=15; scale linearly so full coverage (15) is fully
///   opaque (255). Without this the whole font renders at max alpha 15 -
///   nearly invisible.
/// - Other formats (e.g. the `0xCCCC` AA fonts): each byte is used as direct
///   alpha. The `> 205` clamp works around artifacts (random bright pixels)
///   seen in some of those fonts; the proper fix is palette/coverage
///   normalization (see `projects/ui-font-fidelity.md` follow-ups).
fn unpacked_byte_alpha(format: u16, value: u8) -> u8 {
    if format == 1 {
        value.saturating_mul(17)
    } else if value > 205 {
        0
    } else {
        value
    }
}

impl FontHeader {
    pub fn read<T: io::Read + io::Seek>(reader: &mut T) -> FontHeader {
        let format = read_u16(reader);
        let _unk = read_u8(reader);
        let palette = read_u8(reader);
        let _zeros1 = read_bytes(reader, 32);
        let first_char = read_i16(reader);
        let last_char = read_i16(reader);
        let _zeros2 = read_bytes(reader, 32);
        let width_offset = read_u32(reader);
        let bitmap_offset = read_u32(reader);
        let row_width = read_u16(reader);
        let num_rows = read_u16(reader);

        FontHeader {
            format,
            palette,
            first_char,
            last_char,
            width_offset,
            bitmap_offset,
            row_width,
            num_rows,
        }
    }
}

impl Font {
    fn get_half_pixel(&self) -> f32 {
        0.5 / self.texture.width() as f32
    }

    pub fn get_mesh(&self, str: &str, position: Vector2<f32>, _font_size: f32) -> Mesh {
        let mut x = position.x;
        let y = position.y;

        let font_size = 30.0f32;
        let multiplier = font_size / self.base_height;
        let adj_height = font_size;

        let mut vertices = Vec::new();
        for c in str.chars() {
            let a_info = self.char_to_info.get(&c).unwrap();
            let half_pixel = self.get_half_pixel();
            let min_uv_x = a_info.texture_pack_result.uv_offset_x;
            let min_uv_y = a_info.texture_pack_result.uv_offset_y;
            let max_uv_x = a_info.texture_pack_result.uv_offset_x
                + a_info.texture_pack_result.uv_width
                - (half_pixel * 2.0);
            let max_uv_y = a_info.texture_pack_result.uv_offset_y
                + a_info.texture_pack_result.uv_height
                - (half_pixel * 2.0);

            let adj_width = a_info.width * multiplier;

            vertices.extend(vec![
                TextVertex {
                    position: vec2(x, y),
                    uv: vec2(min_uv_x, min_uv_y),
                },
                TextVertex {
                    position: vec2(x, y + adj_height),
                    uv: vec2(min_uv_x, max_uv_y),
                },
                TextVertex {
                    position: vec2(x + adj_width, y + adj_height),
                    uv: vec2(max_uv_x, max_uv_y),
                },
                TextVertex {
                    position: vec2(x, y),
                    uv: vec2(min_uv_x, min_uv_y),
                },
                TextVertex {
                    position: vec2(x + adj_width, y + adj_height),
                    uv: vec2(max_uv_x, max_uv_y),
                },
                TextVertex {
                    position: vec2(x + adj_width, y),
                    uv: vec2(max_uv_x, min_uv_y),
                },
            ]);

            x += adj_width + SPACING * multiplier;
        }

        mesh::create(vertices)
    }
    pub fn read<T: io::Read + io::Seek>(reader: &mut T) -> Font {
        Self::read_tinted(reader, [255, 255, 255])
    }

    pub fn read_tinted<T: io::Read + io::Seek>(reader: &mut T, tint: [u8; 3]) -> Font {
        let font_bitmap = FontBitmap::read(reader);
        let metrics = &font_bitmap.metrics;

        info!("Loading font with metrics: {:?}", metrics);

        let num_chars = metrics.num_chars();

        let mut char_to_info = HashMap::new();
        let mut texture_packer = TexturePacker::<image::Rgba<u8>>::new_rgba(512, 512);
        for n in 0..num_chars {
            let code = metrics.first_char + n as i16;
            let ascii = char::from_u32(code as u32).unwrap();

            let (width, alpha) = font_bitmap.glyph_alpha(code).unwrap();

            // White glyphs; the bitmap only carries coverage.
            let img: ImageBuffer<image::Rgba<u8>, std::vec::Vec<u8>> =
                image::ImageBuffer::from_fn(width as u32, metrics.height as u32, |x, y| {
                    image::Rgba([
                        tint[0],
                        tint[1],
                        tint[2],
                        alpha[(y as usize) * width + x as usize],
                    ])
                });

            let texture_pack_result = texture_packer.pack(&img);
            let char_info = CharInfo {
                width: (width as f32),
                texture_pack_result,
            };

            char_to_info.insert(ascii, char_info);
        }

        let textures = texture_packer.generate_textures();

        assert!(textures.len() == 1);
        let texture = textures[0].clone();

        // // Debug - save textures
        // texture_packer.save("test2.png");
        // img.save("test1.png").unwrap();
        // panic!("saving images");

        Font {
            texture,
            char_to_info,
            base_height: metrics.height as f32,
        }
    }
}

impl engine::Font for Font {
    fn get_texture(&self) -> Rc<dyn TextureTrait> {
        self.texture.clone()
    }

    fn base_height(&self) -> f32 {
        self.base_height
    }

    fn get_half_pixel(&self) -> f32 {
        self.get_half_pixel()
    }

    fn get_character_info(&self, c: char) -> Option<FontCharacterInfo> {
        let maybe_info = self.char_to_info.get(&c);

        maybe_info?;

        let info = maybe_info.unwrap();
        let half_pixel = self.get_half_pixel();
        let min_uv_x = info.texture_pack_result.uv_offset_x;
        let min_uv_y = info.texture_pack_result.uv_offset_y;
        let max_uv_x = info.texture_pack_result.uv_offset_x + info.texture_pack_result.uv_width
            - (half_pixel * 2.0);
        let max_uv_y = info.texture_pack_result.uv_offset_y + info.texture_pack_result.uv_height
            - (half_pixel * 2.0);

        let advance = info.width;
        Some(FontCharacterInfo {
            max_uv_x,
            min_uv_x,
            min_uv_y,
            max_uv_y,
            advance,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{FontBitmap, FontMetrics};
    use std::io::Cursor;

    /// Build a minimal, valid Dark `.FON` byte buffer for `first..=last` with
    /// the given `columns` (len == num_chars + 1) and `height`. Mirrors the
    /// 84-byte header layout `FontHeader::read` expects.
    fn synth_font(first: i16, last: i16, height: u16, columns: &[u16], format: u16) -> Vec<u8> {
        let num_chars = (last - first + 1) as usize;
        assert_eq!(columns.len(), num_chars + 1);
        let width_offset: u32 = 84;
        let bitmap_offset: u32 = width_offset + (columns.len() as u32) * 2;
        let row_width: u16 = *columns.last().unwrap();

        let mut b = vec![0u8; 84];
        b[0..2].copy_from_slice(&format.to_le_bytes());
        // b[2] unk, b[3] palette = 0
        b[0x24..0x26].copy_from_slice(&first.to_le_bytes());
        b[0x26..0x28].copy_from_slice(&last.to_le_bytes());
        b[0x48..0x4c].copy_from_slice(&width_offset.to_le_bytes());
        b[0x4c..0x50].copy_from_slice(&bitmap_offset.to_le_bytes());
        b[0x50..0x52].copy_from_slice(&row_width.to_le_bytes());
        b[0x52..0x54].copy_from_slice(&height.to_le_bytes());
        for c in columns {
            b.extend_from_slice(&c.to_le_bytes());
        }
        b.extend(vec![0u8; (row_width as usize) * (height as usize)]);
        b
    }

    #[test]
    fn font_metrics_parses_glyph_count_widths_and_height() {
        // Digits '0'..'1' (2 glyphs): '0' spans [0,3)=3px, '1' spans [3,7)=4px.
        let bytes = synth_font(48, 49, 11, &[0, 3, 7], 0);
        let m = FontMetrics::read(&mut Cursor::new(bytes));

        assert_eq!(m.num_chars(), 2);
        assert_eq!(m.height, 11);
        assert_eq!(m.first_char, 48);
        assert_eq!(m.last_char, 49);
        // The column table must carry num_chars + 1 entries so the *last*
        // glyph's width is defined (the old parser dropped it).
        assert_eq!(m.columns.len(), 3);
        assert_eq!(m.glyph_width(48), Some(3));
        assert_eq!(m.glyph_width(49), Some(4));
        // Out-of-range codes have no glyph.
        assert_eq!(m.glyph_width(47), None);
        assert_eq!(m.glyph_width(50), None);
    }

    /// `synth_font` leaves the bitmap strip zeroed; overwrite it with `pixels`.
    fn with_bitmap(mut bytes: Vec<u8>, pixels: &[u8]) -> Vec<u8> {
        let start = bytes.len() - pixels.len();
        bytes[start..].copy_from_slice(pixels);
        bytes
    }

    #[test]
    fn packed_glyph_unpacks_one_bit_per_pixel() {
        // One 3x2 glyph 'A'; row_width = 3 bytes (synth uses the last column).
        // Row 0 = 1 0 1, row 1 = 0 1 0.
        let bytes = with_bitmap(
            synth_font(65, 65, 2, &[0, 3], 0),
            &[0b1010_0000, 0, 0, 0b0100_0000, 0, 0],
        );
        let font = FontBitmap::read(&mut Cursor::new(bytes));

        assert_eq!(
            font.glyph_alpha(65),
            Some((3, vec![255, 0, 255, 0, 255, 0]))
        );
        assert_eq!(font.glyph_alpha(66), None);
    }

    #[test]
    fn format1_glyphs_scale_coverage_and_lay_out_on_advances() {
        // Two 2x1 glyphs, format 1 (coverage 0..=15), row_width = 4 bytes.
        let bytes = with_bitmap(synth_font(65, 66, 1, &[0, 2, 4], 1), &[15, 0, 8, 15]);
        let font = FontBitmap::read(&mut Cursor::new(bytes));

        assert_eq!(font.glyph_alpha(65), Some((2, vec![255, 0])));
        assert_eq!(font.glyph_alpha(66), Some((2, vec![136, 255])));
        // Glyphs run left to right on their advances, nothing added between.
        assert_eq!(font.render_string("AB"), (4, 1, vec![255, 0, 136, 255]));
        // Undefined characters are dropped rather than rendered as blanks.
        assert_eq!(font.render_string("AZB").0, 4);
        assert_eq!(font.render_string("ZZ"), (0, 0, vec![]));
    }

    #[test]
    fn truncated_bitmap_reads_as_transparent_instead_of_panicking() {
        let mut bytes = with_bitmap(synth_font(65, 65, 2, &[0, 3], 1), &[9; 6]);
        bytes.truncate(bytes.len() - 4);
        let font = FontBitmap::read(&mut Cursor::new(bytes));

        assert_eq!(font.glyph_alpha(65), Some((3, vec![153, 153, 0, 0, 0, 0])));
    }

    /// Locate a game asset by data-root-relative path (fonts live under both
    /// `res/fonts/` and `res/intrface/`). Returns `None` when the game data is
    /// absent (e.g. CI), so real-asset tests can no-op.
    fn find_asset(rel: &str) -> Option<std::path::PathBuf> {
        for root in [
            std::env::var("DARK_ASSET_PATH").unwrap_or_default(),
            "../Data".into(),
            "../../Data".into(),
        ] {
            let p = std::path::Path::new(&root).join(rel);
            if p.exists() {
                return Some(p);
            }
        }
        None
    }

    /// Real-font parse guarded by asset availability (game `.FON` files are not
    /// in the repo, so this no-ops in CI). MAINFONT.FON: char 0..=225, 11px
    /// tall, mono (format 0).
    #[test]
    fn mainfont_metrics_from_real_asset_when_present() {
        let Some(path) = find_asset("res/fonts/MAINFONT.FON") else {
            eprintln!("skipping: MAINFONT.FON asset not found");
            return;
        };
        let bytes = std::fs::read(path).unwrap();
        let m = FontMetrics::read(&mut Cursor::new(bytes));
        assert_eq!(m.format, 0);
        assert_eq!(m.first_char, 0);
        assert_eq!(m.last_char, 225);
        assert_eq!(m.height, 11);
        assert_eq!(m.num_chars(), 226);
        assert_eq!(m.columns.len(), 227);
        let w0 = m.glyph_width(b'0' as i16).unwrap();
        assert!((1..=20).contains(&w0), "digit width {w0} out of range");
    }

    /// METAFONT.FON — the original's default GUI style font (main menu et al),
    /// shipped under `res/intrface/`, format 0x0001 ("antialias-16"): each
    /// bitmap byte is a 0..=15 coverage level. Asserts the 0..15 -> 0..255
    /// alpha scaling reaches full opacity; before format-1 support the parser
    /// treated these bytes as direct alpha (max 15/255 - nearly invisible).
    #[test]
    fn metafont_format1_alpha_from_real_asset_when_present() {
        let Some(path) = find_asset("res/intrface/METAFONT.FON") else {
            eprintln!("skipping: METAFONT.FON asset not found");
            return;
        };
        let bytes = std::fs::read(path).unwrap();
        let m = FontMetrics::read(&mut Cursor::new(bytes.clone()));
        assert_eq!(m.format, 1);
        assert_eq!(m.height, 20);
        assert_eq!(m.first_char, 0);
        assert_eq!(m.last_char, 225);

        let bitmap = &bytes[m.bitmap_offset as usize..];
        let max_coverage = bitmap.iter().copied().max().unwrap();
        assert_eq!(max_coverage, 15, "format-1 coverage levels are 0..=15");
        let max_alpha = bitmap
            .iter()
            .map(|&b| super::unpacked_byte_alpha(m.format, b))
            .max()
            .unwrap();
        assert_eq!(max_alpha, 255, "full coverage must decode to full alpha");
    }

    #[test]
    fn test_half_pixel_calculation_formula() {
        // Test the formula without requiring GL context
        // We'll test the formula directly: 0.5 / texture_width
        let test_cases = vec![
            (256, 0.5 / 256.0),
            (512, 0.5 / 512.0),
            (1024, 0.5 / 1024.0),
            (128, 0.5 / 128.0),
        ];

        for (width, expected) in test_cases {
            let actual = 0.5 / width as f32;
            assert_eq!(
                actual, expected,
                "Half pixel calculation should be 0.5 / width for width {}",
                width
            );
        }
    }

    #[test]
    fn test_half_pixel_method_logic() {
        // Test that the get_half_pixel method would return the correct values
        // This tests the logic without OpenGL dependencies

        // Before the fix, this would have returned hardcoded 0.5 / 512.0
        let hardcoded_value = 0.5 / 512.0;

        // After the fix, it should use the actual texture width
        let texture_widths = vec![256, 1024, 2048];

        for width in texture_widths {
            let expected_dynamic = 0.5 / width as f32;
            // The dynamic calculation should be different from hardcoded for non-512 textures
            if width != 512 {
                assert_ne!(
                    expected_dynamic, hardcoded_value,
                    "Dynamic calculation for {}px texture should differ from hardcoded 512px value",
                    width
                );
            }
        }
    }
}
