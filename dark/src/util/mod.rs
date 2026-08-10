mod merge_maps;

use cgmath::{InnerSpace, Point3, point3};
use collision::Sphere;
pub use merge_maps::*;

use std::{io::Read, path::Path, rc::Rc};

use engine::{assets::asset_cache::AssetCache, texture::Texture};
use tracing::trace;

use crate::importers::TEXTURE_IMPORTER;

/// Resolve a texture name to one that actually exists, allowing the extension to
/// differ from the one requested.
///
/// A model's material list stores a literal filename (`FOO.PCX`), but a mod layer
/// may ship that texture under a different extension (`FOO.PNG`). Without this,
/// the exact-name lookup misses, the mesh slot is silently dropped, and the prop
/// disappears from the world.
///
/// Resolution is **mount-first**: whichever mounted archive has any encoding of
/// the texture wins, and only then does encoding preference break the tie. That
/// ordering is what lets a mod layer's upgraded `.dds` replace the original
/// `.pcx` a model names, instead of the original always winning because it was
/// asked for by name.
///
/// Candidates are tried **`txt16/`-qualified first**. Model and mesh textures
/// live in their family archive's `txt16/` subdirectory, and `ZipAssetPath`
/// registers every entry under its bare basename as well as its full path - so
/// an unqualified lookup can match a same-named texture from an unrelated family
/// (a terrain `black.dds` in place of a model's `obj/txt16/BLACK.PCX`).
/// Qualifying keeps the search inside the right namespace.
pub fn resolve_texture_name(asset_cache: &AssetCache, requested: &str) -> Option<String> {
    let base_path = asset_cache.base_path().to_owned();
    let paths = asset_cache.asset_paths();

    let stem = Path::new(requested).with_extension("");
    let stem = stem.to_str()?.to_ascii_lowercase();

    let extensions = engine::texture_format::DECODABLE_EXTENSIONS;
    let mut candidates = Vec::with_capacity(extensions.len() * 2);
    // Correct namespace, best encoding first...
    for ext in extensions {
        candidates.push(format!("{TEXTURE_SUBDIR}/{stem}.{ext}"));
    }
    // ...then the bare basename, for textures that don't live under txt16/.
    for ext in extensions {
        candidates.push(format!("{stem}.{ext}"));
    }

    let resolved = paths.resolve_first(base_path, &candidates)?;
    if !resolved.eq_ignore_ascii_case(requested) {
        trace!("texture {requested} resolved to {resolved}");
    }
    Some(resolved)
}

/// Resolve the diffuse texture used by a Dark LGMD object material.
///
/// 25th Anniversary replacement models can attach a `.mtl` render-material
/// script to the material name. The primary pass may select a different
/// texture from the same-named bitmap.
pub fn resolve_object_material_texture_name(
    asset_cache: &AssetCache,
    requested: &str,
) -> Option<String> {
    let redirected = object_material_primary_texture(asset_cache, requested);
    let texture_name = redirected.as_deref().unwrap_or(requested);
    let resolved = resolve_texture_name(asset_cache, texture_name);

    if redirected.is_some() && resolved.is_none() {
        return resolve_texture_name(asset_cache, requested);
    }

    resolved
}

fn object_material_primary_texture(asset_cache: &AssetCache, requested: &str) -> Option<String> {
    let stem = Path::new(requested).with_extension("");
    let stem = stem.to_str()?.to_ascii_lowercase();
    let candidates = [
        format!("{TEXTURE_SUBDIR}/{stem}.mtl"),
        format!("{stem}.mtl"),
    ];
    let script_name = asset_cache
        .asset_paths()
        .resolve_first(asset_cache.base_path().to_owned(), &candidates)?;

    let reader = asset_cache.get_raw_reader(&script_name)?;
    let mut script = String::new();
    reader.borrow_mut().read_to_string(&mut script).ok()?;
    let texture = primary_render_pass_texture(&script)?;
    trace!("object material {requested} primary pass redirects to {texture}");
    Some(texture)
}

