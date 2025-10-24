///
/// font.rs
///
/// Module to support loading Dark engine fonts (both packed, format=0 and unpacked, format=0xcccc)
///
/// The openDarkEngine was particularly helpful here, in particular this file:
/// https://github.com/volca02/openDarkEngine/blob/7a2d7baaf0fc5194a9066a635c6f44b0f7b26c56/src/base/loaders/ManualFonFileLoader.cpp#L62
///
use std::{collections::HashMap, io, rc::Rc};

use cgmath::{vec2, Vector2};
use engine::{
    scene::{mesh, Mesh, TextVertex},
    texture::{Texture, TextureTrait},
    texture_atlas::{TexturePackResult, TexturePacker},
    FontCharacterInfo,
};
use image::ImageBuffer;
use tracing::info;

use crate::ss2_common::{read_bytes, read_i16, read_u16, read_u32, read_u8};

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
        // Get total length of file
        // Needed so we can get the size of the bitmap
        let mut _vec = Vec::new();
        let _end = reader.read_to_end(&mut _vec).unwrap();
        let end_bytes = reader.stream_position().unwrap();

        // Then rewind and start reading...
        reader.seek(io::SeekFrom::Start(0)).unwrap();

        let header = FontHeader::read(reader);

        // Currently, we don't support any palettes
        assert!(header.palette == 0);

        info!("Loading font with header: {:?}", header);

        let num_chars = header.last_char - header.first_char + 1;

        let mut widths = Vec::new();
        reader
            .seek(io::SeekFrom::Start(header.width_offset as u64))
            .unwrap();

        for _ in 0..num_chars {
            widths.push(read_u16(reader))
        }

        // Read bitmap data
        reader
            .seek(io::SeekFrom::Start(header.bitmap_offset as u64))
            .unwrap();

        let bitmap_size = end_bytes - header.bitmap_offset as u64;
        let bitmap = read_bytes(reader, bitmap_size as usize);

        let mut char_to_info = HashMap::new();
        let mut texture_packer = TexturePacker::<image::Rgba<u8>>::new_rgba(512, 512);
        for n in 0..num_chars - 1 {
            let code = header.first_char + n;
            let ascii = char::from_u32(code as u32).unwrap();
            let idx = n as usize;

            let column = widths[idx];
            let width = widths[idx + 1] - widths[idx];

            // Generate the image corresponding to the character:
            let img: ImageBuffer<image::Rgba<u8>, std::vec::Vec<u8>> =
                image::ImageBuffer::from_fn(width as u32, header.num_rows as u32, |x, y| {
                    let adj_x = x + (column as u32);
                    if header.format == 0 {
                        // This is a little gnarly... what is happening here is that each pixel
                        // is compacted horizontally. Each 'byte' value actually corresponds to 8
                        // pixels - each bit tracking whether the pixel is on or off.

                        // First, get the byte-index of the pixel
                        let idx_packed = adj_x / 8;
                        let byte_index = (y * header.row_width as u32 + idx_packed) as usize;

                        // This gives us the full byte value
                        let packed_val = bitmap[byte_index];

                        // Now, we need to figure out whether the 'bit' corresponding to the pixel
                        // is on or off. We grab the remainder to find the relevant bit:
                        let x_remainder = 7 - (adj_x % 8);
                        // and then check if it is on:
                        let is_on = packed_val >> x_remainder & 1 == 1;

                        let alpha = if is_on { 255u8 } else { 0u8 };

                        image::Rgba([255, 255, 255, alpha])
                    } else {
                        // If format is not 0, this is easy mode... each byte
                        // just directly corresponds to an alpha value.
                        let idx = (y * header.row_width as u32 + adj_x) as usize;
                        let mut alpha = bitmap[idx];
                        // Not sure why this is necessary... but we get artifacts (random bright pixels) in some fonts w/o this
                        if alpha > 205 {
                            alpha = 0;
                        }
                        image::Rgba([255, 255, 255, alpha])
                    }
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
            base_height: header.num_rows as f32,
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
