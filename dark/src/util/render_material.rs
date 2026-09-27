//! NewDark render-material plans. Unsupported directives reject the whole plan;
//! the existing single-texture fallback remains available, without pretending a
//! partially parsed effect is a faithful ordered material.
use cgmath::{Vector3, vec3};
use engine::scene::render_pass::BlendFactor;

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Pass {
    pub texture: Option<String>,
    pub blend: (BlendFactor, BlendFactor),
    pub color: Vector3<f32>,
    pub alpha: f32,
    pub incidence: Option<(String, f32)>,
    pub replace_alpha: bool,
    pub shaded: bool,
    pub mipmap_bias: f32,
    pub environment: bool,
    pub clamp: bool,
}
impl Pass {
    fn incidence_distance(&self) -> Option<f32> {
        self.incidence
            .as_ref()
            .map(|(_, distance)| distance / crate::SCALE_FACTOR)
    }
}
impl Default for Pass {
    fn default() -> Self {
        Self {
            texture: None,
            blend: (BlendFactor::SrcAlpha, BlendFactor::InvSrcAlpha),
            color: vec3(1.0, 1.0, 1.0),
            alpha: 1.0,
            incidence: None,
            replace_alpha: false,
            shaded: false,
            mipmap_bias: 0.0,
            environment: false,
            clamp: false,
        }
    }
}
#[derive(Debug, Default, PartialEq)]
pub(super) struct Plan {
    pub material_only: bool,
    pub force_opaque: bool,
    pub passes: Vec<Pass>,
}

