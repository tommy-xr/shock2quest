//! The 25AE capture of each mission's surroundings (`env/<mission>.dds`), a
//! cubemap that reflective materials sample.
use engine::{assets::asset_cache::AssetCache, texture::CubeTexture};
use std::{io::Read, path::Path, rc::Rc};

/// `None` on a classic install, for a mission without a capture, or for a
/// debug scene.
pub fn load(assets: &AssetCache, mission: &str) -> Option<Rc<CubeTexture>> {
    let stem = Path::new(mission)
        .file_stem()?
        .to_str()?
        .to_ascii_lowercase();
    let reader = assets.get_raw_reader(&format!("env/{stem}.dds"))?;
    let mut bytes = Vec::new();
    reader.borrow_mut().read_to_end(&mut bytes).ok()?;
    let Some(faces) = engine::dds::decode_cube_rgba8(&bytes) else {
        tracing::warn!("env/{stem}.dds is not a cubemap we can decode");
        return None;
    };
    Some(Rc::new(CubeTexture::new(&faces)))
}
