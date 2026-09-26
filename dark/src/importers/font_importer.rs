use std::io::Read;

use engine::assets::{asset_cache::AssetCache, asset_importer::AssetImporter};
use once_cell::sync::Lazy;

use crate::font::{Font, FontPalette, read_font_palette};

/// The shared UI font palette an antialiased `.FON` indexes into.
const FONT_PALETTE: &str = "fontpal.pcx";

pub static FONT_IMPORTER: Lazy<AssetImporter<Box<dyn engine::Font>, Box<dyn engine::Font>, ()>> =
    Lazy::new(|| AssetImporter::define(load_font, |font, _cache, _config| font));

pub static TINTED_FONT_IMPORTER: Lazy<
    AssetImporter<Box<dyn engine::Font>, Box<dyn engine::Font>, [u8; 3]>,
> = Lazy::new(|| {
    AssetImporter::define(
        |_name, reader, cache, tint| {
            Box::new(Font::read_paletted(
                reader,
                font_palette(cache).as_ref(),
                *tint,
            )) as Box<dyn engine::Font>
        },
        |font, _cache, _config| font,
    )
});

/// The shipped font palette, or `None` for a data install without it (the
/// glyph bytes then stay coverage against the caller's tint, as they did
/// before the palette was understood).
fn font_palette(assets: &AssetCache) -> Option<FontPalette> {
    let reader = assets.get_raw_reader(FONT_PALETTE)?;
    let mut bytes = Vec::new();
    reader.borrow_mut().read_to_end(&mut bytes).ok()?;
    read_font_palette(&bytes)
}

fn load_font(
    _name: String,
    reader: &mut Box<dyn engine::assets::asset_paths::ReadableAndSeekable>,
    assets: &mut AssetCache,
    _config: &(),
) -> Box<dyn engine::Font> {
    Box::new(Font::read_paletted(
        reader,
        font_palette(assets).as_ref(),
        [255, 255, 255],
    ))
}
