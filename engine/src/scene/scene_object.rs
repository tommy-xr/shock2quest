extern crate gl;

use crate::Font;
use crate::render_log;
// pub trait SceneObject {
//     fn init(&self) -> ();
//     fn draw(&self) -> ();
//     fn destroy(&self) -> ();
// }
use crate::engine::EngineRenderContext;
use crate::texture::TextureTrait;
use cgmath::Matrix4;
use cgmath::Vector2;
use cgmath::prelude::*;
use cgmath::vec2;
use cgmath::vec3;
use cgmath::vec4;

pub use crate::scene::Geometry;
pub use crate::scene::Material;

use crate::gl_engine::OpenGLEngine;
use std::cell::RefCell;
use std::rc::Rc;

use super::TextVertex;
use super::basic_material;
use super::mesh;
use super::quad;
use super::skinned_material::SkinnedMaterial;
use crate::materials;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FrontFaceWinding {
    Clockwise,
    CounterClockwise,
}

/// The explicit composition layer for a scene object.
///
/// Hosts may gather the main scene and per-eye objects in either order. The
/// renderer therefore consumes this key, rather than vector position, to keep
/// view-locked effects and UI composited identically on every host.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum RenderLayer {
    /// Ordinary world geometry, rendered against the frame's initial depth.
    World,
    /// View-locked scene effects over the world but behind scene UI.
    SceneOverlay,
    /// Viewmodels, HUDs, MFDs, and other scene-owned per-eye UI.
    SceneUi,
    /// System-owned UI such as the pause menu and final screen fade.
    SystemOverlay,
}

impl RenderLayer {
    pub const ORDERED: [Self; 4] = [
        Self::World,
        Self::SceneOverlay,
        Self::SceneUi,
        Self::SystemOverlay,
    ];

    pub fn clears_depth(self) -> bool {
        self != Self::World
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::World => "world",
            Self::SceneOverlay => "scene_overlay",
            Self::SceneUi => "scene_ui",
            Self::SystemOverlay => "system_overlay",
        }
    }
}

/// Provenance for a scene object, so tooling can report *what* the renderer was
/// handed. Never read by the renderer itself; a game layer may filter its own
/// scene by `source` before submitting it (shock2vr drops the player's hands
/// while its pause menu is up).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SceneObjectDebugTag {
    /// Runtime entity this object was built for, when it came from one.
    pub entity_id: Option<u64>,
    /// The entity's authored name, when it has one.
    pub name: Option<String>,
    /// Model the geometry was loaded from (`PropModelName`).
    pub model: Option<String>,
    /// Which render path produced the object, e.g. "entity".
    pub source: Option<String>,
}

#[derive(Clone)]
pub struct SceneObject {
    pub material: Rc<RefCell<Box<dyn Material>>>,
    pub geometry: Rc<Box<dyn Geometry>>,
    pub transform: Matrix4<f32>,
    pub local_transform: Matrix4<f32>, //hack...
    pub skinning_data: [Matrix4<f32>; crate::scene::SKINNING_PALETTE_SIZE],
    pub depth_write: bool,
    render_layer: RenderLayer,
    /// Per-object transparency override (0.0 = opaque, 1.0 = invisible).
    /// Materials are shared (`Rc`) across every object using the same model, so
    /// a lasting material-level override would bleed between entities; instead
    /// this is applied to the material only around this object's own draw.
    pub transparency_override: Option<f32>,
    /// Front-face winding used to cull backfaces for this object. Most engine
    /// geometry remains double-sided; imported Dark models opt in explicitly.
    backface_culling: Option<FrontFaceWinding>,
    /// Pull this object's depth slightly toward the camera (polygon offset).
    /// Used for flat decal-like models (floor/wall signs) that sit coplanar
    /// with world geometry and would otherwise z-fight it.
    depth_bias: bool,
    /// Debug-only provenance; `Rc` so cloning an object per frame stays cheap.
    debug_tag: Option<Rc<SceneObjectDebugTag>>,
}

