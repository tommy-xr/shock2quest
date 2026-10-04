//! Nine-patch geometry is resolved once in canvas pixels. Presenters receive
//! ordinary cropped images, so flat, world and curved canvases share the cuts.
use super::{ImageKind, PlacedContent, PlacedElement, Rect};

fn cuts(length: f32, first: f32, last: f32) -> [f32; 4] {
    let first = first.max(0.0);
    let last = last.max(0.0);
    let scale = if first + last > length {
        length / (first + last)
    } else {
        1.0
    };
    [0.0, first * scale, length - last * scale, length]
}

pub(super) fn append(element: PlacedElement, out: &mut Vec<PlacedElement>) {
    let PlacedContent::Image {
        texture,
        kind:
            ImageKind::NineSlice {
                source_size: [w, h],
                borders: [left, top, right, bottom],
            },
    } = &element.content
    else {
        out.push(element);
        return;
    };
    let rect = element.rect;
    if *w <= 0.0 || *h <= 0.0 || rect.w <= 0.0 || rect.h <= 0.0 {
        return;
    }
    let sx = cuts(*w, *left, *right);
    let sy = cuts(*h, *top, *bottom);
    let dx = cuts(rect.w, sx[1], w - sx[2]);
    let dy = cuts(rect.h, sy[1], h - sy[2]);
    for y in 0..3 {
        for x in 0..3 {
            if dx[x + 1] <= dx[x] || dy[y + 1] <= dy[y] || sx[x + 1] <= sx[x] || sy[y + 1] <= sy[y]
            {
                continue;
            }
            out.push(PlacedElement {
                rect: Rect::new(
                    rect.x + dx[x],
                    rect.y + dy[y],
                    dx[x + 1] - dx[x],
                    dy[y + 1] - dy[y],
                ),
                alpha: element.alpha,
                turns: element.turns,
                content: PlacedContent::Image {
                    texture: texture.clone(),
                    kind: ImageKind::Crop {
                        u0: sx[x] / w,
                        v0: sy[y] / h,
                        u1: sx[x + 1] / w,
                        v1: sy[y + 1] / h,
                    },
                },
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn patches(width: f32, height: f32) -> Vec<PlacedElement> {
        let mut out = Vec::new();
        append(
            PlacedElement {
                rect: Rect::new(10.0, 20.0, width, height),
                alpha: 0.6,
                turns: 0,
                content: PlacedContent::Image {
                    texture: "button.pcx".into(),
                    kind: ImageKind::NineSlice {
                        source_size: [90.0, 32.0],
                        borders: [3.0; 4],
                    },
                },
            },
            &mut out,
        );
        out
    }

    #[test]
    fn wide_and_narrow_buttons_keep_the_same_corners_and_fill_the_rect() {
        for (w, h) in [(54.0, 21.0), (140.0, 18.0), (158.0, 21.0)] {
            let out = patches(w, h);
            assert_eq!(out.len(), 9);
            assert_eq!(out[0].rect, Rect::new(10.0, 20.0, 3.0, 3.0));
            assert_eq!(
                out[8].rect,
                Rect::new(10.0 + w - 3.0, 20.0 + h - 3.0, 3.0, 3.0)
            );
            assert_eq!(out.iter().map(|p| p.rect.w * p.rect.h).sum::<f32>(), w * h);
            for p in &out {
                assert_eq!(p.alpha, 0.6);
                let PlacedContent::Image { kind, .. } = p.content else {
                    panic!()
                };
                assert!(matches!(kind, ImageKind::Crop { .. }));
            }
            // Every horizontal/vertical seam meets, without overdraw or gaps.
            for row in out.chunks(3) {
                assert_eq!(row[0].rect.x + row[0].rect.w, row[1].rect.x);
                assert_eq!(row[1].rect.x + row[1].rect.w, row[2].rect.x);
            }
            for x in 0..3 {
                assert_eq!(out[x].rect.y + out[x].rect.h, out[x + 3].rect.y);
                assert_eq!(out[x + 3].rect.y + out[x + 3].rect.h, out[x + 6].rect.y);
            }
        }
    }

    #[test]
    fn tiny_destinations_shrink_borders_without_inverting_or_overlapping() {
        let out = patches(4.0, 2.0);
        assert_eq!(out.len(), 4);
        assert!(out.iter().all(|p| p.rect.w == 2.0 && p.rect.h == 1.0));
        assert_eq!(cuts(8.0, 3.0, 1.0), [0.0, 3.0, 7.0, 8.0]);
        assert!(patches(0.0, 20.0).is_empty());
    }
}
