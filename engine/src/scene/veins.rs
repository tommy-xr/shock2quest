//! A procedural, tileable vein mask for wet organic surfaces that ship no
//! authored specular mask: thin, wandering bright ridges on a dark field, so
//! a highlight gathers on "veins" instead of spreading evenly.
use crate::texture::{self, Texture, TextureFilter, TextureOptions};
use crate::texture_format::{PixelFormat, RawTextureData};
use std::rc::Rc;

/// Texels per side of the shared vein texture.
pub const VEIN_TEXTURE_SIZE: usize = 256;
const SEED: u32 = 0x5EED_0B1E;

/// Single-channel vein intensity, row-major, `size * size` texels. Pure and
/// deterministic; wraps seamlessly on both axes (every feature is periodic in
/// the unit square).
pub fn generate_veins(size: usize, seed: u32) -> Vec<u8> {
    let mut texels = Vec::with_capacity(size * size);
    for y in 0..size {
        for x in 0..size {
            let p = [x as f32 / size as f32, y as f32 / size as f32];
            texels.push((vein_intensity(p, seed) * 255.0).round() as u8);
        }
    }
    texels
}

/// Two octaves of warped cell borders: a sparse network of main veins and,
/// in patches, finer capillaries branching off them.
fn vein_intensity(p: [f32; 2], seed: u32) -> f32 {
    // Periodic domain warp bends straight cell borders into organic curves.
    let warp = |cells, amount, salt| {
        [
            amount * (value_noise(p, cells, seed ^ salt) - 0.5),
            amount * (value_noise(p, cells, seed ^ salt ^ 0x77) - 0.5),
        ]
    };
    let (coarse, fine) = (warp(3, 0.16, 0x11), warp(9, 0.04, 0x22));
    let q = [p[0] + coarse[0] + fine[0], p[1] + coarse[1] + fine[1]];
    // Fading whole stretches of border breaks closed cells into open,
    // branching runs; varying width makes a vein taper.
    let presence = smoothstep(0.25, 0.55, value_noise(p, 6, seed ^ 0x33));
    let taper = value_noise(p, 6, seed ^ 0x44);
    let main = ridge(cell_border(q, 6, seed ^ 0x55), 0.006 + 0.014 * taper) * presence;
    let patch = smoothstep(0.5, 0.8, value_noise(p, 4, seed ^ 0x66));
    let capillaries = ridge(cell_border(q, 12, seed ^ 0x88), 0.005) * 0.6 * patch;
    main.max(capillaries)
}

/// 1 on a border, falling smoothly to 0 at `width` away from it.
fn ridge(distance: f32, width: f32) -> f32 {
    1.0 - smoothstep(0.0, width, distance)
}

fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Distance (in unit-square coordinates) from `p` to the nearest border of a
/// periodic `cells`x`cells` jittered Voronoi diagram.
fn cell_border(p: [f32; 2], cells: i32, seed: u32) -> f32 {
    let scaled = [p[0] * cells as f32, p[1] * cells as f32];
    let cell = [scaled[0].floor() as i32, scaled[1].floor() as i32];
    let local = [scaled[0] - cell[0] as f32, scaled[1] - cell[1] as f32];
    let site = |dx: i32, dy: i32| {
        let (cx, cy) = (cell[0] + dx, cell[1] + dy);
        let (wx, wy) = (cx.rem_euclid(cells), cy.rem_euclid(cells));
        [
            dx as f32 + 0.1 + 0.8 * hash(wx, wy, seed) - local[0],
            dy as f32 + 0.1 + 0.8 * hash(wx, wy, seed ^ 0x9E37) - local[1],
        ]
    };
    // Nearest site, then the distance to its bisector with every neighbour.
    let mut nearest = [0.0, 0.0];
    let mut nearest_distance = f32::MAX;
    for dy in -1..=1 {
        for dx in -1..=1 {
            let r = site(dx, dy);
            let d = r[0] * r[0] + r[1] * r[1];
            if d < nearest_distance {
                nearest_distance = d;
                nearest = r;
            }
        }
    }
    let mut border = f32::MAX;
    for dy in -2..=2 {
        for dx in -2..=2 {
            let r = site(dx, dy);
            let delta = [r[0] - nearest[0], r[1] - nearest[1]];
            let length_sq = delta[0] * delta[0] + delta[1] * delta[1];
            if length_sq < 1e-8 {
                continue;
            }
            let mid = [(r[0] + nearest[0]) * 0.5, (r[1] + nearest[1]) * 0.5];
            let d = (mid[0] * delta[0] + mid[1] * delta[1]) / length_sq.sqrt();
            border = border.min(d);
        }
    }
    border / cells as f32
}