pub(super) fn parse(source: &str) -> Result<Plan, String> {
    let mut plan = Plan::default();
    let mut pass: Option<Pass> = None;
    let mut opened = false;
    let boolean = |values: &[&str]| match values {
        [] | ["1"] => Ok(true),
        ["0"] => Ok(false),
        _ => Err("invalid boolean".to_owned()),
    };
    let number = |value: &str| {
        value
            .parse::<f32>()
            .ok()
            .filter(|v| v.is_finite())
            .ok_or_else(|| "invalid number".to_owned())
    };
    for raw in source.lines() {
        let line = raw.split_once("//").map_or(raw, |(code, _)| code).trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        // Braces can follow render_pass, but directives remain one per line.
        let tokens = line.replace('{', " { ").replace('}', " } ");
        let fields: Vec<_> = tokens.split_whitespace().collect();
        let key = fields[0].to_ascii_lowercase();
        let args = &fields[1..];
        match key.as_str() {
            "render_pass" if pass.is_none() && (args.is_empty() || args == ["{"]) => {
                pass = Some(Pass::default());
                opened = !args.is_empty();
                continue;
            }
            "{" if pass.is_some() && !opened && args.is_empty() => {
                opened = true;
                continue;
            }
            "}" if pass.is_some() && opened && args.is_empty() => {
                plan.passes.push(pass.take().unwrap());
                opened = false;
                continue;
            }
            _ => {}
        }
        if let Some(p) = pass.as_mut() {
            if !opened {
                return Err("pass lacks opening brace".into());
            }
            match (key.as_str(), args) {
                ("texture", [texture]) => {
                    if p.texture.is_some() {
                        return Err("multiple texture stages".into());
                    }
                    p.texture = Some(texture.trim_matches('"').replace('\\', "/"));
                }
                ("blend", [src, dst]) => {
                    p.blend = (
                        BlendFactor::parse(src).ok_or("unknown blend source")?,
                        BlendFactor::parse(dst).ok_or("unknown blend destination")?,
                    )
                }
                ("rgb", [r, g, b]) => {
                    p.color = vec3(
                        number(r.trim_end_matches(','))?,
                        number(g.trim_end_matches(','))?,
                        number(b.trim_end_matches(','))?,
                    )
                }
                ("alpha", [value]) => p.alpha = number(value)?,
                ("alpha", [func, kind, alpha, distance, ramp])
                    if func.eq_ignore_ascii_case("func")
                        && kind.eq_ignore_ascii_case("incidence") =>
                {
                    p.alpha = number(alpha)?;
                    let distance = number(distance)?;
                    if distance <= 0.0 {
                        return Err("incidence distance must be positive".into());
                    }
                    p.incidence = Some((ramp.replace('\\', "/"), distance));
                }
                ("replace_alpha", values) => p.replace_alpha = boolean(values)?,
                ("shaded", values) => p.shaded = boolean(values)?,
                ("mipmap_bias", [value]) => p.mipmap_bias = number(value)?,
                ("uv_clamp", values) => p.clamp = boolean(values)?,
                ("uv_source", [value]) if value.eq_ignore_ascii_case("environment") => {
                    p.environment = true
                }
                ("uv_source", [value]) if value.eq_ignore_ascii_case("texture") => {
                    p.environment = false
                }
                _ => return Err(format!("unsupported pass directive: {line}")),
            }
        } else {
            match key.as_str() {
                "render_material_only" => plan.material_only = boolean(args)?,
                "force_opaque" => plan.force_opaque = boolean(args)?,
                // Texture sizing is consumed by UI/world loaders and doesn't alter model pass state.
                "ui_scale" | "terrain_scale" => {}
                _ => return Err(format!("unsupported material directive: {line}")),
            }
        }
    }
    if pass.is_some() {
        return Err("unterminated render pass".into());
    }
    if plan.passes.is_empty() {
        plan.material_only = false;
    }
    Ok(plan)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preserves_authored_order_and_vertex_alpha_semantics() {
        let p = parse("render_material_only 1\nforce_opaque\nrender_pass {\ntexture $TEXTURE\nblend SRC_ALPHA ONE\nreplace_alpha 1\nALPHA 0.2\nRGB 0.3, 0.5, 0.8\nshaded 0\nmipmap_bias 2\n}\nrender_pass\n{\ntexture diffuse\nshaded 1\n}\n").unwrap();
        assert!(p.material_only && p.force_opaque);
        assert_eq!(p.passes.len(), 2);
        assert_eq!(p.passes[0].texture.as_deref(), Some("$TEXTURE"));
        assert_eq!(p.passes[0].blend, (BlendFactor::SrcAlpha, BlendFactor::One));
        assert_eq!(p.passes[0].alpha, 0.2);
        assert!(p.passes[0].replace_alpha && !p.passes[0].shaded);
        assert_eq!(p.passes[0].mipmap_bias, 2.0);
        assert_eq!(p.passes[0].color, vec3(0.3, 0.5, 0.8));
        assert!(p.passes[1].shaded && !p.passes[1].replace_alpha);
    }
    #[test]
    fn never_guesses_unsupported_state_or_malformed_passes() {
        for source in [
            "render_pass {\nblend BANANA ZERO\n}",
            "render_pass {\nalpha func WAVE SINE 0 1 0 1000\n}",
            "render_pass {\ntexture a",
            "render_pass {\ntexture a\ntexture b\n}",
        ] {
            assert!(parse(source).is_err(), "{source}");
        }
        assert!(
            !parse("render_material_only 1\n//render_pass {\n# texture no\n//}\n")
                .unwrap()
                .material_only
        );
    }
    #[test]
    fn keeps_incidence_parameters_and_modulation() {
        let p = parse(
            "render_pass {\nblend DST_COLOR ZERO\nalpha func INCIDENCE 0.6 30 materials/ramp\n}\n",
        )
        .unwrap();
        assert_eq!(
            p.passes[0].blend,
            (BlendFactor::DstColor, BlendFactor::Zero)
        );
        assert_eq!(p.passes[0].alpha, 0.6);
        // A surface 30 authored feet away is 12 engine units away. Its
        // incidence lookup must still reach the last distance row.
        assert_eq!(p.passes[0].incidence_distance(), Some(12.0));
        assert_eq!(p.passes[0].incidence, Some(("materials/ramp".into(), 30.0)));
    }
}

