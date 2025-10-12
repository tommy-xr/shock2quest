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

    pub fn generate_texture(&self) -> Texture {
        let width = self.img.width();
        let height = self.img.height();
        let data = self.img.clone().into_raw();
        crate::texture::init_from_memory(RawTextureData {
            bytes: data,
            width,
            height,
            format: self.format,
        })
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
        let width = img.width();
        let height = img.height();

        self.reserve_space(width, height);

        let current_image = self.atlases.get_mut(0).unwrap();
        // Copy image into atlas at current position
        for x in 0..width {
            for y in 0..height {
                let pixel = img.get_pixel(x, y);
                current_image.set_pixel(
                    self.current_atlas_pixel_x + x,
                    self.current_atlas_pixel_y + y,
                    *pixel,
                );
            }
        }

        let uv_offset_x = self.current_atlas_pixel_x as f32 / self.pixel_width as f32;
        let uv_offset_y = self.current_atlas_pixel_y as f32 / self.pixel_height as f32;
        let uv_width = width as f32 / self.pixel_width as f32;
        let uv_height = height as f32 / self.pixel_height as f32;

        // Update cursor
        self.current_atlas_pixel_x += width;
        self.current_row_max_height = self.current_row_max_height.max(height);

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
        let mut ret = Vec::new();

        let len = self.atlases.len();
        for idx in 0..len {
            let tex = self.atlases.get(idx).unwrap().generate_texture();
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
            panic!("need to start a new atlas here (ran out of space trying to reserve space for width: {} height: {}, bounds_x: {} bounds_y: {})...", width, height, bounds_x, bounds_y);
        }
    }

    fn start_new_row(&mut self, _width: u32, height: u32) {
        self.current_atlas_pixel_y += self.current_row_max_height;
        self.current_atlas_pixel_x = 0;
        self.current_row_max_height = height;
    }
}