/// Smooth value noise in [0, 1], periodic over the unit square.
fn value_noise(p: [f32; 2], cells: i32, seed: u32) -> f32 {
    let scaled = [p[0] * cells as f32, p[1] * cells as f32];
    let (x0, y0) = (scaled[0].floor() as i32, scaled[1].floor() as i32);
    let (fx, fy) = (scaled[0] - x0 as f32, scaled[1] - y0 as f32);
    let at = |x: i32, y: i32| hash(x.rem_euclid(cells), y.rem_euclid(cells), seed);
    let (sx, sy) = (fx * fx * (3.0 - 2.0 * fx), fy * fy * (3.0 - 2.0 * fy));
    let top = at(x0, y0) + (at(x0 + 1, y0) - at(x0, y0)) * sx;
    let bottom = at(x0, y0 + 1) + (at(x0 + 1, y0 + 1) - at(x0, y0 + 1)) * sx;
    top + (bottom - top) * sy
}

/// Integer hash to [0, 1).
fn hash(x: i32, y: i32, seed: u32) -> f32 {
    let mut h = (x as u32).wrapping_mul(0x8DA6_B343)
        ^ (y as u32).wrapping_mul(0xD816_3841)
        ^ seed.wrapping_mul(0xCB1A_B31F);
    h ^= h >> 13;
    h = h.wrapping_mul(0x5BD1_E995);
    h ^= h >> 15;
    (h >> 8) as f32 / (1u32 << 24) as f32
}

thread_local! {
    /// Generated and uploaded once per thread on first use; needs a live GL
    /// context (the `shared_white_pixel` pattern).
    static VEINS: std::cell::RefCell<Option<Rc<Texture>>> =
        const { std::cell::RefCell::new(None) };
}

/// The shared vein texture: repeating, mipmapped so fine ridges do not
/// sparkle when minified. Requires a current GL context.
pub(crate) fn shared_vein_texture() -> Rc<Texture> {
    VEINS.with(|cell| {
        cell.borrow_mut()
            .get_or_insert_with(|| {
                let bytes = generate_veins(VEIN_TEXTURE_SIZE, SEED)
                    .into_iter()
                    .flat_map(|v| [v, v, v])
                    .collect();
                Rc::new(texture::init_from_memory2(
                    RawTextureData {
                        bytes,
                        width: VEIN_TEXTURE_SIZE as u32,
                        height: VEIN_TEXTURE_SIZE as u32,
                        format: PixelFormat::RGB,
                    },
                    &TextureOptions {
                        wrap: true,
                        filter: TextureFilter::LinearMipmap,
                        ..Default::default()
                    },
                ))
            })
            .clone()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SIZE: usize = 128;

    #[test]
    fn deterministic() {
        assert_eq!(generate_veins(SIZE, 7), generate_veins(SIZE, 7));
        assert_ne!(generate_veins(SIZE, 7), generate_veins(SIZE, 8));
    }

    /// Sampling just past the right/bottom edge equals the left/top edge, so
    /// the repeating texture has no seam.
    #[test]
    fn tiles_seamlessly() {
        for i in 0..64 {
            let t = i as f32 / 64.0;
            for (a, b) in [([0.0, t], [1.0, t]), ([t, 0.0], [t, 1.0])] {
                let (va, vb) = (vein_intensity(a, SEED), vein_intensity(b, SEED));
                assert!((va - vb).abs() < 1e-3, "{a:?}={va} vs {b:?}={vb}");
            }
        }
        // And adjacent texels across the wrap differ no more than inside.
        let texels = generate_veins(SIZE, SEED);
        let at = |x: usize, y: usize| texels[y * SIZE + x] as i32;
        let max_step = |pairs: &mut dyn Iterator<Item = (i32, i32)>| {
            pairs.map(|(a, b)| (a - b).abs()).max().unwrap()
        };
        let across = max_step(&mut (0..SIZE).map(|y| (at(SIZE - 1, y), at(0, y))));
        let inside = max_step(
            &mut (0..SIZE)
                .flat_map(|y| (1..SIZE).map(move |x| (y, x)))
                .map(|(y, x)| (at(x - 1, y), at(x, y))),
        );
        assert!(across <= inside, "wrap step {across} > interior {inside}");
    }

    /// Ridges are a minority of texels, but present.
    #[test]
    fn veins_are_sparse() {
        let texels = generate_veins(VEIN_TEXTURE_SIZE, SEED);
        let bright = texels.iter().filter(|&&v| v > 128).count() as f32 / texels.len() as f32;
        let dark = texels.iter().filter(|&&v| v < 16).count() as f32 / texels.len() as f32;
        assert!((0.03..0.25).contains(&bright), "bright coverage {bright}");
        assert!(dark > 0.5, "dark field {dark}");
    }
}
