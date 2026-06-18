//! Placement-agnostic 2D UI canvas.
//!
//! UI is described in **canvas pixels** at a fixed virtual resolution (e.g.
//! 640x480, matching the original SS2 art). The content never encodes where it
//! is shown; a *renderer* maps the canvas to a target. Today that is a
//! screen-space overlay (`render_screen_space`); later the same canvas can be
//! rendered into a texture and placed on a world surface for diegetic VR (see
//! `projects/flatscreen-and-vr-architecture.md`).
//!
//! Alignment is resolved at render time (the renderer has the font, so it
//! measures text and centers it), which keeps layout out of hand-tuned
//! coordinates.

// Foundational UI toolkit: a few API items (the full alignment variants,
// geometry/introspection helpers) are intentionally complete ahead of their
// call sites - later flat screens (inventory, log, upgrade) exercise the rest.
#![allow(dead_code)]

use std::rc::Rc;

use cgmath::{Matrix4, Vector2, vec2, vec3};
use dark::importers::{FONT_IMPORTER, TEXTURE_IMPORTER};
use engine::{
    assets::asset_cache::AssetCache,
    measure_text_width,
    scene::SceneObject,
    texture::{TextureOptions, TextureTrait},
};

/// A rectangle in canvas pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }

    pub fn contains(&self, p: Vector2<f32>) -> bool {
        p.x >= self.x && p.y >= self.y && p.x <= self.x + self.w && p.y <= self.y + self.h
    }

    pub fn center(&self) -> Vector2<f32> {
        vec2(self.x + self.w / 2.0, self.y + self.h / 2.0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HAlign {
    Left,
    Center,
    Right,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VAlign {
    Top,
    Middle,
    Bottom,
}

enum UiElement {
    Image {
        rect: Rect,
        texture: String,
        opacity: f32,
    },
    /// Horizontally-filling bar; `fill` (0..1) clips the texture from the left.
    Bar {
        rect: Rect,
        texture: String,
        fill: f32,
        opacity: f32,
    },
    Text {
        rect: Rect,
        text: String,
        font: String,
        size: f32,
        h: HAlign,
        v: VAlign,
        opacity: f32,
    },
}

/// A resolution-independent 2D UI description in canvas pixels.
pub struct UiCanvas {
    size: Vector2<f32>,
    elements: Vec<UiElement>,
}

impl UiCanvas {
    pub fn new(size: Vector2<f32>) -> Self {
        Self {
            size,
            elements: Vec::new(),
        }
    }

    /// Convert a normalized pointer (`InputContext::pointer`, `[0,1]`) to canvas
    /// coordinates, so scenes can hit-test their `Rect`s against it.
    pub fn to_canvas(&self, normalized: Vector2<f32>) -> Vector2<f32> {
        vec2(normalized.x * self.size.x, normalized.y * self.size.y)
    }

    pub fn element_count(&self) -> usize {
        self.elements.len()
    }

    /// Set the opacity (0..1) of the most recently added element. Chains after a
    /// builder call, e.g. `canvas.text(...).opacity(0.6)`.
    pub fn opacity(&mut self, opacity: f32) -> &mut Self {
        if let Some(last) = self.elements.last_mut() {
            let o = opacity.clamp(0.0, 1.0);
            match last {
                UiElement::Image { opacity, .. }
                | UiElement::Bar { opacity, .. }
                | UiElement::Text { opacity, .. } => *opacity = o,
            }
        }
        self
    }

    pub fn image(&mut self, rect: Rect, texture: &str) -> &mut Self {
        self.elements.push(UiElement::Image {
            rect,
            texture: texture.to_owned(),
            opacity: 1.0,
        });
        self
    }

    pub fn bar(&mut self, rect: Rect, texture: &str, fill: f32) -> &mut Self {
        self.elements.push(UiElement::Bar {
            rect,
            texture: texture.to_owned(),
            fill: fill.clamp(0.0, 1.0),
            opacity: 1.0,
        });
        self
    }

    pub fn text(
        &mut self,
        rect: Rect,
        text: &str,
        font: &str,
        size: f32,
        h: HAlign,
        v: VAlign,
    ) -> &mut Self {
        self.elements.push(UiElement::Text {
            rect,
            text: text.to_owned(),
            font: font.to_owned(),
            size,
            h,
            v,
            opacity: 1.0,
        });
        self
    }

    /// Render the canvas as a screen-space overlay scaled to fill `screen_size`.
    pub fn render_screen_space(
        &self,
        asset_cache: &mut AssetCache,
        screen_size: Vector2<f32>,
    ) -> Vec<SceneObject> {
        let sx = screen_size.x / self.size.x;
        let sy = screen_size.y / self.size.y;
        let texture_options = TextureOptions { wrap: false };
        let mut objs = Vec::with_capacity(self.elements.len());

        for element in &self.elements {
            match element {
                UiElement::Image {
                    rect,
                    texture,
                    opacity,
                } => {
                    let tex = asset_cache.get_ext(&TEXTURE_IMPORTER, texture, &texture_options);
                    objs.push(SceneObject::screen_space_quad2(
                        tex.clone() as Rc<dyn TextureTrait>,
                        vec2(rect.x * sx, rect.y * sy),
                        vec2(rect.w * sx, rect.h * sy),
                        *opacity,
                    ));
                }
                UiElement::Bar {
                    rect,
                    texture,
                    fill,
                    opacity: _,
                } => {
                    let tex = asset_cache.get_ext(&TEXTURE_IMPORTER, texture, &texture_options);
                    let material = engine::scene::clipped_screen_material::create_screen_space(
                        tex.clone() as Rc<dyn TextureTrait>,
                        *fill,
                    );
                    let mut obj =
                        SceneObject::new(material, Box::new(engine::scene::quad::create()));
                    obj.set_local_transform(screen_space_quad_transform(
                        vec2(rect.x * sx, rect.y * sy),
                        vec2(rect.w * sx, rect.h * sy),
                    ));
                    objs.push(obj);
                }
                UiElement::Text {
                    rect,
                    text,
                    font,
                    size,
                    h,
                    v,
                    opacity,
                } => {
                    let font_obj = asset_cache.get(&FONT_IMPORTER, font).clone();
                    let font_size = size * sy;
                    let width = measure_text_width(&**font_obj, text, font_size);

                    let rx = rect.x * sx;
                    let ry = rect.y * sy;
                    let rw = rect.w * sx;
                    let rh = rect.h * sy;
                    let x = match h {
                        HAlign::Left => rx,
                        HAlign::Center => rx + (rw - width) / 2.0,
                        HAlign::Right => rx + rw - width,
                    };
                    let y = match v {
                        VAlign::Top => ry,
                        VAlign::Middle => ry + (rh - font_size) / 2.0,
                        VAlign::Bottom => ry + rh - font_size,
                    };

                    objs.push(SceneObject::screen_space_text(
                        text, font_obj, font_size, *opacity, x, y,
                    ));
                }
            }
        }

        objs
    }
}

/// Replicate `SceneObject::screen_space_quad`'s transform so a manually-built
/// screen-space object (e.g. the clipped bar material) maps to the same pixels.
fn screen_space_quad_transform(position: Vector2<f32>, size: Vector2<f32>) -> Matrix4<f32> {
    Matrix4::from_translation(vec3(position.x, position.y, 0.0))
        * Matrix4::from_nonuniform_scale(size.x, size.y, 1.0)
        * Matrix4::from_translation(vec3(0.5, 0.5, 0.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rect_contains() {
        let r = Rect::new(10.0, 20.0, 100.0, 40.0);
        assert!(r.contains(vec2(10.0, 20.0)));
        assert!(r.contains(vec2(60.0, 40.0)));
        assert!(r.contains(vec2(110.0, 60.0)));
        assert!(!r.contains(vec2(9.0, 40.0)));
        assert!(!r.contains(vec2(60.0, 61.0)));
    }

    #[test]
    fn to_canvas_scales_normalized_pointer() {
        let c = UiCanvas::new(vec2(640.0, 480.0));
        assert_eq!(c.to_canvas(vec2(0.5, 0.5)), vec2(320.0, 240.0));
        assert_eq!(c.to_canvas(vec2(1.0, 1.0)), vec2(640.0, 480.0));
    }
}
