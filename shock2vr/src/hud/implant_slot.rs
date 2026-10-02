//! A solid wrist socket with native inventory status art in its recessed bed.
use crate::ui::{Rect, UiCanvas};
use cgmath::{InnerSpace, Vector2, Vector3, vec2, vec3};
use engine::scene::{SceneObject, VertexPositionTextureNormal, basic_material, mesh};
use shipyard::World;
use std::rc::Rc;

pub(crate) const INVENTORY_SIZE: Vector2<f32> = Vector2::new(636.0, 121.0);
const BED_TOP_HALF_WIDTH: f32 = 0.29;
const BED_BOTTOM_HALF_WIDTH: f32 = 0.41;
const BED_HALF_HEIGHT: f32 = 0.45;

/// Fit all four footprint corners into the tapered bed, leaving room for the rim.
/// Narrow implants retain their size; broad ones cannot intersect the side walls.
pub(crate) fn model_scale(extent: Vector3<f32>) -> f32 {
    let taper = (BED_BOTTOM_HALF_WIDTH - BED_TOP_HALF_WIDTH) / (2.0 * BED_HALF_HEIGHT);
    let width = BED_TOP_HALF_WIDTH + BED_BOTTOM_HALF_WIDTH - 0.06;
    let height = 2.0 * (BED_HALF_HEIGHT - 0.04);
    (height / extent.x.max(extent.y)).min(width / (extent.x + taper * extent.y))
}
pub(crate) fn inventory_well(slot: usize) -> Rect {
    Rect::new(563.0 + slot as f32 * 36.0, 84.0, 34.0, 34.0)
}

pub(crate) fn canvas(world: &World, slot: usize) -> UiCanvas {
    let mut canvas = UiCanvas::new(vec2(34.0, 34.0));
    let rect = Rect::new(7.0, 7.0, 20.0, 20.0);
    if let Some(entity) = crate::implants::equipped(world)[slot] {
        // The physical implant sits on the well. Keep its charge strip below
        // the mount so the protruding mesh need not shrink to fit a UI cell.
        canvas.fill(Rect::new(3.0, 41.0, 28.0, 4.0), [70, 80, 80]);
        canvas.fill(Rect::new(4.0, 42.0, 26.0, 2.0), [12, 18, 18]);
        let charge = (crate::implants::energy(world, entity)
            / crate::implants::recharge_capacity(world))
        .clamp(0.0, 1.0);
        if charge > 0.0 {
            canvas.fill(Rect::new(4.0, 42.0, 26.0 * charge, 2.0), [45, 215, 120]);
        }
    } else if crate::implants::socket_locked(world, slot) {
        canvas.image(rect, "iface/block.pcx");
    } else {
        // Keep the native open symbol, but replace its square border with the mesh rim.
        let well = inventory_well(slot);
        canvas.cropped_image(
            rect,
            "invback.pcx",
            Rect::new(well.x + 2.0, well.y + 2.0, 30.0, 30.0),
            INVENTORY_SIZE,
        );
    }
    canvas
}

/// Cached GPU geometry in slot-width units, +Z outward. The bed is z=0,
/// matching the implant's existing seating plane; the lip rises around its base.
pub(crate) fn housing() -> Vec<SceneObject> {
    fn outline(top: f32, bottom: f32, half_height: f32, z: f32) -> [Vector3<f32>; 4] {
        [
            vec3(-bottom, -half_height, z),
            vec3(bottom, -half_height, z),
            vec3(top, half_height, z),
            vec3(-top, half_height, z),
        ]
    }
    fn ring(a: [Vector3<f32>; 4], b: [Vector3<f32>; 4]) -> Vec<[Vector3<f32>; 4]> {
        (0..4)
            .map(|i| [a[i], a[(i + 1) % 4], b[(i + 1) % 4], b[i]])
            .collect()
    }
    fn surface(quads: Vec<[Vector3<f32>; 4]>, rgb: [u8; 3]) -> SceneObject {
        let vertices = quads
            .into_iter()
            .flat_map(|q| {
                let normal = (q[1] - q[0]).cross(q[2] - q[0]).normalize();
                [0, 1, 2, 0, 2, 3].map(|i| VertexPositionTextureNormal {
                    position: q[i],
                    normal,
                    uv: vec2(0.5, 0.5),
                })
            })
            .collect();
        let texture: Rc<dyn engine::texture::TextureTrait> = Rc::new(
            engine::texture::init_from_memory(engine::texture_format::RawTextureData {
                bytes: vec![rgb[0], rgb[1], rgb[2], 255],
                width: 1,
                height: 1,
                format: engine::texture_format::PixelFormat::RGBA,
            }),
        );
        SceneObject::new(
            basic_material::create(texture, 0.0, 0.0),
            Box::new(mesh::create(vertices)),
        )
    }

    let base = outline(0.45, 0.59, 0.60, -0.025);
    let shoulder = outline(0.43, 0.57, 0.58, 0.12);
    let rim = outline(0.40, 0.54, 0.55, 0.16);
    let mouth = outline(0.30, 0.42, 0.46, 0.16);
    let bed = outline(
        BED_TOP_HALF_WIDTH,
        BED_BOTTOM_HALF_WIDTH,
        BED_HALF_HEIGHT,
        0.0,
    );
    let mut contacts = Vec::new();
    for x in [-0.12, 0.0, 0.12] {
        contacts.push([
            vec3(x - 0.025, 0.30, 0.006),
            vec3(x + 0.025, 0.30, 0.006),
            vec3(x + 0.025, 0.40, 0.006),
            vec3(x - 0.025, 0.40, 0.006),
        ]);
    }
    vec![
        surface(ring(base, shoulder), [65, 72, 78]),
        surface(ring(shoulder, rim), [175, 185, 190]),
        surface(ring(rim, mouth), [115, 125, 132]),
        surface(ring(mouth, bed), [35, 40, 43]),
        surface(
            vec![bed, [base[3], base[2], base[1], base[0]]],
            [10, 13, 14],
        ),
        surface(contacts, [145, 103, 43]),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wide_and_tall_implants_fit_inside_the_tapered_bed() {
        let bed = [
            vec2(-0.41, -0.45),
            vec2(0.41, -0.45),
            vec2(0.29, 0.45),
            vec2(-0.29, 0.45),
        ];
        for extent in [
            vec3(2.0, 1.0, 0.3),
            vec3(1.0, 2.0, 0.3),
            vec3(1.0, 1.0, 0.3),
            vec3(0.372, 0.809, 0.338),
        ] {
            let half = extent * model_scale(extent) * 0.5;
            for x in [-half.x, half.x] {
                for y in [-half.y, half.y] {
                    for i in 0..4 {
                        let edge = bed[(i + 1) % 4] - bed[i];
                        let corner = vec2(x, y) - bed[i];
                        assert!(
                            edge.x * corner.y - edge.y * corner.x > 0.0,
                            "implant corner lies outside the cavity: {extent:?}"
                        );
                    }
                }
            }
        }
    }
}
