//! The first verified consumers of additive incidence passes. Keep selection
//! narrow until other material profiles have their own rendering comparisons.
use crate::importers::TEXTURE_IMPORTER;
use engine::{
    assets::asset_cache::AssetCache,
    scene::{SceneObject, SkinnedMaterial, basic_material, incidence::IncidencePass},
    texture::TextureTrait,
};
use std::{cell::RefCell, path::Path, rc::Rc};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Profile {
    PlanarDecal,
    Organic,
    WetGrowth,
}

fn profile(name: &str) -> Option<Profile> {
    match Path::new(name)
        .file_stem()?
        .to_str()?
        .to_ascii_lowercase()
        .as_str()
    {
        "nd-mtlhit" => Some(Profile::PlanarDecal),
        // The mounted SHTUP Hydro/Earth growth uses GOT*, not the legacy GOO*.
        "got2_" | "got3_" | "got4_" => Some(Profile::WetGrowth),
        "nd-anegg" | "nd-grub" | "nd-spdrbby" | "nd-spiderboss" | "nd-overlord" | "nd-reaver" => {
            Some(Profile::Organic)
        }
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
    // Mounted goo's flat underside lies on the floor and z-fights it; the
    // bias carries over to the overlay duplicated below.
    if profile == Profile::WetGrowth {
        if let Some(base) = objects.last_mut() {
            base.set_depth_bias(true);
        }
    }
    let Some(base) = objects.last().map(SceneObject::duplicate) else {
        return;
    };
    // Mesh materials can share a basename with incomplete obj-family stubs.
    // Preserve the family through script includes and mask texture lookup.
    let material_name = if skinned {
        format!("mesh/txt16/{name}")
    } else {
        name.to_owned()
    };
    let passes = if profile == Profile::WetGrowth {
        // Project-owned adaptation of the Nightdive incidence technique. Reuse
        // this surface's diffuse/alpha rather than another model's UV mask, and
        // keep it lit so dark rooms do not acquire glowing growth. Missing ramp
        // art follows the normal fallback below and adds no overlay.
        vec![super::MaterialIncidencePass {
            texture: None,
            ramp: "materials/nd-ir_shine".into(),
            tint: cgmath::vec3(0.5, 0.5, 0.5),
            unlit: false,
            blend_mode: engine::scene::scene_object::BlendMode::AdditiveAlpha,
        }]
    } else {
        super::object_material_incidence_passes(assets, &material_name)
    };
    for pass in passes {
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
        let blend_mode = pass.blend_mode;
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
        overlay.blend_mode = blend_mode;
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
    fn growth_profile_is_limited_to_the_mounted_worm_goo_textures() {
        for name in ["GOT2_.PCX", "got3_.dds", "got4_.pcx"] {
            assert!(profile(name).is_some(), "{name}");
        }
        for name in ["got2_1", "goo2_", "ND-boss_head", "ordinary"] {
            assert!(profile(name).is_none(), "{name} must remain unchanged");
        }
    }

    #[test]
    fn only_verified_surfaces_enable_overlays() {
        assert_eq!(profile("ND-anegg.psd"), Some(Profile::Organic));
        assert_eq!(profile("ND-GRUB.DDS"), Some(Profile::Organic));
        assert_eq!(profile("ND-mtlhit"), Some(Profile::PlanarDecal));
        for name in [
            "ND-spdrBby.psd",
            "ND-spiderboss",
            "ND-overlord",
            "ND-reaver",
        ] {
            assert_eq!(profile(name), Some(Profile::Organic));
        }
        for name in ["ND-anegg_c", "ND-grub_extra", "ND-goldegg", "ordinary"] {
            assert_eq!(profile(name), None, "{name} has not been verified");
        }
    }
}
