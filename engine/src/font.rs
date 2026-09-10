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
/// would occupy for `text`. Mirrors that renderer's layout: the sum of glyph
/// advances, nothing added between glyphs (side bearings are baked into the
/// `.FON` glyph cells), so callers can align/center text before drawing it.
pub fn measure_text_width(font: &dyn Font, text: &str, font_size: f32) -> f32 {
    let multiplier = font_size / font.base_height();
    text.chars()
        .filter_map(|c| font.get_character_info(c))
        .map(|info| info.advance * multiplier)
        .sum()
}

/// Wrap without discarding text, using the same advances as rendering.
/// Paragraph gaps are retained; an overlong word continues on the next line.
pub fn wrap_text_to_width(font: &dyn Font, text: &str, font_size: f32, width: f32) -> Vec<String> {
    let mut lines = Vec::new();
    for paragraph in text.split('\n') {
        let mut line = String::new();
        for word in paragraph.split_whitespace() {
            let candidate = if line.is_empty() {
                word.to_owned()
            } else {
                format!("{line} {word}")
            };
            if measure_text_width(font, &candidate, font_size) <= width {
                line = candidate;
                continue;
            }
            if !line.is_empty() {
                lines.push(std::mem::take(&mut line));
            }
            for ch in word.chars() {
                let candidate = format!("{line}{ch}");
                if !line.is_empty() && measure_text_width(font, &candidate, font_size) > width {
                    lines.push(std::mem::take(&mut line));
                }
                line.push(ch);
            }
        }
        lines.push(line);
    }
    lines
}

/// Shorten `text` until it fits `max_width`, marking the cut with a trailing
/// ellipsis. Returns `text` unchanged when it already fits.
///
/// Used for text of unbounded length - save names, player-authored labels -
/// which would otherwise spill out of its widget and over its neighbours.
/// Measured with real glyph advances, so it is correct for the variable-width
/// bitmap fonts Dark ships.
pub fn ellipsize(font: &dyn Font, text: &str, font_size: f32, max_width: f32) -> String {
    if measure_text_width(font, text, font_size) <= max_width {
        return text.to_owned();
    }

    const ELLIPSIS: &str = "...";
    let ellipsis_width = measure_text_width(font, ELLIPSIS, font_size);
    // Too narrow to say anything meaningful - an empty string beats a stray
    // fragment of an ellipsis drawn over the neighbouring widget.
    if ellipsis_width > max_width {
        return String::new();
    }

    let budget = max_width - ellipsis_width;
    let multiplier = font_size / font.base_height();
    let mut width = 0.0;
    let mut end = 0;
    for (index, c) in text.char_indices() {
        let advance = font
            .get_character_info(c)
            .map(|info| info.advance * multiplier)
            .unwrap_or(0.0);
        if width + advance > budget {
            break;
        }
        width += advance;
        // `index` is the char's start, so the kept prefix ends after it.
        end = index + c.len_utf8();
    }
    format!("{}{}", &text[..end], ELLIPSIS)
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
    fn wrapping_keeps_paragraphs_and_every_glyph_within_width() {
        let font = StubFont {
            advance: 4.0,
            base_height: 10.0,
        };
        let input = "one two\n\nlongwordhere 25%";
        let lines = wrap_text_to_width(&font, input, 10.0, 20.0);
        assert!(lines.iter().any(String::is_empty));
        assert!(
            lines
                .iter()
                .all(|line| measure_text_width(&font, line, 10.0) <= 20.0)
        );
        assert_eq!(
            lines.concat().replace(' ', ""),
            input.split_whitespace().collect::<String>()
        );
        assert!(!lines.iter().any(|line| line.contains("...")));
    }

    #[test]
    fn measure_is_sum_of_advances_with_no_added_spacing() {
        // The Dark engine's advance is exactly the offset-table column
        // difference - side bearings are baked into the glyph cells, nothing is
        // added between glyphs. base_height 10, advance 4, native size
        // (multiplier = 1): 3 glyphs => 3*4 = 12.
        let f = StubFont {
            advance: 4.0,
            base_height: 10.0,
        };
        assert_eq!(measure_text_width(&f, "abc", 10.0), 12.0);
        // Single glyph: its advance.
        assert_eq!(measure_text_width(&f, "a", 10.0), 4.0);
        // Empty string: zero width.
        assert_eq!(measure_text_width(&f, "", 10.0), 0.0);
        // Doubling font_size doubles the advances (multiplier 2): 3*8 = 24.
        assert_eq!(measure_text_width(&f, "abc", 20.0), 24.0);
    }

    #[test]
    fn ellipsize_leaves_text_that_already_fits() {
        // 10px per glyph: "abc" is 30px in a 100px budget.
        let font = StubFont {
            advance: 10.0,
            base_height: 10.0,
        };
        assert_eq!(ellipsize(&font, "abc", 10.0, 100.0), "abc");
        // Exactly filling the width is still a fit.
        assert_eq!(ellipsize(&font, "abc", 10.0, 30.0), "abc");
    }

    #[test]
    fn ellipsize_shortens_text_that_overflows() {
        let font = StubFont {
            advance: 10.0,
            base_height: 10.0,
        };
        // 60px budget - 30px of "..." leaves room for 3 glyphs.
        let out = ellipsize(&font, "abcdefgh", 10.0, 60.0);
        assert_eq!(out, "abc...");
        // The result must actually fit, which is the whole point.
        assert!(measure_text_width(&font, &out, 10.0) <= 60.0);
    }

    #[test]
    fn ellipsize_gives_up_when_even_the_ellipsis_does_not_fit() {
        let font = StubFont {
            advance: 10.0,
            base_height: 10.0,
        };
        // A stray fragment drawn over the neighbouring widget is worse than
        // nothing.
        assert_eq!(ellipsize(&font, "abcdef", 10.0, 20.0), "");
    }

    #[test]
    fn ellipsize_cuts_on_character_boundaries() {
        let font = StubFont {
            advance: 10.0,
            base_height: 10.0,
        };
        // Multi-byte characters must not be sliced mid-codepoint (that would
        // panic on the string index).
        let out = ellipsize(&font, "\u{e4}\u{f6}\u{fc}\u{df}\u{e4}\u{f6}", 10.0, 50.0);
        assert_eq!(out, "\u{e4}\u{f6}...");
    }

    #[test]
    fn ellipsize_scales_with_font_size() {
        let font = StubFont {
            advance: 10.0,
            base_height: 10.0,
        };
        // At double size each glyph is 20px, so half as many survive.
        assert_eq!(ellipsize(&font, "abcdefgh", 20.0, 120.0), "abc...");
    }
}
