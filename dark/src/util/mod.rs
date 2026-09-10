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

/// Resolve icon art without the model-texture `txt16/` search. Object icons
/// prefer their family before considering legacy bare names (also used for
/// report portraits). An explicitly qualified request stays in that family.
/// Within a family, preserve mod precedence and prefer DDS/PNG over PCX.
pub fn resolve_object_icon_name(asset_cache: &AssetCache, requested: &str) -> Option<String> {
    let stem = Path::new(requested).with_extension("");
    let stem = stem.to_str()?.to_ascii_lowercase();
    let resolve = |stem: &str| {
        let candidates = engine::texture_format::DECODABLE_EXTENSIONS
            .iter()
            .map(|ext| format!("{stem}.{ext}"))
            .collect::<Vec<_>>();
        asset_cache
            .asset_paths()
            .resolve_first(asset_cache.base_path().to_owned(), &candidates)
    };
    if !stem.contains('/') {
        if let Some(name) = resolve(&format!("objicon/{stem}")) {
            return Some(name);
        }
    }
    resolve(&stem)
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
    let script = object_material_script(asset_cache, requested)?;
    let texture = primary_render_pass_texture(&script)?;
    trace!("object material {requested} primary pass redirects to {texture}");
    Some(texture)
}

fn object_material_script(asset_cache: &AssetCache, requested: &str) -> Option<String> {
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
    Some(script)
}

/// A sole unlit, additive pass using the authored texture (25AE gun flashes).
/// Do not apply overlay blending to ordinary multi-pass weapon materials.
pub fn object_material_is_additive_flash(asset_cache: &AssetCache, requested: &str) -> bool {
    object_material_script(asset_cache, requested)
        .is_some_and(|script| is_additive_flash_script(&script))
}

fn is_additive_flash_script(script: &str) -> bool {
    let passes = render_passes(script);
    passes.len() == 1
        && passes[0].keeps_authored_texture
        && passes[0].additive_color
        && passes[0].unlit
        && passes[0].texture.is_none()
}

/// Find the texture a render-material script substitutes for the model's own
/// diffuse, if any.
///
/// Only a *base* pass can supply the diffuse. 25AE materials routinely open
/// with an additive shine or modulate overlay whose texture is a specular map,
/// and reach their real diffuse through an `include` (not yet followed - see
/// #912); treating such an overlay as the diffuse renders the object with its
/// spec map and can make it disappear. Passes that cannot be a base pass are
/// skipped, and `$TEXTURE` in a base pass means the model keeps its authored
/// texture.
fn primary_render_pass_texture(script: &str) -> Option<String> {
    for pass in render_passes(script) {
        if pass.blend_is_base.unwrap_or(true) {
            if pass.keeps_authored_texture {
                return None;
            }
            if let Some(texture) = pass.texture.as_deref() {
                return normalize_material_texture_reference(texture);
            }
        }
    }
    None
}

fn render_passes(script: &str) -> Vec<RenderPassFields> {
    let mut passes = Vec::new();
    let mut pass: Option<RenderPassFields> = None;
    let mut brace_depth = 0_i32;
    let mut saw_open_brace = false;

    for raw_line in script.lines() {
        let line = raw_line
            .split_once("//")
            .map_or(raw_line, |(code, _)| code)
            .trim();
        if line.is_empty() {
            continue;
        }

        let mut fields = line.split_whitespace();
        let directive = fields.next().unwrap_or_default();

        // Reset on every `render_pass`, so an unterminated block cannot bleed
        // its fields into the next pass.
        if directive.eq_ignore_ascii_case("render_pass") {
            pass = Some(RenderPassFields::default());
            brace_depth = 0;
            saw_open_brace = false;
        }

        if let Some(fields_so_far) = pass.as_mut() {
            if directive.eq_ignore_ascii_case("texture") {
                match fields.next().map(|texture| texture.trim_matches('"')) {
                    // `$TEXTURE` is the model's own texture, i.e. no redirect.
                    Some(texture) if texture.eq_ignore_ascii_case("$texture") => {
                        fields_so_far.keeps_authored_texture = true;
                    }
                    // The first `texture` is the diffuse; later ones are
                    // additional stages of the same pass.
                    Some(texture) if fields_so_far.texture.is_none() => {
                        fields_so_far.texture = Some(texture.to_owned());
                    }
                    _ => {}
                }
            } else if directive.eq_ignore_ascii_case("blend") {
                let source = fields.next();
                let destination = fields.next();
                fields_so_far.blend_is_base = Some(blend_is_base(source, destination));
                fields_so_far.additive_color = source
                    .is_some_and(|s| s.eq_ignore_ascii_case("SRC_COLOR"))
                    && destination.is_some_and(|s| s.eq_ignore_ascii_case("ONE"));
            } else if directive.eq_ignore_ascii_case("shaded") {
                fields_so_far.unlit = fields.next() == Some("0");
            }
        }

        brace_depth += line.chars().filter(|character| *character == '{').count() as i32;
        if line.contains('{') {
            saw_open_brace = true;
        }
        brace_depth -= line.chars().filter(|character| *character == '}').count() as i32;

        if saw_open_brace && brace_depth <= 0 {
            if let Some(fields_so_far) = pass.take() {
                passes.push(fields_so_far);
            }
            saw_open_brace = false;
        }
    }

    passes
}

