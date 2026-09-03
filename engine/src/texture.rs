use crate::EngineRenderContext;
use crate::texture_format;
use crate::texture_format::RawTextureData;
use crate::texture_format::TextureFormat;
use gl::types;

use std::os::raw::c_void;
use std::rc::Rc;
use std::time::Duration;

pub struct Texture {
    gl_id: types::GLuint,
    width: u32,
    height: u32,
}

// Will this cause problems for multi-threading??
unsafe impl Send for Texture {}
unsafe impl Sync for Texture {}

pub trait TextureTrait {
    fn bind0(&self, render_context: &EngineRenderContext);
    fn bind1(&self, render_context: &EngineRenderContext);
}

impl TextureTrait for Texture {
    fn bind0(&self, _render_context: &EngineRenderContext) {
        bind0(self);
    }
    fn bind1(&self, _render_context: &EngineRenderContext) {
        bind1(self);
    }
}

pub struct AnimatedTexture {
    textures: Vec<Rc<Texture>>,
    time_per_frame: f32,
}

impl AnimatedTexture {
    pub fn new(textures: Vec<Rc<Texture>>, duration_per_frame: Duration) -> AnimatedTexture {
        AnimatedTexture {
            textures,
            time_per_frame: duration_per_frame.as_secs_f32(),
        }
    }
}

impl TextureTrait for AnimatedTexture {
    fn bind0(&self, render_context: &EngineRenderContext) {
        let frame = (render_context.time / self.time_per_frame) as usize;
        let frame = frame % self.textures.len();
        bind0(&self.textures[frame]);
    }
    fn bind1(&self, render_context: &EngineRenderContext) {
        let frame = (render_context.time / self.time_per_frame) as usize;
        let frame = frame % self.textures.len();
        bind1(&self.textures[frame]);
    }
}

impl Texture {
    pub fn width(&self) -> u32 {
        self.width
    }
    pub fn height(&self) -> u32 {
        self.height
    }

    /// Replace one RGB rectangle without reallocating the texture. Animated
    /// Dark lightmaps use this for rare switch transitions; the ordinary
    /// render path keeps sampling the same shared texture object.
    pub fn update_rgb_region(&self, x: u32, y: u32, width: u32, height: u32, pixels: &[u8]) {
        assert!(x + width <= self.width);
        assert!(y + height <= self.height);
        assert_eq!(pixels.len(), (width * height * 3) as usize);

        unsafe {
            gl::BindTexture(gl::TEXTURE_2D, self.gl_id);
            // RGB rows are not necessarily four-byte aligned (many Dark
            // lightmaps are odd widths), so override OpenGL's default while
            // this tightly-packed slice is uploaded.
            gl::PixelStorei(gl::UNPACK_ALIGNMENT, 1);
            gl::TexSubImage2D(
                gl::TEXTURE_2D,
                0,
                x as i32,
                y as i32,
                width as i32,
                height as i32,
                gl::RGB,
                gl::UNSIGNED_BYTE,
                pixels.as_ptr() as *const c_void,
            );
            gl::PixelStorei(gl::UNPACK_ALIGNMENT, 4);
        }
    }
}

impl Drop for Texture {
    fn drop(&mut self) {
        unsafe {
            gl::DeleteTextures(1, &self.gl_id);
        }
    }
}

pub fn bind0(texture: &Texture) {
    unsafe {
        gl::ActiveTexture(gl::TEXTURE0);
        gl::BindTexture(gl::TEXTURE_2D, texture.gl_id);
    }
}

pub fn bind1(texture: &Texture) {
    unsafe {
        gl::ActiveTexture(gl::TEXTURE1);
        gl::BindTexture(gl::TEXTURE_2D, texture.gl_id);
    }
}

pub fn bind(texture: &Texture) {
    bind0(texture);
}

/// How a texture is sampled when it does not map 1:1 to pixels.
#[derive(Hash, Clone, Copy, PartialEq, Eq, Debug)]
pub enum TextureFilter {
    /// Blend neighbouring texels. Right for world and scaled-up art.
    Linear,
    /// Snap to the nearest texel. Right for colour-keyed HUD bitmaps drawn
    /// minified: blending across the key boundary turns it into a visible
    /// fringe that no colour-key test can then recognize.
    Nearest,
    /// Linear, with mipmaps for minification. Right for fine line art drawn
    /// smaller than its texels and at a viewpoint-dependent scale (a hologram
    /// grid on a VR panel): plain `Linear` point-samples the lines, so their
    /// brightness swims with sub-pixel phase as the panel moves.
    LinearMipmap,
}

