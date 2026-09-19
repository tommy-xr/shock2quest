//! CPU-only copy of the rendered MFD font's advances for pure GUI scripts.
use crate::{
    gui::GuiComponent,
    ui::{HAlign, MFD_FONT, Rect, VAlign},
};
use engine::assets::asset_cache::AssetCache;
use shipyard::{Unique, UniqueView, World};
use std::collections::HashMap;

#[derive(Unique)]
pub(crate) struct PanelText {
    advances: HashMap<char, f32>,
    pub height: f32,
    strings: HashMap<String, HashMap<String, String>>,
}

impl PanelText {
    pub fn load(assets: &mut AssetCache) -> Self {
        let font = crate::ui::resolve_font(assets, MFD_FONT);
        let advances = (0..=u16::MAX)
            .filter_map(|code| {
                let ch = char::from_u32(code as u32)?;
                font.get_character_info(ch).map(|info| (ch, info.advance))
            })
            .collect();
        let height = font.base_height();
        let strings = ["misc", "stathelp", "skilhelp", "research"]
            .into_iter()
            .map(|name| {
                let table = assets
                    .get_opt(&dark::importers::STRINGS_IMPORTER, &format!("{name}.str"))
                    .map(|table| (*table).clone())
                    .unwrap_or_default();
                (name.to_owned(), table)
            })
            .collect();
        Self {
            advances,
            height,
            strings,
        }
    }

    pub fn wrap(world: &World, text: &str, width: f32) -> Vec<String> {
        let metrics = world.borrow::<UniqueView<Self>>();
        engine::wrap_text_with_measure(text, width, |text| {
            text.chars()
                .map(|ch| {
                    metrics
                        .as_ref()
                        .map_or(6.0, |m| m.advances.get(&ch).copied().unwrap_or(0.0))
                })
                .sum()
        })
    }

    pub fn string(world: &World, table: &str, key: &str, fallback: &str) -> String {
        world
            .borrow::<UniqueView<Self>>()
            .ok()
            .and_then(|m| {
                m.strings
                    .get(table)
                    .and_then(|t| t.get(&key.to_ascii_lowercase()))
                    .cloned()
            })
            .unwrap_or_else(|| fallback.to_owned())
    }

    pub fn line_height(world: &World) -> f32 {
        world
            .borrow::<UniqueView<Self>>()
            .map(|m| m.height.max(13.0))
            .unwrap_or(13.0)
    }

    pub fn text<T: Clone>(text: &str, rect: Rect) -> GuiComponent<T> {
        GuiComponent::Text {
            position: cgmath::vec2(rect.x, rect.y),
            size: cgmath::vec2(rect.w, rect.h),
            text: text.to_owned(),
            font: MFD_FONT.to_owned(),
            font_size: 0.0,
            h: HAlign::Left,
            v: VAlign::Top,
            alpha: 1.0,
            fit_to_rect: true,
        }
    }

    pub fn paragraph<T: Clone>(world: &World, text: &str, rect: Rect) -> Vec<GuiComponent<T>> {
        let height = Self::line_height(world);
        Self::wrap(world, text, rect.w)
            .iter()
            .take((rect.h / height) as usize)
            .enumerate()
            .filter(|(_, line)| !line.is_empty())
            .map(|(i, line)| {
                Self::text(
                    line,
                    Rect::new(rect.x, rect.y + i as f32 * height, rect.w, height),
                )
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrapping_uses_glyph_widths_and_keeps_long_words_and_paragraphs() {
        let world = World::new();
        world.add_unique(PanelText {
            advances: HashMap::from([('W', 12.0), ('i', 2.0), (' ', 3.0)]),
            height: 12.0,
            strings: HashMap::new(),
        });
        assert_eq!(PanelText::wrap(&world, "WW iiiiii", 24.0), ["WW", "iiiiii"]);
        assert_eq!(
            PanelText::wrap(&world, "WWW\n\niii", 24.0),
            ["WW", "W", "", "iii"]
        );
    }
}