impl SceneObject {
    pub fn screen_space_quad2(
        texture: Rc<dyn TextureTrait>,
        position: Vector2<f32>,
        size: Vector2<f32>,
        opacity: f32,
    ) -> SceneObject {
        let mesh = quad::create();
        let material =
            materials::ScreenSpaceMaterial::create(texture, vec4(1.0, 1.0, 1.0, opacity));

        let xform = Matrix4::from_translation(vec3(position.x, position.y, 0.0))
            * Matrix4::from_nonuniform_scale(size.x, size.y, 1.0)
            * Matrix4::from_translation(vec3(0.5, 0.5, 0.0));
        let mut ret = Self::new(material, Box::new(mesh));
        ret.set_local_transform(xform);
        ret
    }
    pub fn screen_space_quad(
        texture: Rc<dyn TextureTrait>,
        position: Vector2<f32>,
        size: Vector2<f32>,
    ) -> SceneObject {
        let mesh = quad::create();
        let material = materials::ScreenSpaceMaterial::create(texture, vec4(1.0, 1.0, 1.0, 1.0));

        let xform = Matrix4::from_translation(vec3(position.x, position.y, 0.0))
            * Matrix4::from_nonuniform_scale(size.x, size.y, 1.0)
            * Matrix4::from_translation(vec3(0.5, 0.5, 0.0));
        let mut ret = Self::new(material, Box::new(mesh));
        ret.set_local_transform(xform);
        ret
    }
    /// A screen-space quad whose texture is *clipped* at `clip` (0..1) of its
    /// width rather than scaled to it: the bitmap draws at `size` and
    /// everything past `clip` is discarded. This is what a fill bar wants -
    /// the artwork's right-hand border disappears as the bar drains instead of
    /// sliding inwards.
    pub fn screen_space_clipped_quad(
        texture: Rc<dyn TextureTrait>,
        position: Vector2<f32>,
        size: Vector2<f32>,
        clip: f32,
    ) -> SceneObject {
        let material = crate::scene::clipped_screen_material::create_screen_space(texture, clip);
        let xform = Matrix4::from_translation(vec3(position.x, position.y, 0.0))
            * Matrix4::from_nonuniform_scale(size.x, size.y, 1.0)
            * Matrix4::from_translation(vec3(0.5, 0.5, 0.0));
        let mut ret = Self::new(material, Box::new(quad::create()));
        ret.set_local_transform(xform);
        ret
    }

    /// Glyph quads for `str` at `font_size`, laid out from `origin` as the
    /// **top-left of the line**, x growing right and y growing down.
    ///
    /// This is the single text-layout routine: screen space consumes it
    /// directly, and the world-space panel path reproduces the same 2D
    /// convention through its own mapping, so a string cannot be laid out two
    /// different ways depending on where it is shown. Advances are exactly the
    /// `.FON` offset-table column differences (side bearings are baked into
    /// the glyph cells, nothing is added between glyphs), which is also what
    /// [`measure_text_width`](crate::measure_text_width) sums.
    fn text_vertices(
        str: &str,
        font: &dyn Font,
        font_size: f32,
        origin: Vector2<f32>,
    ) -> Vec<TextVertex> {
        let multiplier = font_size / font.base_height();
        let adj_height = font_size;

        let mut x = origin.x;
        let y = origin.y;

        let mut vertices = Vec::new();
        for c in str.chars() {
            let Some(a_info) = font.get_character_info(c) else {
                continue;
            };
            let min_uv_x = a_info.min_uv_x;
            let max_uv_x = a_info.max_uv_x;
            // y grows downward here, so the glyph's top row of texels belongs
            // to the smaller y - the uv rows are swapped relative to the
            // font's own (y-up) ordering.
            let min_uv_y = a_info.max_uv_y;
            let max_uv_y = a_info.min_uv_y;

            let adj_width = a_info.advance * multiplier;

            vertices.extend(vec![
                TextVertex {
                    position: vec2(x, y),
                    uv: vec2(min_uv_x, max_uv_y),
                },
                TextVertex {
                    position: vec2(x, y + adj_height),
                    uv: vec2(min_uv_x, min_uv_y),
                },
                TextVertex {
                    position: vec2(x + adj_width, y + adj_height),
                    uv: vec2(max_uv_x, min_uv_y),
                },
                TextVertex {
                    position: vec2(x, y),
                    uv: vec2(min_uv_x, max_uv_y),
                },
                TextVertex {
                    position: vec2(x + adj_width, y + adj_height),
                    uv: vec2(max_uv_x, min_uv_y),
                },
                TextVertex {
                    position: vec2(x + adj_width, y),
                    uv: vec2(max_uv_x, max_uv_y),
                },
            ]);

            x += adj_width;
        }
        vertices
    }

