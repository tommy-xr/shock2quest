use crate::texture_format::RawTextureData;
use crate::{texture::Texture, texture_format::PixelFormat};
use image::{ImageBuffer, Pixel, Rgb, Rgba};
use std::rc::Rc;
struct TextureAtlas<T>
where
    T: Pixel<Subpixel = u8> + image::PixelWithColorType,
{
    img: ImageBuffer<T, std::vec::Vec<u8>>,
    format: PixelFormat,
}

#[derive(Debug, Copy, Clone)]
pub struct TexturePackResult {
    // Index of the atlas the value was packed in - is it necessary?
    pub atlas_index: u32,

    // The offset of the starting pixel, in UV coordinates
    pub uv_offset_x: f32,
    pub uv_offset_y: f32,

    // The width of the starting pixel, in UV coordinates
    pub uv_width: f32,
    pub uv_height: f32,
}

impl TexturePackResult {
    pub const DEFAULT: TexturePackResult = TexturePackResult {
        atlas_index: 0,
        uv_offset_x: 0.0,
        uv_offset_y: 0.0,
        uv_width: 0.0,
        uv_height: 0.0,
    };
}

impl<T> TextureAtlas<T>
where
    T: Pixel<Subpixel = u8> + image::PixelWithColorType,
{
    pub fn set_pixel(&mut self, x: u32, y: u32, pixel: T) {
        self.img.put_pixel(x, y, pixel);
    }

    pub fn save(&mut self, name: &str) {
        self.img.save(name).unwrap();
    }

    pub fn generate_texture_with(&self, options: &crate::texture::TextureOptions) -> Texture {
        let width = self.img.width();
        let height = self.img.height();
        let data = self.img.clone().into_raw();
        crate::texture::init_from_memory2(
            RawTextureData {
                bytes: data,
                width,
                height,
                format: self.format,
            },
            options,
        )
    }

    pub fn new_rgb(width: u32, height: u32) -> TextureAtlas<image::Rgb<u8>> {
        let img = image::ImageBuffer::from_fn(width, height, |_x, _y| image::Rgb([255, 255, 0]));

        TextureAtlas {
            img,
            format: PixelFormat::RGB,
        }
    }

    pub fn new_rgba(width: u32, height: u32) -> TextureAtlas<image::Rgba<u8>> {
        let img =
            image::ImageBuffer::from_fn(width, height, |_x, _y| image::Rgba([255, 0, 255, 0]));

        TextureAtlas {
            img,
            format: PixelFormat::RGBA,
        }
    }
}

pub struct TexturePacker<PixelFormat>
where
    PixelFormat: Pixel<Subpixel = u8> + image::PixelWithColorType,
{
    atlases: Vec<TextureAtlas<PixelFormat>>,
    #[allow(dead_code)]
    current_atlas_idx: u8,
    current_atlas_pixel_y: u32,
    current_atlas_pixel_x: u32,
    current_row_max_height: u32,
    pixel_width: u32,
    pixel_height: u32,
}

