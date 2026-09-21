//! The first verified consumers of additive incidence passes. Keep selection
//! narrow until other material profiles have their own rendering comparisons.
use crate::importers::TEXTURE_IMPORTER;
use engine::{
    assets::asset_cache::AssetCache,
    scene::{
        SceneObject, SkinnedMaterial, basic_material, incidence::IncidencePass,
        scene_object::BlendMode,
    },
    texture::TextureTrait,
};
use std::{cell::RefCell, path::Path, rc::Rc};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Profile {
    PlanarDecal,
    Organic,
}

fn profile(name: &str) -> Option<Profile> {
    match Path::new(name)
        .file_stem()?
        .to_str()?
        .to_ascii_lowercase()
        .as_str()
    {
        "nd-mtlhit" => Some(Profile::PlanarDecal),
        "nd-anegg" | "nd-grub" => Some(Profile::Organic),
        _ => None,
    }
}

/// Append overlays to the most recently emitted base object. Duplicating its
/// draw state preserves geometry, skinning, culling, local pose and depth bias
/// across the rigid LGMD, legacy creature and high-detail creature importers.
pub(crate) fn append_incidence_overlays(
    objects: &mut Vec<SceneObject>,
    assets: &mut AssetCache,
    name: &str,
    diffuse: Rc<dyn TextureTrait>,
    skinned: bool,
) {
    let Some(profile) = profile(name) else {
        return;
    };
    let Some(base) = objects.last().map(SceneObject::duplicate) else {
        return;
    };
    for pass in super::object_material_incidence_passes(assets, name) {
        // Organic profiles first opt into their dedicated specular textures.
        // Shared fill/rim passes using the diffuse texture remain a follow-up.
        if profile == Profile::Organic && pass.texture.is_none() {
            continue;
        }
        let texture: Rc<dyn TextureTrait> = if let Some(name) = pass.texture.as_deref() {
            let Some(texture) = super::load_texture_with_fallback(assets, name) else {
                continue;
            };
            texture
        } else {
            diffuse.clone()
        };
        let Some(ramp) = load_incidence_ramp(assets, &pass.ramp) else {
            continue;
        };
        let pass = IncidencePass {
            ramp,
            tint: pass.tint,
            unlit: pass.unlit,
            geometric_normal: profile == Profile::PlanarDecal,
        };
        let material = if skinned {
            SkinnedMaterial::create_incidence(texture, pass)
        } else {
            basic_material::create_incidence(texture, pass)
        };
        let mut overlay = base.duplicate();
        overlay.material = Rc::new(RefCell::new(material));
        overlay.blend_mode = BlendMode::AdditiveAlpha;
        overlay.set_depth_write(false);
        objects.push(overlay);
    }
}

fn load_incidence_ramp(assets: &mut AssetCache, name: &str) -> Option<Rc<dyn TextureTrait>> {
    let candidates = engine::texture_format::DECODABLE_EXTENSIONS
        .iter()
        .map(|ext| format!("{name}.{ext}"))
        .collect::<Vec<_>>();
    let resolved = assets
        .asset_paths()
        .resolve_first(assets.base_path().to_owned(), &candidates)?;
    assets
        .get_ext_opt(
            &TEXTURE_IMPORTER,
            &resolved,
            &engine::texture::TextureOptions {
                wrap: false,
                ..Default::default()
            },
        )
        .map(|texture| texture as Rc<dyn TextureTrait>)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_verified_surfaces_enable_overlays() {
        assert_eq!(profile("ND-anegg.psd"), Some(Profile::Organic));
        assert_eq!(profile("ND-GRUB.DDS"), Some(Profile::Organic));
        assert_eq!(profile("ND-mtlhit"), Some(Profile::PlanarDecal));
        for name in ["ND-anegg_c", "ND-grub_extra", "ND-goldegg", "ordinary"] {
            assert_eq!(profile(name), None, "{name} has not been verified");
        }
    }
}
