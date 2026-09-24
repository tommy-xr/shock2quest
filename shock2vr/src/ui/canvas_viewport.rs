//! Windows onto an already-laid-out canvas, re-mapped onto another surface.
//!
//! A presentation that shows only part of a shared canvas (the handheld MFD
//! device shows the MFD slot and a strip of the bottom bar) maps each window's
//! resolved rects into its own space. It makes no placement decision: element
//! positions still come from the shared layout, so flat and VR cannot drift.
use super::{ImageKind, PlacedContent, PlacedElement, Rect};
use cgmath::{Vector2, vec2};

/// `src` (shared-canvas pixels) drawn into `dst` (target-surface pixels).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CanvasViewport {
    pub src: Rect,
    pub dst: Rect,
}

impl CanvasViewport {
    /// `src` scaled uniformly to fit inside `dst`, centered - e.g. a tall
    /// 188x296 MFD panel in a squarer screen letterboxes left and right.
    pub fn fit(src: Rect, dst: Rect) -> Self {
        let (scale, offset) = super::fit(
            vec2(src.w, src.h),
            vec2(dst.w, dst.h),
            super::ScaleMode::PreserveAspect,
        );
        Self {
            src,
            dst: Rect::new(
                dst.x + offset.x,
                dst.y + offset.y,
                src.w * scale.x,
                src.h * scale.y,
            ),
        }
    }

    fn scale(&self) -> Vector2<f32> {
        vec2(self.dst.w / self.src.w, self.dst.h / self.src.h)
    }

    fn to_dst(&self, r: Rect) -> Rect {
        let s = self.scale();
        Rect::new(
            self.dst.x + (r.x - self.src.x) * s.x,
            self.dst.y + (r.y - self.src.y) * s.y,
            r.w * s.x,
            r.h * s.y,
        )
    }

    /// Target-surface point -> shared-canvas point, when it lands in `dst`.
    pub fn to_src_point(&self, p: Vector2<f32>) -> Option<Vector2<f32>> {
        self.dst.contains(p).then(|| {
            let s = self.scale();
            vec2(
                self.src.x + (p.x - self.dst.x) / s.x,
                self.src.y + (p.y - self.dst.y) / s.y,
            )
        })
    }
}

/// The first viewport containing a target-surface point, mapped back into
/// shared-canvas pixels: the device's pointer, fed to the same host the mouse
/// drives.
pub fn target_to_canvas(viewports: &[CanvasViewport], p: Vector2<f32>) -> Option<Vector2<f32>> {
    viewports.iter().find_map(|v| v.to_src_point(p))
}

/// Re-map placed elements through `viewports`, in paint order per viewport.
///
/// Stretchable art straddling a window edge is cropped (its UVs follow);
/// text and object icons, which cannot be cropped cleanly, are kept only when
/// their center falls inside the window.
pub fn place_through(placed: &[PlacedElement], viewports: &[CanvasViewport]) -> Vec<PlacedElement> {
    let mut out = Vec::new();
    for viewport in viewports {
        for element in placed {
            if let Some(clipped) = clip(element, viewport.src) {
                out.push(PlacedElement {
                    rect: viewport.to_dst(clipped.rect),
                    ..clipped
                });
            }
        }
    }
    out
}