    /// [`Self::text_vertices`] normalized so the string's glyph box is the
    /// centered unit square: x in -0.5..0.5 across the whole string, y in
    /// -0.5..0.5 across the line height, y still growing downward.
    ///
    /// Empty (or entirely unmappable) text has no box to normalize into and
    /// yields no vertices.
    fn unit_text_vertices(str: &str, font: &dyn Font) -> Vec<TextVertex> {
        let width = crate::measure_text_width(font, str, 1.0);
        if width <= 0.0 {
            return Vec::new();
        }
        let mut vertices = Self::text_vertices(str, font, 1.0, vec2(0.0, 0.0));
        for vertex in &mut vertices {
            vertex.position = vec2(vertex.position.x / width - 0.5, vertex.position.y - 0.5);
        }
        vertices
    }

    /// Text drawn in screen pixels, anchored at the **top-left** of its glyph
    /// box (`in_x`, `in_y`) with a line height of `font_size`.
    pub fn screen_space_text(
        str: &str,
        font: Rc<Box<dyn Font>>,
        font_size: f32,
        transparency: f32,
        in_x: f32,
        in_y: f32,
    ) -> SceneObject {
        render_log!(DEBUG, "screen-space-text: |{}|{}", str, str.len());
        let vertices = Self::text_vertices(str, &**font, font_size, vec2(in_x, in_y));
        let mesh = mesh::create(vertices);
        let material = materials::ScreenSpaceMaterial::create(
            font.get_texture().clone(),
            vec4(1.0, 1.0, 1.0, transparency),
        );
        Self::new(material, Box::new(mesh))
    }

    /// Text as a **unit quad's worth of geometry**: the string's glyph box is
    /// normalized to the centered unit square (-0.5..0.5 on both axes) with y
    /// growing downward, exactly like [`quad::create`](crate::scene::quad).
    ///
    /// That is what makes one placement rule enough for a world-space UI
    /// panel: text and images are both "fill this rect", so a caller maps a
    /// laid-out rect onto the panel with the same transform for either, and
    /// the two cannot drift apart. The glyph metrics are
    /// [`Self::text_vertices`]', i.e. identical to screen space.
    ///
    /// An empty string (or one whose glyphs are all missing) has no box to
    /// normalize into, so it produces an empty mesh.
    pub fn world_space_text(str: &str, font: Rc<Box<dyn Font>>, transparency: f32) -> SceneObject {
        let mesh = mesh::create(Self::unit_text_vertices(str, &**font));
        let material = basic_material::create(font.get_texture(), 1.0, transparency);
        Self::new(material, Box::new(mesh))
    }

    pub fn create(
        material: RefCell<Box<dyn Material>>,
        geometry: Rc<Box<dyn Geometry>>,
    ) -> SceneObject {
        let transform: Matrix4<f32> = Matrix4::identity();
        SceneObject {
            material: Rc::new(material),
            geometry,
            transform,
            local_transform: Matrix4::identity(),
            skinning_data: [Matrix4::identity(); crate::scene::SKINNING_PALETTE_SIZE],
            depth_write: true,
            render_layer: RenderLayer::World,
            transparency_override: None,
            debug_tag: None,
            backface_culling: None,
            depth_bias: false,
        }
    }

