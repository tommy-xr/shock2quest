//! A solid wrist socket with native inventory status art in its recessed bed.
use crate::ui::{Rect, UiCanvas};
use cgmath::{InnerSpace, Vector2, Vector3, vec2, vec3};
use engine::scene::{SceneObject, VertexPositionTextureNormal, basic_material, mesh};
use shipyard::World;
use std::rc::Rc;

pub(crate) const INVENTORY_SIZE: Vector2<f32> = Vector2::new(636.0, 121.0);
const BED_HALF_WIDTH: f32 = 0.24;
const BED_HALF_HEIGHT: f32 = 0.46;
// Authored against the two prongs of the SOFTRED/BLUE/GREN/PURP implant meshes,
// in slot-width units after model_scale and the shared seating transform.
const PIN_X: [f32; 2] = [-0.032, 0.095];
const PIN_Z: f32 = 0.297;
// The implant body ends at +0.126: leave only a hairline seam at full insertion.
const CONNECTOR_FRONT: f32 = 0.128;
const CONNECTOR_BACK: f32 = 0.44;
const BORE_HALF_WIDTH: f32 = 0.034;
const BORE_HALF_HEIGHT: f32 = 0.030;

/// Keep the implant prominent while leaving clearance inside the slim rim.
pub(crate) fn model_scale(extent: Vector3<f32>) -> f32 {
    (2.0 * (BED_HALF_HEIGHT - 0.05) / extent.x.max(extent.y))
        .min(2.0 * (BED_HALF_WIDTH - 0.03) / extent.x)
}
pub(crate) fn inventory_well(slot: usize) -> Rect {
    Rect::new(563.0 + slot as f32 * 36.0, 84.0, 34.0, 34.0)
}

