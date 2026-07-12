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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::texture::TextureTrait;
    use std::rc::Rc;

    /// Fixed-metrics stub font: every glyph is `advance` wide at `base_height`.
    struct StubFont {
        advance: f32,
        base_height: f32,
    }

    impl Font for StubFont {
        fn get_texture(&self) -> Rc<dyn TextureTrait> {
            unreachable!("measure_text_width does not touch the texture")
        }
        fn get_character_info(&self, _c: char) -> Option<FontCharacterInfo> {
            Some(FontCharacterInfo {
                min_uv_x: 0.0,
                min_uv_y: 0.0,
                max_uv_x: 0.0,
                max_uv_y: 0.0,
                advance: self.advance,
            })
        }
        fn base_height(&self) -> f32 {
            self.base_height
        }
        fn get_half_pixel(&self) -> f32 {
            0.0
        }
    }

    #[test]
    fn measure_scales_with_font_size_and_counts_gaps() {
        // base_height 10, advance 4. At native size (10) multiplier = 1:
        // 3 glyphs => 3*4 advance + 2 inter-glyph gaps of 1 = 14.
        let f = StubFont {
            advance: 4.0,
            base_height: 10.0,
        };
        assert_eq!(measure_text_width(&f, "abc", 10.0), 14.0);
        // Single glyph: no inter-glyph gap.
        assert_eq!(measure_text_width(&f, "a", 10.0), 4.0);
        // Empty string: zero width.
        assert_eq!(measure_text_width(&f, "", 10.0), 0.0);
        // Doubling font_size doubles both advances and the gaps (multiplier 2):
        // 3*8 + 2*2 = 28.
        assert_eq!(measure_text_width(&f, "abc", 20.0), 28.0);
    }
}
