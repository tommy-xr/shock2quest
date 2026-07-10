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

/// How a canvas maps onto a target when their aspect ratios differ.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScaleMode {
    /// Fill the target, stretching each axis independently (may distort).
    Stretch,
    /// Uniform scale that fits the canvas inside the target, centered, leaving
    /// empty bars on the longer axis (letterbox / pillarbox).
    PreserveAspect,
}

/// Canvas->target mapping such that `target_px = canvas_px * scale + offset`.
fn fit(
    canvas: Vector2<f32>,
    target: Vector2<f32>,
    mode: ScaleMode,
) -> (Vector2<f32>, Vector2<f32>) {
    match mode {
        ScaleMode::Stretch => (
            vec2(target.x / canvas.x, target.y / canvas.y),
            vec2(0.0, 0.0),
        ),
        ScaleMode::PreserveAspect => {
            let s = (target.x / canvas.x).min(target.y / canvas.y);
            let offset = vec2(
                (target.x - canvas.x * s) / 2.0,
                (target.y - canvas.y * s) / 2.0,
            );
            (vec2(s, s), offset)
        }
    }
}

/// Map a normalized pointer (`InputContext::pointer`, `[0,1]`) to canvas pixels
/// for a `canvas_size` canvas shown on `screen_size` under `mode`. Returns
/// `None` when the pointer falls in the letterbox bars (outside the canvas).
pub fn pointer_to_canvas(
    canvas_size: Vector2<f32>,
    normalized: Vector2<f32>,
    screen_size: Vector2<f32>,
    mode: ScaleMode,
) -> Option<Vector2<f32>> {
    let (scale, offset) = fit(canvas_size, screen_size, mode);
    let screen = vec2(normalized.x * screen_size.x, normalized.y * screen_size.y);
    let canvas = vec2(
        (screen.x - offset.x) / scale.x,
        (screen.y - offset.y) / scale.y,
    );
    if canvas.x < 0.0 || canvas.y < 0.0 || canvas.x > canvas_size.x || canvas.y > canvas_size.y {
        None
    } else {
        Some(canvas)
    }
}

/// Map a canvas-pixel rect to normalized screen coordinates (`[0,1]` per
/// axis, origin top-left) for a `canvas_size` canvas shown on `screen_size`
/// under `mode` - the rect analogue (and inverse) of [`pointer_to_canvas`].
/// Lets clients (e.g. the debug runtime's `GET /v1/ui`) aim a normalized
/// pointer at a canvas rect without re-deriving the letterbox math.
pub fn canvas_rect_to_screen(
    rect: Rect,
    canvas_size: Vector2<f32>,
    screen_size: Vector2<f32>,
    mode: ScaleMode,
) -> Rect {
    let (scale, offset) = fit(canvas_size, screen_size, mode);
    Rect::new(
        (rect.x * scale.x + offset.x) / screen_size.x,
        (rect.y * scale.y + offset.y) / screen_size.y,
        rect.w * scale.x / screen_size.x,
        rect.h * scale.y / screen_size.y,
    )
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

    /// Render the canvas as a screen-space overlay on `screen_size`, mapped via
    /// `mode` (stretch-to-fill or aspect-preserving letterbox).
    pub fn render_screen_space(
        &self,
        asset_cache: &mut AssetCache,
        screen_size: Vector2<f32>,
        mode: ScaleMode,
    ) -> Vec<SceneObject> {
        let (scale, offset) = fit(self.size, screen_size, mode);
        let texture_options = TextureOptions {
            wrap: false,
            ..Default::default()
        };
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
                        vec2(rect.x * scale.x + offset.x, rect.y * scale.y + offset.y),
                        vec2(rect.w * scale.x, rect.h * scale.y),
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
                        vec2(rect.x * scale.x + offset.x, rect.y * scale.y + offset.y),
                        vec2(rect.w * scale.x, rect.h * scale.y),
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
                    let font_size = size * scale.y;
                    let width = measure_text_width(&**font_obj, text, font_size);

                    let rx = rect.x * scale.x + offset.x;
                    let ry = rect.y * scale.y + offset.y;
                    let rw = rect.w * scale.x;
                    let rh = rect.h * scale.y;
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
    fn stretch_maps_pointer_independent_of_screen_size() {
        // Stretch: normalized maps straight to the canvas regardless of screen.
        let canvas = vec2(640.0, 480.0);
        let p = pointer_to_canvas(
            canvas,
            vec2(0.5, 0.5),
            vec2(1920.0, 1080.0),
            ScaleMode::Stretch,
        );
        assert_eq!(p, Some(vec2(320.0, 240.0)));
    }

    #[test]
    fn preserve_aspect_letterboxes_and_centers() {
        // 640x480 (4:3) canvas in a 1280x480 (wider) target: uniform scale 1.0,
        // 320px pillarbox bars on each side. Screen center maps to canvas center.
        let canvas = vec2(640.0, 480.0);
        let screen = vec2(1280.0, 480.0);
        let center = pointer_to_canvas(canvas, vec2(0.5, 0.5), screen, ScaleMode::PreserveAspect);
        assert_eq!(center, Some(vec2(320.0, 240.0)));

        // A point inside the left pillarbox bar is outside the canvas.
        let in_bar = pointer_to_canvas(canvas, vec2(0.1, 0.5), screen, ScaleMode::PreserveAspect);
        assert_eq!(in_bar, None);
    }

    #[test]
    fn canvas_rect_to_screen_letterboxes_and_roundtrips() {
        // 640x480 canvas pillarboxed on a 1280x480 screen: scale 1, 320px bars.
        let canvas = vec2(640.0, 480.0);
        let screen = vec2(1280.0, 480.0);
        let r = canvas_rect_to_screen(
            Rect::new(0.0, 0.0, 640.0, 480.0),
            canvas,
            screen,
            ScaleMode::PreserveAspect,
        );
        assert_eq!(r, Rect::new(0.25, 0.0, 0.5, 1.0));

        // Roundtrip: the normalized center of a mapped rect points back at the
        // canvas rect's center through pointer_to_canvas.
        let target = Rect::new(17.0, 166.0, 45.0, 60.0);
        let mapped = canvas_rect_to_screen(target, canvas, screen, ScaleMode::PreserveAspect);
        let back = pointer_to_canvas(canvas, mapped.center(), screen, ScaleMode::PreserveAspect)
            .expect("center should land inside the canvas");
        assert!((back.x - target.center().x).abs() < 1e-3);
        assert!((back.y - target.center().y).abs() < 1e-3);
    }

    #[test]
    fn preserve_aspect_is_stretch_when_aspect_matches() {
        // 4:3 canvas on a 4:3 screen: no bars, so it matches stretch.
        let canvas = vec2(640.0, 480.0);
        let p = pointer_to_canvas(
            canvas,
            vec2(0.5, 0.25),
            vec2(800.0, 600.0),
            ScaleMode::PreserveAspect,
        );
        assert_eq!(p, Some(vec2(320.0, 120.0)));
    }
}
