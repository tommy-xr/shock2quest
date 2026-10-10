//! Opt-in terrain art: resolve pixels and logical UV dimensions together.
//! `original_fam` is mounted only by the upgraded-terrain asset stack. Without
//! it, classic installs and flag-off runs retain their exact legacy lookup.
use std::{io::Read, path::Path};

use engine::{assets::asset_paths::AbstractAssetPath, texture::PlaybackMode};

#[derive(Clone, Debug, PartialEq)]
pub struct TerrainAnimation {
    pub frames: Vec<String>,
    pub frame_ms: u32,
    pub playback: PlaybackMode,
}

#[derive(Debug, PartialEq)]
pub struct TerrainTexture {
    pub name: String,
    pub dimensions: Option<(u32, u32)>,
    pub animation: Option<TerrainAnimation>,
}

pub fn resolve(
    paths: &dyn AbstractAssetPath,
    base: &str,
    requested: &str,
    allow_static_upgrade: bool,
) -> TerrainTexture {
    let requested = requested.to_ascii_lowercase();
    let original = format!("original_fam/{requested}");
    let enabled = paths.exists(base.to_owned(), original.clone());
    let original = if enabled { original } else { requested.clone() };
    let dimensions = read(paths, base, &original)
        .and_then(|bytes| engine::texture_format::read_pcx_dimensions(&bytes));
    let fallback = TerrainTexture {
        name: original,
        dimensions,
        animation: None,
    };
    if !enabled {
        return fallback;
    }

    let stem = Path::new(&requested).with_extension("");
    let stem = format!("fam/{}", stem.to_string_lossy());
    let material = format!("{stem}.mtl");
    let mut logical = dimensions;
    let mut texture = stem.clone();
    if let Some(bytes) = read(paths, base, &material) {
        let Ok(source) = String::from_utf8(bytes) else {
            return fallback;
        };
        let Some(script) =
            super::material_includes::expand_material_includes(&material, &source, |name| {
                String::from_utf8(read(paths, base, name)?).ok()
            })
        else {
            return fallback;
        };
        if let Some(animation) = resolve_animation(paths, base, &script) {
            let Ok(Some(size)) = material_dimensions(&script, dimensions) else {
                return fallback;
            };
            return TerrainTexture {
                name: animation.frames[0].clone(),
                dimensions: Some(size),
                animation: Some(animation),
            };
        }
        if !allow_static_upgrade {
            return fallback;
        }
        match logical_dimensions(&script, dimensions) {
            Ok(Some(size)) => logical = Some(size),
            Ok(None) => {}
            Err(()) => return fallback,
        }
        if let Some(redirect) = super::primary_render_pass_texture(&script) {
            // The existing diffuse-pass parser strips the FAM prefix.
            texture = format!("fam/{}", redirect.replace('\\', "/").to_ascii_lowercase());
        }
    }
    if !allow_static_upgrade {
        return fallback;
    }
    // Never substitute an image whose world-space footprint is unknown.
    if logical.is_none() {
        return fallback;
    }
    let texture = Path::new(&texture).with_extension("");
    let candidates = engine::texture_format::DECODABLE_EXTENSIONS
        .iter()
        .map(|ext| format!("{}.{ext}", texture.to_string_lossy()))
        .collect::<Vec<_>>();
    match paths.resolve_first(base.to_owned(), &candidates) {
        Some(name) => {
            tracing::debug!(
                "terrain {requested}: {name}, logical {logical:?}, original {dimensions:?}"
            );
            TerrainTexture {
                name,
                dimensions: logical,
                animation: None,
            }
        }
        None => fallback,
    }
}

fn resolve_animation(
    paths: &dyn AbstractAssetPath,
    base: &str,
    script: &str,
) -> Option<TerrainAnimation> {
    use engine::scene::render_pass::BlendFactor;
    let plan = super::render_material::parse(script).ok()?;
    if !plan.material_only || plan.passes.len() != 1 {
        return None;
    }
    let pass = &plan.passes[0];
    // A single ordinary shaded pass can use the existing world shader.
    if !pass.shaded
        || pass.environment
        || pass.incidence.is_some()
        || pass.clamp
        || pass.mipmap_bias != 0.0
        || pass.alpha != 1.0
        || pass.color != cgmath::vec3(1.0, 1.0, 1.0)
        || !matches!(
            pass.blend,
            (BlendFactor::SrcAlpha, BlendFactor::InvSrcAlpha)
                | (BlendFactor::One, BlendFactor::Zero)
        )
    {
        return None;
    }
    let animation = pass.animation.as_ref()?;
    let prefix = animation.prefix.to_ascii_lowercase();
    if !prefix.starts_with("fam/") {
        return None;
    }
    let frames = (0..animation.count)
        .map(|frame| {
            // NewDark's *_ form uses the unnumbered prefix as frame zero.
            let name = if frame == 0 {
                prefix.clone()
            } else {
                format!(
                    "{prefix}{}{frame}",
                    if prefix.ends_with('_') { "" } else { "_" }
                )
            };
            let candidates = engine::texture_format::DECODABLE_EXTENSIONS
                .iter()
                .map(|ext| format!("{name}.{ext}"))
                .collect::<Vec<_>>();
            paths.resolve_first(base.to_owned(), &candidates)
        })
        .collect::<Option<Vec<_>>>()?;
    Some(TerrainAnimation {
        frames,
        frame_ms: pass.frame_ms,
        playback: pass.playback,
    })
}