#[derive(Hash)]
pub struct TextureOptions {
    pub wrap: bool,
    /// Treat palette index 0 as transparent when decoding paletted formats
    /// (PCX). Dark bitmap sprites (particles) are keyed this way; wall/UI
    /// textures are not, so this is opt-in.
    pub transparent_index_0: bool,
    /// Turn opaque line art into a translucent tinted overlay: each texel's
    /// alpha becomes its luminance and its colour becomes this tint. Black
    /// therefore drops out entirely, which is what makes a grid bitmap read as
    /// a hologram over the world instead of a black panel.
    pub luminance_alpha_tint: Option<[u8; 3]>,
    pub filter: TextureFilter,
}

impl Default for TextureOptions {
    fn default() -> TextureOptions {
        TextureOptions {
            wrap: true,
            transparent_index_0: false,
            luminance_alpha_tint: None,
            filter: TextureFilter::Linear,
        }
    }
}

pub fn init_from_memory(raw_texture_data: RawTextureData) -> Texture {
    init_from_memory2(raw_texture_data, &TextureOptions::default())
}

pub fn init_from_memory2(raw_texture_data: RawTextureData, options: &TextureOptions) -> Texture {
    let mut texture = 0;
    unsafe {
        gl::GenTextures(1, &mut texture);
        gl::BindTexture(gl::TEXTURE_2D, texture); // all upcoming GL_TEXTURE_2D operations now have effect on this texture object
        // set the texture wrapping parameters

        let wrap = if options.wrap {
            gl::REPEAT
        } else {
            gl::CLAMP_TO_EDGE
        };
        gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_WRAP_S, wrap as i32); // set texture wrapping to gl::REPEAT (default wrapping method)
        gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_WRAP_T, wrap as i32);
        // gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_WRAP_S, gl::CLAMP_TO_EDGE as i32); // set texture wrapping to gl::REPEAT (default wrapping method)
        // gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_WRAP_T, gl::CLAMP_TO_EDGE as i32);

        // set texture filtering parameters
        let (min_filter, mag_filter) = match options.filter {
            TextureFilter::Linear => (gl::LINEAR, gl::LINEAR),
            TextureFilter::Nearest => (gl::NEAREST, gl::NEAREST),
            TextureFilter::LinearMipmap => (gl::LINEAR_MIPMAP_LINEAR, gl::LINEAR),
        };
        gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MIN_FILTER, min_filter as i32);
        gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MAG_FILTER, mag_filter as i32);
    }

    let pixel_format = match raw_texture_data.format {
        texture_format::PixelFormat::RGB => gl::RGB,
        texture_format::PixelFormat::RGBA => gl::RGBA,
    };

    let pixel_size_in_bytes = match raw_texture_data.format {
        texture_format::PixelFormat::RGB => 3,
        texture_format::PixelFormat::RGBA => 4,
    };

    assert!(
        raw_texture_data.bytes.len()
            == (raw_texture_data.width * raw_texture_data.height * pixel_size_in_bytes) as usize,
        "Texture data size does not match width and height - width: {} height: {} pixel_size_in_bytes: {} actual_bytes: {}",
        raw_texture_data.width,
        raw_texture_data.height,
        pixel_size_in_bytes,
        raw_texture_data.bytes.len()
    );

    unsafe {
        gl::TexImage2D(
            gl::TEXTURE_2D,
            0,
            pixel_format as i32,
            raw_texture_data.width as i32,
            raw_texture_data.height as i32,
            0,
            pixel_format,
            gl::UNSIGNED_BYTE,
            &raw_texture_data.bytes[0] as *const u8 as *const c_void,
        );
        if matches!(options.filter, TextureFilter::LinearMipmap) {
            gl::GenerateMipmap(gl::TEXTURE_2D);
        }
    }

    Texture {
        gl_id: texture,
        width: raw_texture_data.width,
        height: raw_texture_data.height,
    }
    // */
    // Texture { gl_id: 0 }
}

pub fn init<T: crate::texture_format::TextureFormat>(
    buffer: &std::vec::Vec<u8>,
    format: T,
) -> Texture {
    // TODO:
    //let img = image::load_from_memory_with_format(&buffer, format).expect("Failed to load texture");

    let raw_texture_data = TextureFormat::load(&format, buffer);
    init_from_memory(raw_texture_data)
}

pub fn init2(buffer: &[u8], format: &dyn TextureFormat) -> Texture {
    // TODO:
    //let img = image::load_from_memory_with_format(&buffer, format).expect("Failed to load texture");

    let raw_texture_data = format.load(buffer);
    init_from_memory(raw_texture_data)
}