pub(crate) fn canvas(world: &World, slot: usize) -> UiCanvas {
    let mut canvas = UiCanvas::new(vec2(34.0, 34.0));
    // Keep the status symbol clear of the connector at the +Y end of the bed.
    let rect = Rect::new(10.0, 14.0, 14.0, 14.0);
    if let Some(entity) = crate::implants::equipped(world)[slot] {
        // The physical implant sits on the well. Keep its charge strip below
        // the mount so the protruding mesh need not shrink to fit a UI cell.
        canvas.fill(Rect::new(8.0, 41.0, 18.0, 4.0), [70, 80, 80]);
        canvas.fill(Rect::new(9.0, 42.0, 16.0, 2.0), [12, 18, 18]);
        let charge = (crate::implants::energy(world, entity)
            / crate::implants::recharge_capacity(world))
        .clamp(0.0, 1.0);
        if charge > 0.0 {
            canvas.fill(Rect::new(9.0, 42.0, 16.0 * charge, 2.0), [45, 215, 120]);
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

    let base = outline(0.32, 0.32, 0.54, -0.020);
    let shoulder = outline(0.315, 0.315, 0.535, 0.020);
    let rim = outline(0.30, 0.30, 0.52, 0.035);
    let mouth = outline(0.25, 0.25, 0.47, 0.035);
    let bed = outline(BED_HALF_WIDTH, BED_HALF_WIDTH, BED_HALF_HEIGHT, 0.0);

    // A small tapered block receives the prong tips at the +Y end of the bed.
    // Its front faces -Y; two actual blind bores continue into the solid block.
    let section = |low: f32, high: f32, y: f32| {
        let half_width = |z: f32| 0.17 - (z - 0.02) / 0.35 * 0.03;
        let center = (PIN_X[0] + PIN_X[1]) * 0.5;
        [
            vec3(center - half_width(low), y, low),
            vec3(center + half_width(low), y, low),
            vec3(center + half_width(high), y, high),
            vec3(center - half_width(high), y, high),
        ]
    };
    let aperture = |x: f32, y: f32, half_width: f32, half_height: f32| {
        [
            vec3(x - half_width, y, PIN_Z - half_height),
            vec3(x + half_width, y, PIN_Z - half_height),
            vec3(x + half_width, y, PIN_Z + half_height),
            vec3(x - half_width, y, PIN_Z + half_height),
        ]
    };
    let front = section(0.02, 0.37, CONNECTOR_FRONT);
    let back = section(0.02, 0.37, CONNECTOR_BACK);
    let holes = PIN_X.map(|x| aperture(x, CONNECTOR_FRONT, 0.043, 0.038));
    let middle = section(PIN_Z - 0.038, PIN_Z + 0.038, CONNECTOR_FRONT);
    let face = vec![
        section(0.02, PIN_Z - 0.038, CONNECTOR_FRONT),
        section(PIN_Z + 0.038, 0.37, CONNECTOR_FRONT),
        [middle[0], holes[0][0], holes[0][3], middle[3]],
        [holes[0][1], holes[1][0], holes[1][3], holes[0][2]],
        [holes[1][1], middle[1], middle[2], holes[1][2]],
    ];
    let mut sides = ring(back, front);
    sides.push([back[3], back[2], back[1], back[0]]);
    let mut bevels = Vec::new();
    let mut bores = Vec::new();
    let mut contacts = Vec::new();
    for (i, x) in PIN_X.into_iter().enumerate() {
        let inner = aperture(
            x,
            CONNECTOR_FRONT + 0.015,
            BORE_HALF_WIDTH,
            BORE_HALF_HEIGHT,
        );
        let end = aperture(x, CONNECTOR_BACK - 0.01, BORE_HALF_WIDTH, BORE_HALF_HEIGHT);
        bevels.extend(ring(holes[i], inner));
        bores.extend(ring(inner, end));
        contacts.push(end);
    }
    vec![
        surface(ring(base, shoulder), [50, 56, 61]),
        surface(ring(shoulder, rim), [115, 125, 130]),
        surface(ring(rim, mouth), [80, 88, 94]),
        surface(ring(mouth, bed), [28, 32, 35]),
        surface(
            vec![bed, [base[3], base[2], base[1], base[0]]],
            [10, 13, 14],
        ),
        surface(sides, [65, 72, 78]),
        surface(face, [95, 105, 112]),
        surface(bevels, [135, 145, 150]),
        surface(bores, [18, 20, 22]),
        surface(contacts, [120, 85, 35]),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn original_implant_prongs_seat_inside_the_two_bores() {
        let extent = vec3(0.372229, 0.809114, 0.338684);
        let scale = model_scale(extent);
        let lift = extent.z * scale * 0.5 + 0.0005 / crate::hud::virtual_arms::IMPLANT_SLOT_WIDTH;
        assert!((CONNECTOR_FRONT - 0.124333 * scale).abs() < 0.003);
        // The six tip vertices of SOFTRED.BIN, converted to engine coordinates.
        let tips = [
            vec3(-0.0238, 0.404557, 0.1000),
            vec3(-0.0238, 0.404557, 0.1225),
            vec3(-0.0433, 0.404557, 0.1113),
            vec3(0.1013, 0.404557, 0.1000),
            vec3(0.1013, 0.404557, 0.1225),
            vec3(0.0818, 0.404557, 0.1113),
        ];
        for (i, tip) in tips.into_iter().enumerate() {
            let p = tip * scale + vec3(0.0, 0.0, lift);
            assert!((p.x - PIN_X[i / 3]).abs() < BORE_HALF_WIDTH);
            assert!((p.z - PIN_Z).abs() < BORE_HALF_HEIGHT);
            assert!(p.y > CONNECTOR_FRONT + 0.015 && p.y < CONNECTOR_BACK - 0.01);
        }
    }

    #[test]
    fn wide_and_tall_implants_fit_inside_the_narrow_bed() {
        let bed = [
            vec2(-BED_HALF_WIDTH, -BED_HALF_HEIGHT),
            vec2(BED_HALF_WIDTH, -BED_HALF_HEIGHT),
            vec2(BED_HALF_WIDTH, BED_HALF_HEIGHT),
            vec2(-BED_HALF_WIDTH, BED_HALF_HEIGHT),
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
