//! A slowed shot's wake as a rippling sleeve of displaced air: rings laid
//! along its path, their radius waving along the length and drifting with
//! world time, swelling and fading with age. Pure geometry; the trail
//! renders it.
use std::collections::VecDeque;

use cgmath::{InnerSpace, Vector3, vec2, vec3};
use engine::scene::VertexPositionTextureNormal;

/// World units between rings along the path.
const RING_SPACING: f32 = 0.03;
/// Rings kept per tube, oldest dropped first: at most this many times
/// `RING_SPACING` of path (12 world units, a long room).
pub(crate) const MAX_RINGS: usize = 400;
/// Vertices around each ring.
pub(crate) const SIDES: usize = 12;
/// Ring radius before ripple and swelling.
const BASE_RADIUS: f32 = 0.06;
/// Ripple depth as a share of the radius.
pub(crate) const RIPPLE_AMPLITUDE: f32 = 0.3;
/// World units from one ripple to the next (about 5.6 per unit, six rings
/// each, so a ripple reads as a curve rather than a zigzag).
pub(crate) const RIPPLE_WAVELENGTH: f32 = 0.18;
/// Ripples drift along the tube at this many wavelengths per world second.
const RIPPLE_DRIFT: f32 = 0.5;
/// Texels across the fade texture's age axis: `u` lands on texel centres
/// so linear filtering never wraps the gone end into the new one.
pub(crate) const AGE_TEXELS: usize = 32;
/// Radius gained per world second of age, as a share of the base.
pub(crate) const GROWTH: f32 = 0.6;

struct Ring {
    center: Vector3<f32>,
    /// Arc length from the tube's start.
    distance: f32,
    age: f32,
}

pub(crate) struct RippleTube {
    rings: VecDeque<Ring>,
    /// World seconds since the tube began, driving the ripple drift.
    clock: f32,
    /// World seconds a ring lasts.
    lifetime: f32,
    /// The path's end so far, which may sit between rings.
    head: Vector3<f32>,
    head_distance: f32,
    /// Path length past the newest ring, not yet a ring.
    pending: f32,
}

impl RippleTube {
    pub fn new(start: Vector3<f32>, lifetime: f32) -> Self {
        let mut rings = VecDeque::with_capacity(MAX_RINGS);
        rings.push_back(Ring {
            center: start,
            distance: 0.0,
            age: 0.0,
        });
        Self {
            rings,
            clock: 0.0,
            lifetime: lifetime.max(1.0e-3),
            head: start,
            head_distance: 0.0,
            pending: 0.0,
        }
    }

    /// Extends the path to `point`, laying a ring every `RING_SPACING`.
    pub fn extend_to(&mut self, point: Vector3<f32>) {
        let travel = point - self.head;
        let length = travel.magnitude();
        if length < 1.0e-6 {
            return;
        }
        let direction = travel / length;
        let mut along = RING_SPACING - self.pending;
        while along <= length {
            if self.rings.len() == MAX_RINGS {
                self.rings.pop_front();
            }
            self.rings.push_back(Ring {
                center: self.head + direction * along,
                distance: self.head_distance + along,
                age: 0.0,
            });
            along += RING_SPACING;
        }
        self.pending = length - (along - RING_SPACING);
        self.head = point;
        self.head_distance += length;
    }

    /// Ages every ring by world `dt`, dropping those past their lifetime.
    pub fn advance(&mut self, dt: f32) {
        self.clock += dt;
        for ring in &mut self.rings {
            ring.age += dt;
        }
        while self
            .rings
            .front()
            .is_some_and(|ring| ring.age >= self.lifetime)
        {
            self.rings.pop_front();
        }
    }

    pub fn is_empty(&self) -> bool {
        self.rings.is_empty()
    }

    /// Ring radius at `distance` along the path and `age`.
    fn radius(&self, distance: f32, age: f32) -> f32 {
        let phase =
            (distance / RIPPLE_WAVELENGTH - self.clock * RIPPLE_DRIFT) * std::f32::consts::TAU;
        BASE_RADIUS * (1.0 + RIPPLE_AMPLITUDE * phase.sin()) * (1.0 + GROWTH * age)
    }

