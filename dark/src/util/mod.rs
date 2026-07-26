mod merge_maps;

use cgmath::{InnerSpace, Point3, point3};
use collision::Sphere;
pub use merge_maps::*;

use std::{path::Path, rc::Rc};

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