    pub fn draw_opaque(
        &self,
        engine_context: &OpenGLEngine,
        render_context: &EngineRenderContext,
        view: &Matrix4<f32>,
        lights: &crate::scene::light::LightArray,
    ) {
        if !self.material.borrow().has_initialized() {
            self.material
                .borrow_mut()
                .initialize(engine_context.is_opengl_es);
        }

        let xform = self.transform * self.local_transform;
        if !self.depth_write {
            unsafe { gl::DepthMask(gl::FALSE) };
        }

        if let Some(t) = self.transparency_override {
            self.material
                .borrow_mut()
                .set_transparency_override(Some(t));
        }
        if self.material.borrow().draw_opaque(
            render_context,
            view,
            &xform,
            &self.skinning_data,
            lights,
        ) {
            self.draw_geometry(true);
        }
        if self.transparency_override.is_some() {
            self.material.borrow_mut().set_transparency_override(None);
        }

        if !self.depth_write {
            unsafe { gl::DepthMask(gl::TRUE) };
        }
    }
    pub fn draw_transparent(
        &self,
        _engine_context: &OpenGLEngine,
        render_context: &EngineRenderContext,
        view: &Matrix4<f32>,
        lights: &crate::scene::light::LightArray,
    ) {
        let xform = self.transform * self.local_transform;
        if let Some(t) = self.transparency_override {
            self.material
                .borrow_mut()
                .set_transparency_override(Some(t));
        }
        if self.material.borrow().draw_transparent(
            render_context,
            view,
            &xform,
            &self.skinning_data,
            lights,
        ) {
            self.draw_geometry(false);
        }
        if self.transparency_override.is_some() {
            self.material.borrow_mut().set_transparency_override(None);
        }
    }

    /// Get the world position of this scene object from its transform matrix
    pub fn get_world_position(&self) -> cgmath::Vector3<f32> {
        let final_transform = self.transform * self.local_transform;
        cgmath::Vector3::new(
            final_transform[3][0],
            final_transform[3][1],
            final_transform[3][2],
        )
    }
    pub fn set_transform(&mut self, transform: Matrix4<f32>) {
        self.transform = transform;
    }

    pub fn set_local_transform(&mut self, transform: Matrix4<f32>) {
        self.local_transform = transform;
    }

    /// Set the 40 joint matrices; the parent-frame slots (40..80) are
    /// filled with each joint's own transform, so a stretchy second-bone
    /// reference degrades to the rigid single-bone result. Callers with real
    /// parent frames use [`set_skinning_palette`](Self::set_skinning_palette).
    pub fn set_skinning_data(
        &mut self,
        skinning_data: [Matrix4<f32>; crate::scene::MAX_SKINNED_JOINTS],
    ) {
        let half = crate::scene::MAX_SKINNED_JOINTS;
        self.skinning_data[..half].copy_from_slice(&skinning_data);
        self.skinning_data[half..].copy_from_slice(&skinning_data);
    }

    /// Set the full 80-slot palette: joint transforms in 0..40, per-joint
    /// parent frames (parent orientation about the joint's position) in
    /// 40..80, blended by stretchy vertices.
    pub fn set_skinning_palette(
        &mut self,
        palette: [Matrix4<f32>; crate::scene::SKINNING_PALETTE_SIZE],
    ) {
        self.skinning_data = palette;
    }

    pub fn get_transform(&self) -> Matrix4<f32> {
        self.transform
    }

    pub fn new(material: Box<dyn Material>, geometry: Box<dyn Geometry>) -> SceneObject {
        SceneObject {
            material: Rc::new(RefCell::new(material)),
            geometry: Rc::new(geometry),
            transform: Matrix4::identity(),
            local_transform: Matrix4::identity(),
            skinning_data: [Matrix4::identity(); crate::scene::SKINNING_PALETTE_SIZE],
            depth_write: true,
            render_layer: RenderLayer::World,
            transparency_override: None,
            debug_tag: None,
            backface_culling: None,
            depth_bias: false,
        }
    }

    pub fn duplicate(&self) -> SceneObject {
        SceneObject {
            material: self.material.clone(),
            geometry: self.geometry.clone(),
            transform: self.transform,
            local_transform: self.local_transform,
            skinning_data: self.skinning_data,
            depth_write: self.depth_write,
            render_layer: self.render_layer,
            transparency_override: self.transparency_override,
            debug_tag: self.debug_tag.clone(),
            backface_culling: self.backface_culling,
            depth_bias: self.depth_bias,
        }
    }

    pub fn set_depth_write(&mut self, enabled: bool) {
        self.depth_write = enabled;
    }