    /// The tube as an indexed triangle list. Texture `u` is the ring's age
    /// over its lifetime (first texel new, last gone) and `v` its ripple phase in
    /// wavelengths, so a texture can fade the wake and band its rings.
    /// Empty with fewer than two rings.
    pub fn mesh(&self) -> (Vec<VertexPositionTextureNormal>, Vec<u32>) {
        let count = self.rings.len();
        if count < 2 {
            return (Vec::new(), Vec::new());
        }
        let mut vertices = Vec::with_capacity(count * SIDES);
        // A path that doubles back can put two neighbours on one point: keep
        // the last good heading there.
        let mut tangent = vec3(0.0, 0.0, 1.0);
        for (index, ring) in self.rings.iter().enumerate() {
            // Local heading from the neighbouring rings.
            let before = &self.rings[index.saturating_sub(1)];
            let after = &self.rings[(index + 1).min(count - 1)];
            let span = after.center - before.center;
            if span.magnitude2() > 1.0e-12 {
                tangent = span.normalize();
            }
            let reference = if tangent.y.abs() < 0.9 {
                vec3(0.0, 1.0, 0.0)
            } else {
                vec3(1.0, 0.0, 0.0)
            };
            let right = reference.cross(tangent).normalize();
            let up = tangent.cross(right);
            let radius = self.radius(ring.distance, ring.age);
            let fraction = (ring.age / self.lifetime).clamp(0.0, 1.0);
            let u = (0.5 + fraction * (AGE_TEXELS - 1) as f32) / AGE_TEXELS as f32;
            let v = ring.distance / RIPPLE_WAVELENGTH - self.clock * RIPPLE_DRIFT;
            for side in 0..SIDES {
                let angle = side as f32 / SIDES as f32 * std::f32::consts::TAU;
                let normal = right * angle.cos() + up * angle.sin();
                vertices.push(VertexPositionTextureNormal {
                    position: ring.center + normal * radius,
                    uv: vec2(u, v),
                    normal,
                });
            }
        }
        let mut indices = Vec::with_capacity((count - 1) * SIDES * 6);
        for ring in 0..count as u32 - 1 {
            for side in 0..SIDES as u32 {
                let next = (side + 1) % SIDES as u32;
                let [a, b] = [ring * SIDES as u32 + side, ring * SIDES as u32 + next];
                let [c, d] = [a + SIDES as u32, b + SIDES as u32];
                indices.extend([a, b, d, a, d, c]);
            }
        }
        (vertices, indices)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn straight(length: f32) -> RippleTube {
        let mut tube = RippleTube::new(vec3(0.0, 0.0, 0.0), 1.0);
        tube.extend_to(vec3(length, 0.0, 0.0));
        tube
    }

    #[test]
    fn rings_are_laid_by_spacing_and_capped() {
        let per_unit = (1.0 / RING_SPACING).round() as usize;
        assert!((per_unit..=per_unit + 1).contains(&straight(1.0).rings.len()));
        // Spacing holds across short steps too.
        let mut tube = RippleTube::new(vec3(0.0, 0.0, 0.0), 1.0);
        for i in 1..=100 {
            tube.extend_to(vec3(i as f32 * 0.01, 0.0, 0.0));
        }
        assert!((per_unit..=per_unit + 1).contains(&tube.rings.len()));
        assert_eq!(straight(100.0).rings.len(), MAX_RINGS);
    }

    #[test]
    fn the_radius_ripples_within_its_amplitude_and_swells_with_age() {
        let tube = straight(2.0);
        let (vertices, indices) = tube.mesh();
        assert_eq!(vertices.len(), tube.rings.len() * SIDES);
        assert_eq!(indices.len(), (tube.rings.len() - 1) * SIDES * 6);
        let radii: Vec<f32> = tube
            .rings
            .iter()
            .map(|ring| tube.radius(ring.distance, 0.0))
            .collect();
        let (lo, hi) = radii
            .iter()
            .fold((f32::MAX, f32::MIN), |(lo, hi), r| (lo.min(*r), hi.max(*r)));
        assert!(lo >= BASE_RADIUS * (1.0 - RIPPLE_AMPLITUDE) - 1e-6);
        assert!(hi <= BASE_RADIUS * (1.0 + RIPPLE_AMPLITUDE) + 1e-6);
        assert!(
            hi - lo > BASE_RADIUS * RIPPLE_AMPLITUDE,
            "it actually ripples"
        );
        let swollen = tube.radius(0.3, 1.0) / tube.radius(0.3, 0.0);
        assert!((swollen - (1.0 + GROWTH)).abs() < 1e-5);
    }

    #[test]
    fn rings_fade_to_gone_at_their_lifetime() {
        let mut tube = straight(1.0);
        tube.advance(0.5);
        let (vertices, _) = tube.mesh();
        let half_way = (0.5 + 0.5 * (AGE_TEXELS - 1) as f32) / AGE_TEXELS as f32;
        assert!(vertices.iter().all(|v| (v.uv.x - half_way).abs() < 1e-5));
        tube.advance(0.5);
        assert!(tube.is_empty());
        assert!(tube.mesh().0.is_empty());
    }

    #[test]
    fn a_path_that_doubles_back_stays_finite() {
        let mut tube = RippleTube::new(vec3(0.0, 0.0, 0.0), 1.0);
        tube.extend_to(vec3(0.06, 0.0, 0.0));
        tube.extend_to(vec3(0.0, 0.0, 0.0));
        let (vertices, _) = tube.mesh();
        assert!(!vertices.is_empty());
        assert!(
            vertices
                .iter()
                .all(|v| v.position.x.is_finite() && v.normal.x.is_finite())
        );
    }

    #[test]
    fn rings_lie_across_the_heading() {
        let mut tube = RippleTube::new(vec3(1.0, 2.0, 3.0), 1.0);
        let heading = vec3(0.0, 0.6, 0.8);
        tube.extend_to(vec3(1.0, 2.0, 3.0) + heading);
        let (vertices, _) = tube.mesh();
        for (ring, chunk) in tube.rings.iter().zip(vertices.chunks_exact(SIDES)) {
            for vertex in chunk {
                let offset = vertex.position - ring.center;
                assert!(offset.dot(heading).abs() < 1e-4);
                assert!((offset.normalize() - vertex.normal).magnitude() < 1e-4);
            }
        }
    }
}