impl<PixelFormat> TexturePacker<PixelFormat>
where
    PixelFormat: Pixel<Subpixel = u8> + image::PixelWithColorType,
{
    pub fn new_rgb(width: u32, height: u32) -> TexturePacker<Rgb<u8>> {
        let atlases = vec![TextureAtlas::<Rgb<u8>>::new_rgb(width, height)];
        TexturePacker {
            atlases,
            current_atlas_idx: 0,
            current_atlas_pixel_x: 0,
            current_atlas_pixel_y: 0,
            current_row_max_height: 16,
            pixel_width: width,
            pixel_height: height,
        }
    }

    pub fn new_rgba(width: u32, height: u32) -> TexturePacker<Rgba<u8>> {
        let atlases = vec![TextureAtlas::<Rgba<u8>>::new_rgba(width, height)];
        TexturePacker {
            atlases,
            current_atlas_idx: 0,
            current_atlas_pixel_x: 0,
            current_atlas_pixel_y: 0,
            current_row_max_height: 16,
            pixel_width: width,
            pixel_height: height,
        }
    }

    pub fn pack(&mut self, img: &ImageBuffer<PixelFormat, std::vec::Vec<u8>>) -> TexturePackResult {
        self.pack_padded(img, 0)
    }

    /// Pack `img` with `padding` texels of empty space reserved around it, and
    /// return the UVs of the **image**, not of the padded cell.
    ///
    /// A glyph atlas needs this. Cells packed flush against each other share an
    /// edge, so sampling one under `GL_LINEAR` at any non-integer scale - a
    /// 640x480 canvas in an 800x600 window, say - interpolates a glyph's outer
    /// texels with whatever is beside it in the atlas, which is the next glyph.
    /// Narrow glyphs (`I`, `l`, `1`) get swallowed by their neighbours that
    /// way. The padding gives that interpolation empty space to reach into.
    pub fn pack_padded(
        &mut self,
        img: &ImageBuffer<PixelFormat, std::vec::Vec<u8>>,
        padding: u32,
    ) -> TexturePackResult {
        let width = img.width();
        let height = img.height();

        self.reserve_space(width + padding * 2, height + padding * 2);

        let origin_x = self.current_atlas_pixel_x + padding;
        let origin_y = self.current_atlas_pixel_y + padding;
        let current_image = self.atlases.get_mut(0).unwrap();
        // Copy image into atlas at current position
        for x in 0..width {
            for y in 0..height {
                let pixel = img.get_pixel(x, y);
                current_image.set_pixel(origin_x + x, origin_y + y, *pixel);
            }
        }

        let uv_offset_x = origin_x as f32 / self.pixel_width as f32;
        let uv_offset_y = origin_y as f32 / self.pixel_height as f32;
        let uv_width = width as f32 / self.pixel_width as f32;
        let uv_height = height as f32 / self.pixel_height as f32;

        // Update cursor
        self.current_atlas_pixel_x += width + padding * 2;
        self.current_row_max_height = self.current_row_max_height.max(height + padding * 2);

        TexturePackResult {
            atlas_index: 0,
            uv_offset_x,
            uv_offset_y,
            uv_width,
            uv_height,
        }
    }

    pub fn save(&mut self, name: &str) {
        let current_image = self.atlases.get_mut(0).unwrap();
        current_image.save(name);
    }

    pub fn generate_textures(&self) -> Vec<Rc<Texture>> {
        self.generate_textures_with(&crate::texture::TextureOptions::default())
    }

    /// [`generate_textures`](Self::generate_textures) with explicit sampling.
    /// A glyph atlas wants `Nearest`: its features are single texels, and
    /// blending them at a fractional scale turns letter spacing into mush.
    pub fn generate_textures_with(
        &self,
        options: &crate::texture::TextureOptions,
    ) -> Vec<Rc<Texture>> {
        let mut ret = Vec::new();

        let len = self.atlases.len();
        for idx in 0..len {
            let tex = self
                .atlases
                .get(idx)
                .unwrap()
                .generate_texture_with(options);
            ret.push(Rc::new(tex));
        }

        ret
    }

    fn reserve_space(&mut self, width: u32, height: u32) {
        let bounds_x = self.current_atlas_pixel_x + width;

        if bounds_x >= self.pixel_width {
            // Jump to another row
            self.start_new_row(width, height);
        }

        let bounds_y = self.current_atlas_pixel_y + height;
        if bounds_y >= self.pixel_height {
            panic!(
                "need to start a new atlas here (ran out of space trying to reserve space for width: {} height: {}, bounds_x: {} bounds_y: {})...",
                width, height, bounds_x, bounds_y
            );
        }
    }

    fn start_new_row(&mut self, _width: u32, height: u32) {
        self.current_atlas_pixel_y += self.current_row_max_height;
        self.current_atlas_pixel_x = 0;
        self.current_row_max_height = height;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    fn glyph(width: u32, height: u32) -> ImageBuffer<Rgba<u8>, Vec<u8>> {
        ImageBuffer::from_pixel(width, height, Rgba([255, 255, 255, 255]))
    }

    /// The returned UVs address the image, not the padded cell, and the next
    /// pack starts past the padding - so two glyphs never share an edge texel.
    #[test]
    fn padding_offsets_the_uvs_and_separates_consecutive_cells() {
        let mut packer = TexturePacker::<Rgba<u8>>::new_rgba(64, 64);
        let first = packer.pack_padded(&glyph(4, 8), 1);
        let second = packer.pack_padded(&glyph(4, 8), 1);

        // The image sits one texel in from the cell it was given.
        assert_eq!(first.uv_offset_x, 1.0 / 64.0);
        assert_eq!(first.uv_width, 4.0 / 64.0);
        // 1 pad + 4 image + 1 pad = 6 texels before the next image's pad.
        assert_eq!(second.uv_offset_x, 7.0 / 64.0);
        // A full empty texel separates the two images.
        let first_right = first.uv_offset_x + first.uv_width;
        assert_eq!(second.uv_offset_x - first_right, 2.0 / 64.0);
    }

    /// Zero padding is the old behaviour exactly, so the atlas's other users
    /// (lightmaps, cell textures) are untouched.
    #[test]
    fn zero_padding_packs_flush_as_before() {
        let mut packer = TexturePacker::<Rgba<u8>>::new_rgba(64, 64);
        let first = packer.pack(&glyph(4, 8));
        let second = packer.pack(&glyph(4, 8));

        assert_eq!(first.uv_offset_x, 0.0);
        assert_eq!(second.uv_offset_x, 4.0 / 64.0);
    }
}
