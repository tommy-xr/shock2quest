use tracing::trace;

#[derive(Clone, Copy)]
pub enum PixelFormat {
    RGB,
    RGBA,
}

#[derive(Clone)]
pub struct RawTextureData {
    pub bytes: std::vec::Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub format: PixelFormat,
}

pub trait TextureFormat {
    fn load(&self, buffer: &[u8]) -> RawTextureData;
}

pub struct FormatUsingImageCrate {
    image_format: image::ImageFormat,
}

impl TextureFormat for FormatUsingImageCrate {
    fn load(&self, buffer: &[u8]) -> RawTextureData {
        let img = image::load_from_memory_with_format(buffer, self.image_format)
            .expect("Failed to load texture");
        let mut data = img.to_rgba8().into_raw();
        apply_color_key(&mut data, img.width(), img.height());

        RawTextureData {
            bytes: data,
            width: img.width(),
            height: img.height(),
            format: PixelFormat::RGBA,
        }
    }
}

fn apply_color_key(pixels: &mut [u8], width: u32, height: u32) {
    for x in 0..width {
        for y in 0..height {
            let pos = (((y * width) + x) * 4u32) as usize;

            let r = pixels[pos];
            let g = pixels[pos + 1usize];
            let b = pixels[pos + 2usize];
            let _a = pixels[pos + 3usize];

            if r > 250 && g < 5 && b > 250 || r < 5 && g > 250 && b > 250 {
                pixels[pos] = 0;
                pixels[pos + 1usize] = 0;
                pixels[pos + 2usize] = 0;
                pixels[pos + 3usize] = 0;
            }
        }
    }
}

/// GL-free PCX dimension read. Reuses the same `pcx::Reader` header parse as the full
/// decode in `PcxFormat::load`, so `(width, height)` are guaranteed to match
/// `Texture::width()/height()` — but WITHOUT decoding pixels or touching the GPU. This
/// lets the level parse compute texture dimensions on a worker thread (no GL context).
pub fn read_pcx_dimensions(buffer: &[u8]) -> Option<(u32, u32)> {
    let pcx = pcx::Reader::new(buffer).ok()?;
    Some((pcx.width() as u32, pcx.height() as u32))
}

pub struct PcxFormat {}

impl TextureFormat for PcxFormat {
    fn load(&self, buffer: &[u8]) -> RawTextureData {
        let mut pcx = pcx::Reader::new(buffer).unwrap();
        let width = pcx.width() as u32;
        let height = pcx.height() as u32;
        trace!(
            "width = {}, height = {}, paletted = {}",
            width,
            height,
            pcx.is_paletted()
        );

        let size = width * height * 4u32;
        trace!("size: {size}");
        let mut data: Vec<u8> = vec![0; size as usize];

        let mut image = Vec::new();
        for _ in 0..pcx.height() {
            let mut row: Vec<u8> = std::iter::repeat(0).take(pcx.width() as usize).collect();
            pcx.next_row_paletted(&mut row).unwrap();
            image.push(row);
        }

        trace!("!! reading palette...");
        let mut palette = vec![0; (256 * 3) as usize];
        pcx.read_palette(&mut palette).unwrap();

        trace!("!! reading img...");
        for y in 0..height {
            for x in 0..width {
                let i = image[y as usize][x as usize] as usize;
                let pcx_r = palette[i * 3];
                let pcx_g = palette[i * 3 + 1];
                let pcx_b = palette[i * 3 + 2];

                let idx = (((y * width) + x) * 4) as usize;
                data[idx] = pcx_r;
                data[idx + 1] = pcx_g;
                data[idx + 2] = pcx_b;
                data[idx + 3] = 255;
            }
        }

        apply_color_key(&mut data, width, height);

        RawTextureData {
            bytes: data,
            width,
            height,
            format: PixelFormat::RGBA,
        }
    }
}

pub const PNG: FormatUsingImageCrate = FormatUsingImageCrate {
    image_format: image::ImageFormat::Png,
};
pub const JPEG: FormatUsingImageCrate = FormatUsingImageCrate {
    image_format: image::ImageFormat::Jpeg,
};
pub const GIF: FormatUsingImageCrate = FormatUsingImageCrate {
    image_format: image::ImageFormat::Gif,
};
pub const PCX: PcxFormat = PcxFormat {};

pub fn extension_to_format(str: String) -> Option<Box<dyn TextureFormat>> {
    let lowercase_str = str.to_ascii_lowercase();
    match lowercase_str.as_str() {
        "pcx" => Some(Box::new(PCX)),
        "png" => Some(Box::new(PNG)),
        "gif" => Some(Box::new(GIF)),
        "jpeg" => Some(Box::new(JPEG)),
        "jpg" => Some(Box::new(JPEG)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_pcx_dimensions_matches_written_size() {
        // Round-trip: write a PCX of a known size, then read just its dimensions back.
        let (w, h): (u16, u16) = (7, 5);
        let mut buf: Vec<u8> = Vec::new();
        {
            let mut writer = pcx::WriterRgb::new(&mut buf, (w, h), (96, 96)).unwrap();
            let row = vec![0u8; (w as usize) * 3];
            for _ in 0..h {
                writer.write_row(&row).unwrap();
            }
            writer.finish().unwrap();
        }
        assert_eq!(read_pcx_dimensions(&buf), Some((w as u32, h as u32)));
    }

    #[test]
    fn read_pcx_dimensions_rejects_non_pcx() {
        assert_eq!(read_pcx_dimensions(&[0u8; 4]), None);
        assert_eq!(read_pcx_dimensions(&[]), None);
    }
}