    pub fn set_render_layer(&mut self, layer: RenderLayer) {
        self.render_layer = layer;
    }

    pub fn render_layer(&self) -> RenderLayer {
        self.render_layer
    }

    pub fn set_backface_culling(&mut self, front_face: Option<FrontFaceWinding>) {
        self.backface_culling = front_face;
    }

    /// Attach debug provenance (see [`SceneObjectDebugTag`]).
    pub fn set_debug_tag(&mut self, tag: Option<Rc<SceneObjectDebugTag>>) {
        self.debug_tag = tag;
    }

    pub fn debug_tag(&self) -> Option<&SceneObjectDebugTag> {
        self.debug_tag.as_deref()
    }

    /// The transparency in effect for this draw: the per-object override when
    /// set, otherwise the material's own value.
    pub fn effective_transparency(&self) -> Option<f32> {
        self.transparency_override
            .or_else(|| self.material.borrow().transparency())
    }

    pub fn backface_culling(&self) -> Option<FrontFaceWinding> {
        self.backface_culling
    }

    /// Pull this object's depth slightly toward the camera so it wins the
    /// depth test against coplanar world geometry instead of z-fighting it.
    pub fn set_depth_bias(&mut self, enabled: bool) {
        self.depth_bias = enabled;
    }

    pub fn depth_bias(&self) -> bool {
        self.depth_bias
    }

    /// `apply_depth_bias` is false on the transparent pass: the bias exists
    /// for opaque coplanar decals, and translucent flats (membranes, glass)
    /// were never verified with an offset applied.
    fn draw_geometry(&self, apply_depth_bias: bool) {
        let depth_bias = apply_depth_bias && self.depth_bias;
        if depth_bias {
            unsafe {
                gl::Enable(gl::POLYGON_OFFSET_FILL);
                gl::PolygonOffset(-1.0, -2.0);
            }
        }
        if let Some(front_face) = self.backface_culling {
            unsafe {
                gl::Enable(gl::CULL_FACE);
                gl::CullFace(gl::BACK);
                gl::FrontFace(match front_face {
                    FrontFaceWinding::Clockwise => gl::CW,
                    FrontFaceWinding::CounterClockwise => gl::CCW,
                });
            }
        }

        self.geometry.draw();

        if self.backface_culling.is_some() {
            // Culling is an explicit per-object opt-in; restore the default for
            // world, procedural, and UI geometry drawn afterward.
            unsafe {
                gl::Disable(gl::CULL_FACE);
                gl::FrontFace(gl::CCW);
            }
        }

        if depth_bias {
            unsafe {
                gl::PolygonOffset(0.0, 0.0);
                gl::Disable(gl::POLYGON_OFFSET_FILL);
            }
        }
    }

    pub fn set_skinned_transparency(&mut self, transparency: Option<f32>) {
        if let Some(material) = self
            .material
            .borrow_mut()
            .as_any_mut()
            .downcast_mut::<SkinnedMaterial>()
        {
            match transparency {
                Some(value) => material.set_transparency_override(value),
                None => material.reset_transparency(),
            }
        }
    }

    /// Override transparency (0.0 = opaque, 1.0 = invisible) for this object
    /// only, or reset with `None`. Applied around this object's draw so other
    /// objects sharing the same material are unaffected.
    pub fn set_transparency(&mut self, transparency: Option<f32>) {
        self.transparency_override = transparency;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordinary_scene_objects_are_double_sided_by_default() {
        let object = SceneObject::new(
            super::super::color_material::create(vec3(1.0, 1.0, 1.0)),
            Box::new(super::super::geometry::EmptyMesh),
        );

        assert_eq!(object.backface_culling(), None);
    }

    #[test]
    fn scene_objects_are_untagged_until_a_debug_tag_is_attached() {
        let mut object = SceneObject::new(
            super::super::color_material::create(vec3(1.0, 1.0, 1.0)),
            Box::new(super::super::geometry::EmptyMesh),
        );

        assert_eq!(object.debug_tag(), None);

        let tag = SceneObjectDebugTag {
            entity_id: Some(246),
            name: Some("Pistol".to_owned()),
            model: Some("atek_w".to_owned()),
            source: Some("entity".to_owned()),
        };
        object.set_debug_tag(Some(Rc::new(tag.clone())));

        assert_eq!(object.debug_tag(), Some(&tag));
        // The tag has to survive the per-frame clone the render path makes.
        assert_eq!(object.clone().debug_tag(), Some(&tag));
    }

    #[test]
    fn a_per_object_override_wins_over_the_shared_material_transparency() {
        let mut object = SceneObject::new(
            super::super::color_material::create(vec3(1.0, 1.0, 1.0)),
            Box::new(super::super::geometry::EmptyMesh),
        );

        // color_material reports no transparency of its own.
        assert_eq!(object.effective_transparency(), None);

        object.set_transparency(Some(0.35));

        assert_eq!(object.effective_transparency(), Some(0.35));
    }
}

#[cfg(test)]
mod text_layout_tests {
    use super::*;
    use crate::font::FontCharacterInfo;