fn read(paths: &dyn AbstractAssetPath, base: &str, name: &str) -> Option<Vec<u8>> {
    if !paths.exists(base.to_owned(), name.to_owned()) {
        return None;
    }
    let reader = paths.get_reader(base.to_owned(), name.to_owned())?;
    let mut bytes = Vec::new();
    reader.borrow_mut().read_to_end(&mut bytes).ok()?;
    Some(bytes)
}

fn logical_dimensions(
    script: &str,
    original: Option<(u32, u32)>,
) -> Result<Option<(u32, u32)>, ()> {
    // These materials need UV/animation execution, not just a different image.
    // Keep their original art until that separate material-effects work lands.
    if script.lines().any(|line| {
        line.split_whitespace().next().is_some_and(|key| {
            matches!(
                key.to_ascii_lowercase().as_str(),
                "uv_mod" | "ani_frames" | "ani_rate"
            )
        })
    }) {
        return Err(());
    }
    material_dimensions(script, original)
}

fn material_dimensions(
    script: &str,
    original: Option<(u32, u32)>,
) -> Result<Option<(u32, u32)>, ()> {
    let size = dimension_directive(script, "terrain_scale")?.or(original);
    let tiles = dimension_directive(script, "tile_factor")?.unwrap_or((1, 1));
    size.map(|(w, h)| {
        // One replacement image can contain several original repeats, e.g.
        // MedSci's SqrTilWht1 is an 8x8 patch of varied floor tiles.
        let w = w.checked_mul(tiles.0).filter(|n| *n <= 16384).ok_or(())?;
        let h = h.checked_mul(tiles.1).filter(|n| *n <= 16384).ok_or(())?;
        Ok((w, h))
    })
    .transpose()
}

