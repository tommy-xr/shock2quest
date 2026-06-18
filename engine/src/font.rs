use std::rc::Rc;

use crate::texture::TextureTrait;

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

    fn get_half_pixel(&self) -> f32;
}

/// Width, in the same units as `font_size`, that
/// [`SceneObject::screen_space_text`](crate::scene::SceneObject::screen_space_text)
/// would occupy for `text`. Mirrors that renderer's per-glyph advance + inter-glyph
/// spacing so callers can align/center text before drawing it.
pub fn measure_text_width(font: &dyn Font, text: &str, font_size: f32) -> f32 {
    let multiplier = font_size / font.base_height();
    let mut width = 0.0;
    let mut count = 0u32;
    for c in text.chars() {
        if let Some(info) = font.get_character_info(c) {
            width += info.advance * multiplier;
            count += 1;
        }
    }
    // screen_space_text adds one `multiplier` of spacing after each glyph; only the
    // gaps between glyphs contribute to visible width.
    if count > 1 {
        width += multiplier * (count - 1) as f32;
    }
    width
}