fn primary_render_pass_texture(script: &str) -> Option<String> {
    let mut in_render_pass = false;
    let mut saw_open_brace = false;
    let mut brace_depth = 0_i32;

    for raw_line in script.lines() {
        let line = raw_line
            .split_once("//")
            .map_or(raw_line, |(code, _)| code)
            .trim();
        if line.is_empty() {
            continue;
        }

        if !in_render_pass {
            let directive = line.split_whitespace().next()?;
            if !directive.eq_ignore_ascii_case("render_pass") {
                continue;
            }
            in_render_pass = true;
        }

        let mut fields = line.split_whitespace();
        if fields
            .next()
            .is_some_and(|field| field.eq_ignore_ascii_case("texture"))
        {
            let texture = fields.next()?.trim_matches('"');
            if texture.eq_ignore_ascii_case("$texture") {
                return None;
            }
            return normalize_material_texture_reference(texture);
        }

        brace_depth += line.chars().filter(|character| *character == '{').count() as i32;
        if line.contains('{') {
            saw_open_brace = true;
        }
        brace_depth -= line.chars().filter(|character| *character == '}').count() as i32;
        if saw_open_brace && brace_depth <= 0 {
            return None;
        }
    }

    None
}

fn normalize_material_texture_reference(reference: &str) -> Option<String> {
    let normalized = reference.replace('\\', "/");
    let lower = normalized.to_ascii_lowercase();
    let relative = if lower.starts_with("obj/txt16/") {
        &normalized["obj/txt16/".len()..]
    } else if lower.starts_with("obj/") {
        &normalized["obj/".len()..]
    } else if lower.starts_with("fam/") {
        &normalized["fam/".len()..]
    } else {
        &normalized
    };

    (!relative.is_empty()).then(|| relative.to_owned())
}

/// Family archives keep their textures in this subdirectory (`obj/txt16/...`,
/// `mesh/txt16/...`), and are mounted at the family root - so within a mount the
/// path is just `txt16/<name>`.
const TEXTURE_SUBDIR: &str = "txt16";

/// [`resolve_texture_name`] plus the actual load. Returns `None` when no encoding
/// of the texture is available.
pub fn load_texture_with_fallback(
    asset_cache: &mut AssetCache,
    requested: &str,
) -> Option<Rc<Texture>> {
    let resolved = resolve_texture_name(asset_cache, requested)?;
    asset_cache.get_opt(&TEXTURE_IMPORTER, &resolved)
}

///
/// load_multiple_textures
///
/// Loads frames of an animated texture, using the dark engine format (e.g. "FLOOR_1.PCX", "FLOOR_2.PCX", etc.)
pub fn load_multiple_textures_for_family(
    asset_cache: &mut AssetCache,
    family_name: &str,
    tex_name: &str,
) -> Vec<Rc<Texture>> {
    load_multiple_textures(
        asset_cache,
        format!("{}/{}.PCX", family_name, tex_name).as_str(),
        false,
    )
}

pub fn load_multiple_textures_for_model(
    asset_cache: &mut AssetCache,
    tex_name: &str,
) -> Vec<Rc<Texture>> {
    load_multiple_textures(asset_cache, tex_name, true)
}

pub fn load_multiple_textures(
    asset_cache: &mut AssetCache,
    tex_name: &str,
    require_underscore: bool,
) -> Vec<Rc<Texture>> {
    let mut textures = Vec::new();
    let mut next_idx = 1;

    let path = Path::new(tex_name);
    let path_without_extension = path.with_extension("");
    let maybe_extension_str = path.extension().and_then(|s| s.to_str());
    let maybe_path_without_extension_str = path_without_extension.to_str();

    let maybe_underscore = if path_without_extension
        .to_str()
        .map(|s| s.ends_with('_'))
        .unwrap_or(false)
    {
        ""
    } else if require_underscore {
        return vec![];
    } else {
        "_"
    };
    if let (Some(extension), Some(path_without_extension)) =
        (maybe_extension_str, maybe_path_without_extension_str)
    {
        loop {
            let mut maybe_texture = asset_cache.get_opt(
                &TEXTURE_IMPORTER,
                &format!(
                    "{}{}{}.{}",
                    path_without_extension, maybe_underscore, next_idx, extension
                ),
            );

            // Sometimes, single digit numbers are padded with a 0, ie: 're301_01.pcx'
            // We should check for that case, too...
            if maybe_texture.is_none() && next_idx < 10 {
                maybe_texture = asset_cache.get_opt(
                    &TEXTURE_IMPORTER,
                    &format!(
                        "{}{}0{}.{}",
                        path_without_extension, maybe_underscore, next_idx, extension
                    ),
                );
            }

            if let Some(texture) = maybe_texture {
                textures.push(texture.clone());
                next_idx += 1;
            } else {
                break;
            }
        }
    }
    textures
}