pub(super) fn apply(
    object: &mut engine::scene::SceneObject,
    assets: &mut engine::assets::asset_cache::AssetCache,
    requested: &str,
    skinned: bool,
) -> bool {
    use engine::{
        scene::{
            Material, SkinnedMaterial, basic_material,
            render_pass::RenderPass,
            scene_object::{BlendMode, MaterialPass, MaterialStack},
        },
        texture::{CubeTexture, TextureOptions, TextureTrait},
    };
    use std::{cell::RefCell, io::Read, rc::Rc};
    let Some(source) = super::object_material_script(assets, requested) else {
        return false;
    };
    let plan = match parse(&source) {
        Ok(plan) => plan,
        Err(reason) => {
            tracing::debug!("material {requested}: keeping legacy fallback ({reason})");
            return false;
        }
    };
    if plan.passes.is_empty() {
        if plan.force_opaque {
            object.material.borrow_mut().set_render_pass(RenderPass {
                force_opaque: true,
                shaded: true,
                ..Default::default()
            });
        }
        return true;
    }
    let load = |assets: &mut engine::assets::asset_cache::AssetCache,
                name: &str,
                clamp: bool|
     -> Option<Rc<dyn TextureTrait>> {
        let resolved = super::resolve_texture_name(assets, name)?;
        assets
            .get_ext_opt(
                &crate::importers::TEXTURE_IMPORTER,
                &resolved,
                &TextureOptions {
                    wrap: !clamp,
                    ..Default::default()
                },
            )
            .map(|t| t as Rc<dyn TextureTrait>)
    };
    let original = if !plan.material_only {
        let Some(texture) = load(assets, requested, false) else {
            return false;
        };
        Some(texture)
    } else {
        None
    };
    let mut passes = Vec::new();
    let mut has_base = !plan.material_only;
    for pass in &plan.passes {
        let name = pass.texture.as_deref().map(|name| {
            if name.eq_ignore_ascii_case("$TEXTURE") {
                requested
            } else {
                name
            }
        });
        // Other macros and unavailable textures are not silently replaced with white.
        if name.is_some_and(|name| name.starts_with('$')) {
            return false;
        }
        let mut state = RenderPass {
            color: pass.color,
            alpha: pass.alpha,
            replace_alpha: pass.replace_alpha,
            force_opaque: plan.force_opaque
                && name.is_some_and(|name| name.eq_ignore_ascii_case(requested)),
            shaded: pass.shaded,
            mipmap_bias: pass.mipmap_bias,
            ..Default::default()
        };
        let texture: Rc<dyn TextureTrait> = if pass.environment {
            let Some(resolved) = name.and_then(|name| super::resolve_texture_name(assets, name))
            else {
                return false;
            };
            let Some(reader) = assets.get_raw_reader(&resolved) else {
                return false;
            };
            let mut bytes = Vec::new();
            if reader.borrow_mut().read_to_end(&mut bytes).is_err() {
                return false;
            }
            let Some(cube) = engine::dds::decode_cube_rgba8(&bytes) else {
                return false;
            };
            state.cube = Some(Rc::new(CubeTexture::new(&cube)));
            engine::texture::shared_white_pixel()
        } else if let Some(name) = name {
            let Some(texture) = load(assets, name, pass.clamp) else {
                return false;
            };
            texture
        } else {
            engine::texture::shared_white_pixel()
        };
        if let Some((ramp, _)) = &pass.incidence {
            let Some(texture) = load(assets, ramp, true) else {
                return false;
            };
            // Material distance is authored in Dark feet, like model vertices.
            state.incidence = Some((texture, pass.incidence_distance().unwrap()));
        }
        let emissivity = object.material.borrow().emissivity();
        let mut material: Box<dyn Material> = if skinned {
            SkinnedMaterial::create(texture, emissivity, 0.0)
        } else {
            basic_material::create(texture, emissivity, 0.0)
        };
        let base = pass.incidence.is_none()
            && matches!(
                pass.blend,
                (BlendFactor::SrcAlpha, BlendFactor::InvSrcAlpha)
                    | (BlendFactor::One, BlendFactor::Zero)
            );
        let writes_depth = !has_base && base && pass.alpha >= 1.0;
        has_base |= writes_depth;
        state.alpha_test = writes_depth;
        material.set_render_pass(state);
        passes.push(MaterialPass {
            material: Rc::new(RefCell::new(material)),
            blend: BlendMode::Authored(pass.blend.0, pass.blend.1),
            writes_depth,
            replaces_alpha: pass.replace_alpha,
        });
    }
    // Loading is transactional: a missing pass leaves the complete prior material.
    if let Some(texture) = original {
        object.material.borrow_mut().set_diffuse_texture(texture);
    }
    if plan.force_opaque {
        object.material.borrow_mut().set_render_pass(RenderPass {
            force_opaque: true,
            shaded: true,
            ..Default::default()
        });
    }
    if !passes.is_empty() {
        object.material_stack = Some(Rc::new(MaterialStack {
            material_only: plan.material_only,
            passes,
        }));
    }
    true
}