fn clip(element: &PlacedElement, window: Rect) -> Option<PlacedElement> {
    let r = element.rect;
    let (x0, y0) = (r.x.max(window.x), r.y.max(window.y));
    let (x1, y1) = (
        (r.x + r.w).min(window.x + window.w),
        (r.y + r.h).min(window.y + window.h),
    );
    if x1 <= x0 || y1 <= y0 {
        return None;
    }
    if x0 == r.x && y0 == r.y && x1 == r.x + r.w && y1 == r.y + r.h {
        return Some(element.clone());
    }
    let croppable_uv = match &element.content {
        PlacedContent::Image { kind, .. } => match kind {
            ImageKind::Ui => Some((vec2(0.0, 0.0), vec2(1.0, 1.0))),
            ImageKind::Crop { u0, v0, u1, v1 } => Some((vec2(*u0, *v0), vec2(*u1, *v1))),
            _ => None,
        },
        _ => None,
    };
    let Some((uv0, uv1)) = croppable_uv else {
        return window.contains(r.center()).then(|| element.clone());
    };
    // Linear in both axes, so the clipped fraction of the rect is the clipped
    // fraction of its UV span.
    let lerp = |a: f32, b: f32, t: f32| a + (b - a) * t;
    let (tx0, tx1) = ((x0 - r.x) / r.w, (x1 - r.x) / r.w);
    let (ty0, ty1) = ((y0 - r.y) / r.h, (y1 - r.y) / r.h);
    let PlacedContent::Image { texture, .. } = &element.content else {
        unreachable!()
    };
    Some(PlacedElement {
        rect: Rect::new(x0, y0, x1 - x0, y1 - y0),
        alpha: element.alpha,
        content: PlacedContent::Image {
            texture: texture.clone(),
            kind: ImageKind::Crop {
                u0: lerp(uv0.x, uv1.x, tx0),
                v0: lerp(uv0.y, uv1.y, ty0),
                u1: lerp(uv0.x, uv1.x, tx1),
                v1: lerp(uv0.y, uv1.y, ty1),
            },
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image(rect: Rect) -> PlacedElement {
        PlacedElement {
            rect,
            alpha: 1.0,
            content: PlacedContent::Image {
                texture: "art.pcx".into(),
                kind: ImageKind::Ui,
            },
        }
    }

    fn text(rect: Rect) -> PlacedElement {
        PlacedElement {
            rect,
            alpha: 1.0,
            content: PlacedContent::Text {
                text: "hi".into(),
                font: "f".into(),
            },
        }
    }

    #[test]
    fn fit_letterboxes_and_maps_points_both_ways() {
        let v = CanvasViewport::fit(
            Rect::new(2.0, 124.0, 188.0, 296.0),
            Rect::new(0.0, 0.0, 200.0, 200.0),
        );
        // Height-limited: 296 -> 200, width 188 * 200/296 centered.
        assert!((v.dst.h - 200.0).abs() < 1e-4);
        assert!((v.dst.x - (200.0 - v.dst.w) / 2.0).abs() < 1e-4);
        let center = target_to_canvas(&[v], v.dst.center()).unwrap();
        assert!((center - v.src.center()).x.abs() < 1e-3);
        assert!((center - v.src.center()).y.abs() < 1e-3);
        // The letterbox margin maps to nothing.
        assert_eq!(target_to_canvas(&[v], vec2(1.0, 100.0)), None);
    }

    #[test]
    fn straddling_art_is_cropped_with_matching_uvs() {
        let v = CanvasViewport {
            src: Rect::new(100.0, 0.0, 100.0, 100.0),
            dst: Rect::new(0.0, 0.0, 50.0, 50.0),
        };
        let out = place_through(&[image(Rect::new(50.0, 0.0, 100.0, 100.0))], &[v]);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].rect, Rect::new(0.0, 0.0, 25.0, 50.0));
        match out[0].content {
            PlacedContent::Image {
                kind: ImageKind::Crop { u0, u1, v0, v1 },
                ..
            } => assert_eq!((u0, u1, v0, v1), (0.5, 1.0, 0.0, 1.0)),
            _ => panic!("expected crop"),
        }
    }

    #[test]
    fn text_is_kept_whole_by_its_center_or_dropped() {
        let v = CanvasViewport {
            src: Rect::new(0.0, 0.0, 100.0, 100.0),
            dst: Rect::new(0.0, 0.0, 100.0, 100.0),
        };
        let inside = text(Rect::new(80.0, 10.0, 30.0, 10.0));
        let outside = text(Rect::new(95.0, 10.0, 30.0, 10.0));
        let out = place_through(&[inside.clone(), outside], &[v]);
        assert_eq!(out, vec![inside]);
    }
}
