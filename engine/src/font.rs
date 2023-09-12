use std::rc::Rc;

use crate::{
    texture::{Texture, TextureTrait},
    texture_descriptor::TextureDescriptor,
};

pub struct FontCharacterInfo {
    pub min_uv_x: f32,
    pub min_uv_y: f32,
    pub max_uv_x: f32,
    pub max_uv_y: f32,
    pub advance: f32,
}

pub trait Font {
    fn get_texture(&self) -> Rc<dyn TextureTrait>;

    fn get_character_info(&self, c: char) -> Option<FontCharacterInfo>;

    fn base_height(&self) -> f32;
}