fn dimension_directive(script: &str, directive: &str) -> Result<Option<(u32, u32)>, ()> {
    let mut size = None;
    for line in script.lines() {
        let code = line.split_once("//").map_or(line, |(code, _)| code);
        let fields = code.split_whitespace().collect::<Vec<_>>();
        if !fields
            .first()
            .is_some_and(|word| word.eq_ignore_ascii_case(directive))
        {
            continue;
        }
        let dimension = |word: &str| {
            word.parse::<u32>()
                .ok()
                .filter(|n| *n > 0 && *n <= 16384)
                .ok_or(())
        };
        size = Some(match fields.as_slice() {
            [_, square] => {
                let n = dimension(square)?;
                (n, n)
            }
            [_, width, height] => (dimension(width)?, dimension(height)?),
            _ => return Err(()),
        });
    }
    Ok(size)
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine::assets::asset_paths::ReadableAndSeekable;
    use std::{cell::RefCell, collections::HashMap, io::Cursor};

    struct Files(HashMap<String, Vec<u8>>);
    impl AbstractAssetPath for Files {
        fn exists(&self, _: String, name: String) -> bool {
            self.0.contains_key(&name)
        }
        fn get_reader(
            &self,
            _: String,
            name: String,
        ) -> Option<RefCell<Box<dyn ReadableAndSeekable>>> {
            Some(RefCell::new(Box::new(Cursor::new(
                self.0.get(&name)?.clone(),
            ))))
        }
    }

    fn fixture() -> Files {
        let mut pcx = Vec::new();
        let mut writer = pcx::WriterRgb::new(&mut pcx, (64, 32), (96, 96)).unwrap();
        for _ in 0..32 {
            writer.write_row(&[0; 64 * 3]).unwrap();
        }
        writer.finish().unwrap();
        Files(HashMap::from([
            ("original_fam/med/wall.pcx".into(), pcx.clone()),
            ("med/wall.pcx".into(), pcx),
            (
                "fam/med/wall.mtl".into(),
                b"include ../shared/wall.inc".to_vec(),
            ),
            (
                "fam/shared/wall.inc".into(),
                b"terrain_scale 128 64\nrender_pass {\ntexture FAM\\shared\\new_wall\nshaded 1\n}"
                    .to_vec(),
            ),
            ("fam/shared/new_wall.dds".into(), vec![]),
        ]))
    }

    #[test]
    fn included_redirect_and_logical_dimensions_are_resolved_together() {
        let files = fixture();
        assert_eq!(
            resolve(&files, "", "MED/WALL.PCX", true),
            TerrainTexture {
                name: "fam/shared/new_wall.dds".into(),
                dimensions: Some((128, 64)),
                animation: None,
            }
        );
        // Existing animations opt out as a whole, including their dimensions.
        assert_eq!(
            resolve(&files, "", "med/wall.pcx", false),
            TerrainTexture {
                name: "original_fam/med/wall.pcx".into(),
                dimensions: Some((64, 32)),
                animation: None,
            }
        );
    }

    #[test]
    fn incomplete_or_invalid_materials_fall_back_with_original_dimensions() {
        for missing in ["fam/shared/new_wall.dds", "fam/shared/wall.inc"] {
            let mut files = fixture();
            files.0.remove(missing);
            let resolved = resolve(&files, "", "med/wall.pcx", true);
            assert_eq!(resolved.name, "original_fam/med/wall.pcx");
            assert_eq!(resolved.dimensions, Some((64, 32)));
        }
        let mut files = fixture();
        files
            .0
            .insert("fam/med/wall.mtl".into(), b"terrain_scale 0".to_vec());
        assert_eq!(
            resolve(&files, "", "med/wall.pcx", true).dimensions,
            Some((64, 32))
        );
    }

    #[test]
    fn flag_off_keeps_legacy_lookup_and_plain_replacements_keep_original_size() {
        let mut files = fixture();
        files.0.remove("original_fam/med/wall.pcx");
        assert_eq!(
            resolve(&files, "", "med/wall.pcx", true).name,
            "med/wall.pcx"
        );
        let mut files = fixture();
        files.0.remove("fam/med/wall.mtl");
        files.0.insert("fam/med/wall.png".into(), vec![]);
        assert_eq!(
            resolve(&files, "", "med/wall.pcx", true),
            TerrainTexture {
                name: "fam/med/wall.png".into(),
                dimensions: Some((64, 32)),
                animation: None,
            }
        );
    }

    #[test]
    fn numbered_animation_is_complete_and_can_replace_legacy_animation() {
        let mut files = fixture();
        files.0.insert("fam/med/wall.mtl".into(), b"terrain_scale 64\nrender_material_only 1\nrender_pass {\nani_mode PINGPONG\nani_rate 164\ntexture *_ 3 FAM/med/pulse_\nshaded 1\n}".to_vec());
        for name in ["pulse_", "pulse_1", "pulse_2"] {
            files.0.insert(format!("fam/med/{name}.dds"), vec![]);
        }
        let resolved = resolve(&files, "", "med/wall.pcx", false);
        let animation = resolved.animation.unwrap();
        assert_eq!(
            animation.frames,
            [
                "fam/med/pulse_.dds",
                "fam/med/pulse_1.dds",
                "fam/med/pulse_2.dds"
            ]
        );
        assert_eq!(animation.frame_ms, 164);
        assert_eq!(animation.playback, PlaybackMode::PingPong);
        assert_eq!(resolved.dimensions, Some((64, 64)));
        files.0.remove("fam/med/pulse_2.dds");
        let fallback = resolve(&files, "", "med/wall.pcx", false);
        assert_eq!(fallback.name, "original_fam/med/wall.pcx");
        assert_eq!(fallback.dimensions, Some((64, 32)));
        assert!(fallback.animation.is_none());
    }

    #[test]
    fn varied_tile_patches_preserve_the_original_tile_footprint() {
        // MedSci MEDSC006 -> SqrTilWht1: 32px original, eight authored
        // tiles across the replacement. Sampling at 1/8 UVs keeps tile size.
        assert_eq!(
            logical_dimensions("tile_factor 8", Some((32, 32))),
            Ok(Some((256, 256)))
        );
        assert_eq!(
            logical_dimensions("tile_factor 2 4", Some((64, 32))),
            Ok(Some((128, 128)))
        );
        assert_eq!(
            logical_dimensions("terrain_scale 64 32\ntile_factor 2", Some((16, 16))),
            Ok(Some((128, 64)))
        );
        for source in [
            "tile_factor 0",
            "tile_factor NaN",
            "tile_factor 16384",
            "uv_mod SCALE 2 2",
            "ani_frames 4",
        ] {
            assert!(logical_dimensions(source, Some((32, 32))).is_err());
        }
        let mut files = fixture();
        files.0.insert(
            "fam/shared/wall.inc".into(),
            b"tile_factor 8\nrender_pass {\ntexture FAM/shared/new_wall\n}".to_vec(),
        );
        assert_eq!(
            resolve(&files, "", "med/wall.pcx", true).dimensions,
            Some((512, 256))
        );
    }

    #[test]
    fn logical_dimensions_support_square_rectangular_and_overrides() {
        assert_eq!(
            logical_dimensions("terrain_scale 64", None),
            Ok(Some((64, 64)))
        );
        assert_eq!(
            logical_dimensions("terrain_scale 64\nTERRAIN_SCALE 128 32 // wall", None),
            Ok(Some((128, 32)))
        );
        for invalid in ["0", "-1", "NaN", "64 0", "1 2 3", "999999999999"] {
            assert!(logical_dimensions(&format!("terrain_scale {invalid}"), None).is_err());
        }
        assert_eq!(
            logical_dimensions("// terrain_scale 1\nui_scale 4", None),
            Ok(None)
        );
    }
}
