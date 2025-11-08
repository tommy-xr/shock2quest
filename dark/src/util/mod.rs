mod merge_maps;

use cgmath::{InnerSpace, Point3, point3};
use collision::Sphere;
pub use merge_maps::*;

use std::{path::Path, rc::Rc};

use engine::{assets::asset_cache::AssetCache, texture::Texture};

use crate::importers::TEXTURE_IMPORTER;

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