    /// Fixed-metrics stub font: every glyph is `advance` wide at `base_height`.
    struct StubFont;

    impl Font for StubFont {
        fn get_texture(&self) -> Rc<dyn TextureTrait> {
            unreachable!("text layout does not touch the texture")
        }
        fn get_character_info(&self, c: char) -> Option<FontCharacterInfo> {
            // '?' stands in for a glyph the font does not have.
            if c == '?' {
                return None;
            }
            Some(FontCharacterInfo {
                min_uv_x: 0.0,
                min_uv_y: 0.0,
                max_uv_x: 1.0,
                max_uv_y: 1.0,
                advance: 4.0,
            })
        }
        fn base_height(&self) -> f32 {
            10.0
        }
        fn get_half_pixel(&self) -> f32 {
            0.0
        }
    }

    fn bounds(vertices: &[TextVertex]) -> (f32, f32, f32, f32) {
        vertices.iter().fold(
            (f32::MAX, f32::MAX, f32::MIN, f32::MIN),
            |(min_x, min_y, max_x, max_y), v| {
                (
                    min_x.min(v.position.x),
                    min_y.min(v.position.y),
                    max_x.max(v.position.x),
                    max_y.max(v.position.y),
                )
            },
        )
    }

    /// Screen-space text grows to the right and DOWN from its anchor, so the
    /// anchor is the top-left of the glyph box and its height is the font size.
    #[test]
    fn text_lays_out_right_and_down_from_its_top_left() {
        let v = SceneObject::text_vertices("abc", &StubFont, 20.0, vec2(100.0, 50.0));
        // advance 4 at base_height 10 => 8 per glyph at font_size 20.
        assert_eq!(bounds(&v), (100.0, 50.0, 124.0, 70.0));
    }

    /// The world-space mesh is the same layout normalized into the centered
    /// unit square - the shape `quad::create` has - which is what lets a
    /// world-space panel place text with the exact same transform it uses for
    /// an image, instead of a text-only anchoring rule that can drift.
    #[test]
    fn world_text_normalizes_to_the_centered_unit_square() {
        let (min_x, min_y, max_x, max_y) =
            bounds(&SceneObject::unit_text_vertices("abc", &StubFont));
        assert!((min_x + 0.5).abs() < 1e-6, "left edge: {min_x}");
        assert!((min_y + 0.5).abs() < 1e-6, "top edge: {min_y}");
        assert!((max_x - 0.5).abs() < 1e-6, "right edge: {max_x}");
        assert!((max_y - 0.5).abs() < 1e-6, "bottom edge: {max_y}");

        // Longer text still fills exactly one box (it is the placed rect that
        // gets narrower or wider, never the normalization).
        let (min_x, _, max_x, _) = bounds(&SceneObject::unit_text_vertices("abcdefgh", &StubFont));
        assert!((min_x + 0.5).abs() < 1e-6);
        assert!((max_x - 0.5).abs() < 1e-6);
    }

    /// Nothing to draw, and above all no division by a zero-width box.
    #[test]
    fn world_text_with_no_measurable_glyphs_is_empty() {
        assert!(SceneObject::unit_text_vertices("", &StubFont).is_empty());
        assert!(SceneObject::unit_text_vertices("???", &StubFont).is_empty());
    }
}
