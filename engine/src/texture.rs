use crate::texture_format;
use crate::texture_format::RawTextureData;
use crate::texture_format::TextureFormat;
use crate::EngineRenderContext;
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

#[derive(Hash)]
pub struct TextureOptions {
    pub wrap: bool,
}

impl Default for TextureOptions {
    fn default() -> TextureOptions {
        TextureOptions { wrap: true }
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
        gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MIN_FILTER, gl::LINEAR as i32);
        gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MAG_FILTER, gl::LINEAR as i32);
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
        raw_texture_data.width, raw_texture_data.height, pixel_size_in_bytes, raw_texture_data.bytes.len()
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
        //gl::GenerateMipmap(gl::TEXTURE_2D);
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

pub fn init2(buffer: &std::vec::Vec<u8>, format: &Box<dyn TextureFormat>) -> Texture {
    // TODO:
    //let img = image::load_from_memory_with_format(&buffer, format).expect("Failed to load texture");

    let raw_texture_data = format.load(buffer);
    init_from_memory(raw_texture_data)
}