pub fn compute_bounding_sphere(vertices: &Vec<Point3<f32>>) -> Sphere<f32> {
    let mut sphere = Sphere {
        radius: 0.0,
        center: *vertices.get(0).unwrap_or(&point3(0.0, 0.0, 0.0)),
    };
    // Compute bounding sphere
    for vertex in vertices {
        if (vertex - sphere.center).magnitude() > sphere.radius {
            sphere.radius = (vertex - sphere.center).magnitude();
        }
    }
    // Refine the sphere's center and radius by iterating over each vertex again
    for vertex in vertices {
        let d = (vertex - sphere.center).magnitude();
        if d > sphere.radius {
            let overage = d - sphere.radius;
            sphere.radius += overage / 2.0;
            sphere.center = sphere.center + (vertex - sphere.center) * (overage / (2.0 * d));
        }
    }

    sphere
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, collections::HashMap, io::Cursor};

    use engine::assets::{
        asset_cache::AssetCache,
        asset_paths::{AbstractAssetPath, ReadableAndSeekable},
    };

    use super::resolve_object_material_texture_name;

    struct FakeAssetPath(HashMap<String, Vec<u8>>);

    impl AbstractAssetPath for FakeAssetPath {
        fn exists(&self, _base_path: String, asset_name: String) -> bool {
            self.0.contains_key(&asset_name)
        }

        fn get_reader(
            &self,
            _base_path: String,
            asset_name: String,
        ) -> Option<RefCell<Box<dyn ReadableAndSeekable>>> {
            let bytes = self.0.get(&asset_name)?.clone();
            Some(RefCell::new(Box::new(Cursor::new(bytes))))
        }
    }

    fn cache(assets: &[(&str, &[u8])]) -> AssetCache {
        let assets = assets
            .iter()
            .map(|(name, bytes)| ((*name).to_owned(), bytes.to_vec()))
            .collect();
        AssetCache::new(String::new(), Box::new(FakeAssetPath(assets)))
    }

    #[test]
    fn object_material_uses_primary_mtl_render_pass_texture() {
        let asset_cache = cache(&[
            (
                "txt16/nd-airlock_scr.mtl",
                b"render_material_only 1\nrender_pass\n{\n texture OBJ\\TXT16\\ND-airlock_2\n shaded 0\n}\n",
            ),
            ("txt16/nd-airlock_scr.dds", b"magenta helper"),
            ("txt16/nd-airlock_2.dds", b"opaque door atlas"),
        ]);

        assert_eq!(
            resolve_object_material_texture_name(&asset_cache, "ND-airlock_scr"),
            Some("txt16/nd-airlock_2.dds".to_owned())
        );
    }

    #[test]
    fn object_material_without_mtl_keeps_ordinary_opaque_texture() {
        let asset_cache = cache(&[("txt16/panel.dds", b"opaque panel")]);

        assert_eq!(
            resolve_object_material_texture_name(&asset_cache, "PANEL.PCX"),
            Some("txt16/panel.dds".to_owned())
        );
    }

    #[test]
    fn object_material_texture_macro_keeps_the_authored_default() {
        let asset_cache = cache(&[
            (
                "txt16/panel.mtl",
                b"render_pass\n{\n texture $TEXTURE\n}\nrender_pass\n{\n texture OBJ\\TXT16\\reflection\n}\n",
            ),
            ("txt16/panel.dds", b"opaque panel"),
            ("txt16/reflection.dds", b"reflection map"),
        ]);

        assert_eq!(
            resolve_object_material_texture_name(&asset_cache, "PANEL.PCX"),
            Some("txt16/panel.dds".to_owned())
        );
    }
}
