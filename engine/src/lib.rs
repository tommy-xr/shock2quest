pub mod assets;
pub mod audio;
mod engine;
pub mod file_system;
mod font;
mod gl_engine;
pub mod importers;
pub mod logging;
pub mod macros;
pub mod materials;
pub mod scene;
mod shader;
mod shader_program;
pub mod texture;
pub mod texture_atlas;
pub mod texture_descriptor;
pub mod texture_format;
pub mod util;

pub use crate::engine::Engine;
pub use crate::engine::EngineRenderContext;
pub use crate::font::{Font, FontCharacterInfo};

pub fn opengl() -> Box<dyn Engine> {
    let engine = gl_engine::init_gl();
    Box::new(engine)
}

pub fn opengles() -> Box<dyn Engine> {
    let engine = gl_engine::init_gles();
    Box::new(engine)
}

#[cfg(target_os = "android")]
pub fn android() -> Box<dyn Engine> {
    let engine = gl_engine::init_android();
    Box::new(engine)
}