/// The fields of one `render_pass` block that texture selection depends on.
#[derive(Default)]
struct RenderPassFields {
    /// The first `texture` directive in the pass.
    texture: Option<String>,
    /// The pass names `$TEXTURE` - the model's own texture.
    keeps_authored_texture: bool,
    /// `None` when the pass carries no `blend` directive at all.
    blend_is_base: Option<bool>,
    additive_color: bool,
    unlit: bool,
}

/// Whether a `blend <source> <destination>` pair describes a base pass - one
/// that can carry the diffuse - rather than an overlay composited onto an
/// earlier pass.
///
/// Ordinary alpha blending (`SRC_ALPHA INV_SRC_ALPHA`) and plain opaque replace
/// (`ONE ZERO`) are base passes. Additive destinations (`ONE`) accumulate onto
/// what is already drawn, and any factor reading the destination colour
/// modulates it, so both are overlays.
fn blend_is_base(source: Option<&str>, destination: Option<&str>) -> bool {
    // A `blend` directive we cannot read is assumed to composite: guessing
    // "base" here would reinstate exactly the bug this selection prevents.
    let (Some(source), Some(destination)) = (source, destination) else {
        return false;
    };

    if source.eq_ignore_ascii_case("DST_COLOR") || source.eq_ignore_ascii_case("INV_DST_COLOR") {
        return false;
    }

    destination.eq_ignore_ascii_case("INV_SRC_ALPHA") || destination.eq_ignore_ascii_case("ZERO")
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
    fn ui_icons_prefer_their_family_and_high_resolution_encoding() {
        let assets = cache(&[
            ("disc.dds", b"model texture"),
            ("disc.pcx", b"model texture"),
            ("objicon/disc.pcx", b"classic icon"),
            ("objicon/disc.png", b"remastered icon"),
            ("iface/frame.pcx", b"classic frame"),
            ("iface/frame.dds", b"remastered frame"),
            ("mport.pcx", b"legacy portrait"),
        ]);
        assert_eq!(
            super::resolve_object_icon_name(&assets, "DISC.PCX").as_deref(),
            Some("objicon/disc.png")
        );
        assert_eq!(
            super::resolve_object_icon_name(&assets, "iface/frame.pcx").as_deref(),
            Some("iface/frame.dds")
        );
        assert_eq!(
            super::resolve_object_icon_name(&assets, "mport.pcx").as_deref(),
            Some("mport.pcx")
        );
        assert_eq!(
            super::resolve_object_icon_name(&assets, "iface/disc.pcx"),
            None
        );
        let classic = cache(&[("objicon/disc.pcx", b"classic icon")]);
        assert_eq!(
            super::resolve_object_icon_name(&classic, "disc.png").as_deref(),
            Some("objicon/disc.pcx")
        );
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

    /// 25AE viewmodel materials open with an *additive* shine overlay whose
    /// texture is the specular map (`ND-atek.mtl` is the pistol); the diffuse
    /// pass arrives through an `include`. Taking that overlay as the diffuse
    /// swapped the pistol's base texture for its spec map and rendered it
    /// see-through.
    #[test]
    fn object_material_ignores_an_additive_overlay_pass() {
        let asset_cache = cache(&[
            (
                "txt16/nd-atek.mtl",
                b"include ../../materials/ND-viewmodel.inc
render_pass
{
 blend SRC_ALPHA ONE
 texture obj/txt16/nd-atek_S
 shaded 1
}
",
            ),
            ("txt16/nd-atek.dds", b"diffuse"),
            ("txt16/nd-atek_s.dds", b"specular map"),
        ]);

        assert_eq!(
            resolve_object_material_texture_name(&asset_cache, "ND-atek"),
            Some("txt16/nd-atek.dds".to_owned())
        );
    }

    /// `blend DST_COLOR ZERO` modulates what is already in the framebuffer, so
    /// it is an overlay too.
    #[test]
    fn object_material_ignores_a_modulate_overlay_pass() {
        let asset_cache = cache(&[
            (
                "txt16/panel.mtl",
                b"render_pass
{
 blend DST_COLOR ZERO
 texture OBJ\\TXT16\\shine
}
",
            ),
            ("txt16/panel.dds", b"diffuse"),
            ("txt16/shine.dds", b"shine"),
        ]);

        assert_eq!(
            resolve_object_material_texture_name(&asset_cache, "PANEL.PCX"),
            Some("txt16/panel.dds".to_owned())
        );
    }

    /// An overlay first, then a real base pass: the base pass wins.
    #[test]
    fn object_material_takes_the_first_base_pass_after_an_overlay() {
        let asset_cache = cache(&[
            (
                "txt16/screen.mtl",
                b"render_pass
{
 blend SRC_ALPHA ONE
 texture OBJ\\TXT16\\glow
}
render_pass
{
 blend SRC_ALPHA INV_SRC_ALPHA
 texture OBJ\\TXT16\\screen_2
}
",
            ),
            ("txt16/screen.dds", b"placeholder"),
            ("txt16/glow.dds", b"glow"),
            ("txt16/screen_2.dds", b"real screen"),
        ]);

        assert_eq!(
            resolve_object_material_texture_name(&asset_cache, "SCREEN"),
            Some("txt16/screen_2.dds".to_owned())
        );
    }

    /// `ONE ZERO` is plain opaque replace - the most base-like blend there is.
    /// Rejecting it as a "modulate" overlay confuses the destination factor
    /// with `DST_COLOR ZERO`.
    #[test]
    fn object_material_accepts_an_opaque_replace_pass() {
        let asset_cache = cache(&[
            (
                "txt16/panel.mtl",
                b"render_pass\n{\n blend ONE ZERO\n texture OBJ\\TXT16\\panel_2\n}\n",
            ),
            ("txt16/panel.dds", b"placeholder"),
            ("txt16/panel_2.dds", b"real panel"),
        ]);

        assert_eq!(
            resolve_object_material_texture_name(&asset_cache, "PANEL.PCX"),
            Some("txt16/panel_2.dds".to_owned())
        );
    }

    /// Within one pass the first `texture` is the diffuse; later ones are
    /// additional stages and must not override it.
    #[test]
    fn object_material_takes_the_first_texture_stage_of_a_pass() {
        let asset_cache = cache(&[
            (
                "txt16/panel.mtl",
                b"render_pass\n{\n texture OBJ\\TXT16\\diffuse\n texture OBJ\\TXT16\\lightmap\n}\n",
            ),
            ("txt16/panel.dds", b"placeholder"),
            ("txt16/diffuse.dds", b"diffuse"),
            ("txt16/lightmap.dds", b"lightmap"),
        ]);

        assert_eq!(
            resolve_object_material_texture_name(&asset_cache, "PANEL.PCX"),
            Some("txt16/diffuse.dds".to_owned())
        );
    }

    /// A `blend` we cannot read must not be optimistically treated as a base
    /// pass - that would reinstate the overlay-as-diffuse bug.
    #[test]
    fn object_material_treats_an_unreadable_blend_as_an_overlay() {
        let asset_cache = cache(&[
            (
                "txt16/panel.mtl",
                b"render_pass\n{\n blend add\n texture OBJ\\TXT16\\glow\n}\n",
            ),
            ("txt16/panel.dds", b"diffuse"),
            ("txt16/glow.dds", b"glow"),
        ]);

        assert_eq!(
            resolve_object_material_texture_name(&asset_cache, "PANEL.PCX"),
            Some("txt16/panel.dds".to_owned())
        );
    }

    /// One truncated directive must not discard every later pass.
    #[test]
    fn object_material_survives_a_malformed_texture_directive() {
        let asset_cache = cache(&[
            (
                "txt16/screen.mtl",
                b"render_pass\n{\n texture\n}\nrender_pass\n{\n texture OBJ\\TXT16\\screen_2\n}\n",
            ),
            ("txt16/screen.dds", b"placeholder"),
            ("txt16/screen_2.dds", b"real screen"),
        ]);

        assert_eq!(
            resolve_object_material_texture_name(&asset_cache, "SCREEN"),
            Some("txt16/screen_2.dds".to_owned())
        );
    }

    /// An unterminated block must not bleed its fields into the next pass.
    #[test]
    fn object_material_does_not_merge_an_unterminated_pass_into_the_next() {
        let asset_cache = cache(&[
            (
                "txt16/screen.mtl",
                b"render_pass\n texture OBJ\\TXT16\\orphan\nrender_pass\n{\n blend SRC_ALPHA ONE\n texture OBJ\\TXT16\\glow\n}\n",
            ),
            ("txt16/screen.dds", b"authored"),
            ("txt16/orphan.dds", b"orphan"),
            ("txt16/glow.dds", b"glow"),
        ]);

        assert_eq!(
            resolve_object_material_texture_name(&asset_cache, "SCREEN"),
            Some("txt16/screen.dds".to_owned())
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

#[cfg(test)]
mod additive_tests {
    use super::*;
    #[test]
    fn additive_flash_requires_one_authored_unlit_pass() {
        let flash = "render_material_only 1\nrender_pass\n{\nblend SRC_COLOR ONE\ntexture $TEXTURE\nshaded 0\n}\n";
        assert!(is_additive_flash_script(flash));
        assert!(!is_additive_flash_script(
            &flash.replace("shaded 0", "shaded 1")
        ));
        assert!(!is_additive_flash_script(
            &flash.replace("SRC_COLOR ONE", "ONE ZERO")
        ));
        assert!(!is_additive_flash_script(
            &flash.replace("$TEXTURE", "specular")
        ));
        assert!(!is_additive_flash_script(&format!("{flash}{flash}")));
        assert!(!is_additive_flash_script(&flash.replace('}', "")));
    }
}
