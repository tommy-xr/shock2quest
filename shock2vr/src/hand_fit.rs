//! Curls the glove's fingers until they touch whatever the hand is holding.
//!
//! Pickup physics says nothing about shape - every pickup template is
//! `PhysType SPHERE` - so the fit runs against the item's **render mesh**
//! ([`dark::importers::VrContactMesh`]), transformed into the hand's own
//! space. For each finger the solver sweeps the open->fist blend the glove
//! already poses with, brackets the first curl at which a phalanx capsule
//! reaches the surface, and bisects for the contact point. The result is a
//! [`FingerAmounts`] the renderer feeds straight to
//! [`crate::hand_pose::Pose::blend_per_finger`].
//!
//! Everything here is pure: the caller supplies a posed-hand [`HandRig`] and a
//! [`ContactMesh`], so the solver is exercised in tests against synthetic
//! meshes with no assets and no GPU.

use cgmath::{EuclideanSpace, InnerSpace, Point3, Vector3};

use crate::hand_pose::FingerAmounts;

/// One finger of the hand.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Finger {
    Thumb,
    Index,
    Middle,
    Ring,
    Pinky,
}

impl Finger {
    pub const ALL: [Finger; 5] = [
        Finger::Thumb,
        Finger::Index,
        Finger::Middle,
        Finger::Ring,
        Finger::Pinky,
    ];

    /// Where this finger's curl lands in a [`FingerAmounts`].
    fn set(self, amounts: &mut FingerAmounts, curl: f32) {
        match self {
            Finger::Thumb => amounts.thumb = curl,
            Finger::Index => amounts.index = curl,
            Finger::Middle => amounts.middle = curl,
            Finger::Ring => amounts.ring = curl,
            Finger::Pinky => amounts.pinky = curl,
        }
    }
}

/// A capsule along one phalanx, in hand space (world units).
#[derive(Debug, Clone, Copy)]
pub struct Capsule {
    pub a: Point3<f32>,
    pub b: Point3<f32>,
    pub radius: f32,
}

/// A hand the fit can pose: the phalanx capsules of one finger at one curl,
/// in hand space.
///
/// Whole phalanges rather than just the fingertip: a tip-only test lets a
/// knuckle close straight through the item while the tip is still clear.
pub trait HandRig {
    fn phalanges(&mut self, finger: Finger, curl: f32) -> Vec<Capsule>;
}

/// The shape family of a grip: which curl envelope each finger searches in.
///
/// One blend space throughout - every amount is "how far from `open` toward
/// `fist`" - so a family is a per-finger floor and cap on that blend rather
/// than a second pose vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GripFamily {
    /// Wrap a handle: every finger closes as far as the surface allows.
    Cylindrical,
    /// Thumb and index only; the rest hold a relaxed rest curl out of the way.
    Pinch,
    /// Something bigger than the palm: fingers splay on the surface rather
    /// than wrapping, so the curl is capped shallow.
    Broad,
    /// A gun: the index rests on the trigger (and the player's own pull adds
    /// the rest on top), the others wrap the grip.
    Trigger,
}

/// How far the index may curl on a trigger before the player's own pull takes
/// over - the resting finger, not a squeezed one.
const TRIGGER_INDEX_CAP: f32 = 0.45;

/// The relaxed curl a pinch leaves its idle fingers at: out of the way of the
/// pinched item without reading as a fist.
const PINCH_REST: f32 = 0.55;

/// How far a hand closes on something too big to wrap.
const BROAD_CAP: f32 = 0.4;

/// The curl range one finger searches: `cap == floor` means the finger takes
/// no part in this grip and simply holds that pose.
#[derive(Debug, Clone, Copy)]
struct Envelope {
    floor: f32,
    cap: f32,
}

impl GripFamily {
    fn envelope(self, finger: Finger) -> Envelope {
        let full = Envelope {
            floor: 0.0,
            cap: 1.0,
        };
        match (self, finger) {
            (GripFamily::Cylindrical, _) => full,
            (GripFamily::Broad, _) => Envelope {
                floor: 0.0,
                cap: BROAD_CAP,
            },
            (GripFamily::Pinch, Finger::Thumb | Finger::Index) => full,
            (GripFamily::Pinch, _) => Envelope {
                floor: PINCH_REST,
                cap: PINCH_REST,
            },
            (GripFamily::Trigger, Finger::Index) => Envelope {
                floor: 0.0,
                cap: TRIGGER_INDEX_CAP,
            },
            (GripFamily::Trigger, _) => full,
        }
    }

    /// Readable name, for the hand readout and the grip override table.
    pub fn as_str(self) -> &'static str {
        match self {
            GripFamily::Cylindrical => "cylindrical",
            GripFamily::Pinch => "pinch",
            GripFamily::Broad => "broad",
            GripFamily::Trigger => "trigger",
        }
    }
}

/// Metres expressed in the world units the hand frame is measured in.
const fn meters(m: f32) -> f32 {
    m / crate::METERS_PER_WORLD_UNIT
}

/// Thinner than this in its smallest dimension and an item is pinched rather
/// than gripped - a magazine, a keycard.
const PINCH_THICKNESS: f32 = meters(0.018);

/// Wider than this across its two smaller dimensions and the hand cannot close
/// round it at all - a basketball, a helmet.
const BROAD_GIRTH: f32 = meters(0.10);

/// How far a phalanx may sink into the surface before it counts as contact.
/// Skin against a hard edge deforms a little; zero here would leave a visible
/// air gap at every grip.
const CONTACT_TOLERANCE: f32 = meters(0.0015);

/// Curl samples the coarse sweep takes before bisecting. Fine enough that a
/// fingertip cannot step clean through a wall of the mesh between samples.
const COARSE_STEPS: usize = 16;

/// Bisection rounds after the sweep brackets first contact; 6 rounds resolve
/// the curl to under 1% of the envelope.
const BISECT_ROUNDS: usize = 6;

/// The triangles a hand can close against, in hand space.
pub struct ContactMesh {
    triangles: Vec<[Point3<f32>; 3]>,
}

impl ContactMesh {
    pub fn new(triangles: Vec<[Point3<f32>; 3]>) -> Self {
        Self { triangles }
    }

    pub fn is_empty(&self) -> bool {
        self.triangles.is_empty()
    }

    pub fn triangle_count(&self) -> usize {
        self.triangles.len()
    }

    /// Axis-aligned extents of the mesh in hand space, or `None` when empty.
    pub fn extents(&self) -> Option<Vector3<f32>> {
        let mut min = Vector3::new(f32::INFINITY, f32::INFINITY, f32::INFINITY);
        let mut max = Vector3::new(f32::NEG_INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY);
        for triangle in &self.triangles {
            for corner in triangle {
                let p = corner.to_vec();
                min = Vector3::new(min.x.min(p.x), min.y.min(p.y), min.z.min(p.z));
                max = Vector3::new(max.x.max(p.x), max.y.max(p.y), max.z.max(p.z));
            }
        }
        min.x.is_finite().then(|| max - min)
    }

    /// Indices of the triangles whose bounding box overlaps `[min, max]`. Run
    /// once per finger over its whole curl sweep, so the inner loop only ever
    /// sees triangles that finger could reach.
    fn near(&self, min: Vector3<f32>, max: Vector3<f32>) -> Vec<usize> {
        self.triangles
            .iter()
            .enumerate()
            .filter(|(_, triangle)| {
                let mut lo = triangle[0].to_vec();
                let mut hi = lo;
                for corner in &triangle[1..] {
                    let p = corner.to_vec();
                    lo = Vector3::new(lo.x.min(p.x), lo.y.min(p.y), lo.z.min(p.z));
                    hi = Vector3::new(hi.x.max(p.x), hi.y.max(p.y), hi.z.max(p.z));
                }
                lo.x <= max.x
                    && hi.x >= min.x
                    && lo.y <= max.y
                    && hi.y >= min.y
                    && lo.z <= max.z
                    && hi.z >= min.z
            })
            .map(|(index, _)| index)
            .collect()
    }

    /// Whether any capsule has reached the surface.
    fn touches(&self, capsules: &[Capsule], candidates: &[usize]) -> bool {
        capsules.iter().any(|capsule| {
            candidates.iter().any(|index| {
                segment_triangle_distance(capsule.a, capsule.b, &self.triangles[*index])
                    < capsule.radius - CONTACT_TOLERANCE
            })
        })
    }
}

/// The family to use for an item of these hand-space extents, when nothing
/// authored says otherwise. Guns are named, not measured - a pistol's box
/// looks like any other handle - so [`GripFamily::Trigger`] only ever arrives
/// as a hint.
pub fn family_from_extents(extents: Vector3<f32>) -> GripFamily {
    let mut sorted = [extents.x.abs(), extents.y.abs(), extents.z.abs()];
    sorted.sort_by(f32::total_cmp);
    if sorted[0] < PINCH_THICKNESS {
        GripFamily::Pinch
    } else if sorted[1] > BROAD_GIRTH {
        GripFamily::Broad
    } else {
        GripFamily::Cylindrical
    }
}

/// Curl every finger until it reaches the item, within its family's envelope.
///
/// An empty mesh (a model with no triangles, a skinned melee rig) fits nothing
/// and every finger takes its cap - the same closed grip the glove posed
/// before there was a fit at all.
pub fn fit(rig: &mut impl HandRig, mesh: &ContactMesh, family: GripFamily) -> FingerAmounts {
    let mut amounts = FingerAmounts::default();
    for finger in Finger::ALL {
        finger.set(&mut amounts, fit_finger(rig, mesh, family, finger));
    }
    amounts
}

fn fit_finger(
    rig: &mut impl HandRig,
    mesh: &ContactMesh,
    family: GripFamily,
    finger: Finger,
) -> f32 {
    let Envelope { floor, cap } = family.envelope(finger);
    if cap <= floor || mesh.is_empty() {
        return cap.max(floor);
    }

    // One prefilter per finger, over everything the finger sweeps through.
    let (min, max) = swept_bounds(rig, finger, floor, cap);
    let candidates = mesh.near(min, max);
    if candidates.is_empty() {
        return cap;
    }

    let curl_at = |step: usize| floor + (cap - floor) * (step as f32 / COARSE_STEPS as f32);

    let mut clear = floor;
    let mut first_contact = None;
    for step in 1..=COARSE_STEPS {
        let curl = curl_at(step);
        if mesh.touches(&rig.phalanges(finger, curl), &candidates) {
            first_contact = Some(curl);
            break;
        }
        clear = curl;
    }

    let Some(mut blocked) = first_contact else {
        return cap;
    };

    for _ in 0..BISECT_ROUNDS {
        let middle = 0.5 * (clear + blocked);
        if mesh.touches(&rig.phalanges(finger, middle), &candidates) {
            blocked = middle;
        } else {
            clear = middle;
        }
    }
    clear
}

/// Bounding box of everything one finger passes through between `floor` and
/// `cap`, grown by the capsule radii.
fn swept_bounds(
    rig: &mut impl HandRig,
    finger: Finger,
    floor: f32,
    cap: f32,
) -> (Vector3<f32>, Vector3<f32>) {
    let mut min = Vector3::new(f32::INFINITY, f32::INFINITY, f32::INFINITY);
    let mut max = Vector3::new(f32::NEG_INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY);
    for step in 0..=COARSE_STEPS {
        let curl = floor + (cap - floor) * (step as f32 / COARSE_STEPS as f32);
        for capsule in rig.phalanges(finger, curl) {
            let radius = Vector3::new(capsule.radius, capsule.radius, capsule.radius);
            for point in [capsule.a.to_vec(), capsule.b.to_vec()] {
                let lo = point - radius;
                let hi = point + radius;
                min = Vector3::new(min.x.min(lo.x), min.y.min(lo.y), min.z.min(lo.z));
                max = Vector3::new(max.x.max(hi.x), max.y.max(hi.y), max.z.max(hi.z));
            }
        }
    }
    (min, max)
}

// --- geometry -------------------------------------------------------------

/// Distance from segment `pq` to a triangle; zero when they cross.
fn segment_triangle_distance(p: Point3<f32>, q: Point3<f32>, tri: &[Point3<f32>; 3]) -> f32 {
    if segment_crosses_triangle(p, q, tri) {
        return 0.0;
    }
    let mut best = point_triangle_distance(p, tri).min(point_triangle_distance(q, tri));
    for edge in 0..3 {
        best = best.min(segment_segment_distance(
            p,
            q,
            tri[edge],
            tri[(edge + 1) % 3],
        ));
    }
    best
}

/// Moller-Trumbore, restricted to the segment's own parameter range. Without
/// it a segment threading clean through a triangle would report the distance
/// to its nearest edge rather than zero.
fn segment_crosses_triangle(p: Point3<f32>, q: Point3<f32>, tri: &[Point3<f32>; 3]) -> bool {
    let direction = q - p;
    let edge1 = tri[1] - tri[0];
    let edge2 = tri[2] - tri[0];
    let h = direction.cross(edge2);
    let determinant = edge1.dot(h);
    if determinant.abs() < 1e-12 {
        return false;
    }
    let inverse = 1.0 / determinant;
    let s = p - tri[0];
    let u = inverse * s.dot(h);
    if !(0.0..=1.0).contains(&u) {
        return false;
    }
    let r = s.cross(edge1);
    let v = inverse * direction.dot(r);
    if v < 0.0 || u + v > 1.0 {
        return false;
    }
    let t = inverse * edge2.dot(r);
    (0.0..=1.0).contains(&t)
}

/// Distance from `p` to the closest point on a triangle (Ericson, *Real-Time
/// Collision Detection*): test the vertex, edge and face regions in turn.
fn point_triangle_distance(p: Point3<f32>, tri: &[Point3<f32>; 3]) -> f32 {
    let (a, b, c) = (tri[0], tri[1], tri[2]);
    let ab = b - a;
    let ac = c - a;
    let ap = p - a;

    let d1 = ab.dot(ap);
    let d2 = ac.dot(ap);
    if d1 <= 0.0 && d2 <= 0.0 {
        return ap.magnitude();
    }

    let bp = p - b;
    let d3 = ab.dot(bp);
    let d4 = ac.dot(bp);
    if d3 >= 0.0 && d4 <= d3 {
        return bp.magnitude();
    }

    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        let v = d1 / (d1 - d3);
        return (ap - ab * v).magnitude();
    }

    let cp = p - c;
    let d5 = ab.dot(cp);
    let d6 = ac.dot(cp);
    if d6 >= 0.0 && d5 <= d6 {
        return cp.magnitude();
    }

    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        let w = d2 / (d2 - d6);
        return (ap - ac * w).magnitude();
    }

    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
        let w = (d4 - d3) / ((d4 - d3) + (d5 - d6));
        return (p - (b + (c - b) * w)).magnitude();
    }

    let denominator = 1.0 / (va + vb + vc);
    let v = vb * denominator;
    let w = vc * denominator;
    (ap - (ab * v + ac * w)).magnitude()
}

/// Distance between two segments, clamped to both parameter ranges.
fn segment_segment_distance(
    p1: Point3<f32>,
    q1: Point3<f32>,
    p2: Point3<f32>,
    q2: Point3<f32>,
) -> f32 {
    let d1 = q1 - p1;
    let d2 = q2 - p2;
    let r = p1 - p2;
    let a = d1.dot(d1);
    let e = d2.dot(d2);
    let f = d2.dot(r);

    const EPSILON: f32 = 1e-12;
    let (s, t) = if a <= EPSILON && e <= EPSILON {
        (0.0, 0.0)
    } else if a <= EPSILON {
        (0.0, (f / e).clamp(0.0, 1.0))
    } else {
        let c = d1.dot(r);
        if e <= EPSILON {
            ((-c / a).clamp(0.0, 1.0), 0.0)
        } else {
            let b = d1.dot(d2);
            let denominator = a * e - b * b;
            let s = if denominator > EPSILON {
                ((b * f - c * e) / denominator).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let t = (b * s + f) / e;
            if t < 0.0 {
                ((-c / a).clamp(0.0, 1.0), 0.0)
            } else if t > 1.0 {
                (((b - c) / a).clamp(0.0, 1.0), 1.0)
            } else {
                (s, t)
            }
        }
    };

    ((p1 + d1 * s) - (p2 + d2 * t)).magnitude()
}

#[cfg(test)]
mod tests {
    use super::*;
    use cgmath::{Matrix3, Rad, point3, vec3};

    /// Phalanx thickness the synthetic hand tests with.
    const TEST_RADIUS: f32 = meters(0.008);
    const PHALANX: f32 = meters(0.032);

    /// A flat five-finger hand: every finger starts at the palm pointing down
    /// -Z and curls toward +Y, one bend per phalanx. Enough geometry to
    /// exercise the solver without loading the glove.
    struct FlatHand;

    impl HandRig for FlatHand {
        fn phalanges(&mut self, finger: Finger, curl: f32) -> Vec<Capsule> {
            let x = match finger {
                Finger::Thumb => meters(0.035),
                Finger::Index => meters(0.018),
                Finger::Middle => 0.0,
                Finger::Ring => meters(-0.018),
                Finger::Pinky => meters(-0.035),
            };
            let mut origin = point3(x, 0.0, 0.0);
            let mut direction = vec3(0.0, 0.0, -1.0);
            // 60 degrees a joint: a full curl brings the finger through a
            // half turn, which is what closes it round a handle.
            let bend = Rad(curl * std::f32::consts::FRAC_PI_3);
            (0..3)
                .map(|_| {
                    direction = Matrix3::from_angle_x(bend) * direction;
                    let end = origin + direction * PHALANX;
                    let capsule = Capsule {
                        a: origin,
                        b: end,
                        radius: TEST_RADIUS,
                    };
                    origin = end;
                    capsule
                })
                .collect()
        }
    }

    fn quad(
        a: Point3<f32>,
        b: Point3<f32>,
        c: Point3<f32>,
        d: Point3<f32>,
    ) -> Vec<[Point3<f32>; 3]> {
        vec![[a, b, c], [a, c, d]]
    }

    /// An axis-aligned box, as 12 triangles.
    fn box_mesh(center: Vector3<f32>, half: Vector3<f32>) -> Vec<[Point3<f32>; 3]> {
        let corner = |sx: f32, sy: f32, sz: f32| {
            point3(
                center.x + sx * half.x,
                center.y + sy * half.y,
                center.z + sz * half.z,
            )
        };
        let mut triangles = Vec::new();
        for sign in [-1.0f32, 1.0] {
            triangles.extend(quad(
                corner(sign, -1.0, -1.0),
                corner(sign, 1.0, -1.0),
                corner(sign, 1.0, 1.0),
                corner(sign, -1.0, 1.0),
            ));
            triangles.extend(quad(
                corner(-1.0, sign, -1.0),
                corner(1.0, sign, -1.0),
                corner(1.0, sign, 1.0),
                corner(-1.0, sign, 1.0),
            ));
            triangles.extend(quad(
                corner(-1.0, -1.0, sign),
                corner(1.0, -1.0, sign),
                corner(1.0, 1.0, sign),
                corner(-1.0, 1.0, sign),
            ));
        }
        triangles
    }

    /// A tube whose axis runs along X - the handle the fingers wrap.
    fn cylinder_mesh(center: Vector3<f32>, radius: f32, half_length: f32) -> Vec<[Point3<f32>; 3]> {
        const SEGMENTS: usize = 48;
        let ring = |step: usize| {
            let angle = step as f32 / SEGMENTS as f32 * std::f32::consts::TAU;
            (
                center.y + radius * angle.cos(),
                center.z + radius * angle.sin(),
            )
        };
        (0..SEGMENTS)
            .flat_map(|step| {
                let (y0, z0) = ring(step);
                let (y1, z1) = ring(step + 1);
                quad(
                    point3(center.x - half_length, y0, z0),
                    point3(center.x + half_length, y0, z0),
                    point3(center.x + half_length, y1, z1),
                    point3(center.x - half_length, y1, z1),
                )
            })
            .collect()
    }

    /// A UV sphere - something too big to wrap.
    fn sphere_mesh(center: Vector3<f32>, radius: f32) -> Vec<[Point3<f32>; 3]> {
        const RINGS: usize = 24;
        const SEGMENTS: usize = 32;
        let point = |ring: usize, segment: usize| {
            let phi = ring as f32 / RINGS as f32 * std::f32::consts::PI;
            let theta = segment as f32 / SEGMENTS as f32 * std::f32::consts::TAU;
            point3(
                center.x + radius * phi.sin() * theta.cos(),
                center.y + radius * phi.cos(),
                center.z + radius * phi.sin() * theta.sin(),
            )
        };
        (0..RINGS)
            .flat_map(|ring| {
                (0..SEGMENTS).flat_map(move |segment| {
                    quad(
                        point(ring, segment),
                        point(ring + 1, segment),
                        point(ring + 1, segment + 1),
                        point(ring, segment + 1),
                    )
                })
            })
            .collect()
    }

    /// Whether the hand, curled by `amounts`, is inside the mesh anywhere.
    fn penetrates(mesh: &ContactMesh, amounts: &FingerAmounts) -> bool {
        let all = (0..mesh.triangle_count()).collect::<Vec<_>>();
        Finger::ALL.iter().any(|finger| {
            let curl = match finger {
                Finger::Thumb => amounts.thumb,
                Finger::Index => amounts.index,
                Finger::Middle => amounts.middle,
                Finger::Ring => amounts.ring,
                Finger::Pinky => amounts.pinky,
            };
            mesh.touches(&FlatHand.phalanges(*finger, curl), &all)
        })
    }

    fn closed_fist() -> FingerAmounts {
        FingerAmounts {
            thumb: 1.0,
            index: 1.0,
            middle: 1.0,
            ring: 1.0,
            pinky: 1.0,
        }
    }

    /// The whole point of the fit, as a negative test and its fix in one: the
    /// ungoverned fist closes straight through a handle, and the fitted curl
    /// stops on its surface.
    #[test]
    fn fingers_stop_on_a_handle_the_fist_would_close_through() {
        let mesh = ContactMesh::new(cylinder_mesh(
            vec3(0.0, meters(0.028), meters(-0.008)),
            meters(0.015),
            meters(0.06),
        ));

        assert!(
            penetrates(&mesh, &closed_fist()),
            "a full fist should close through a handle - otherwise this test proves nothing"
        );

        let fitted = fit(&mut FlatHand, &mesh, GripFamily::Cylindrical);
        assert!(
            !penetrates(&mesh, &fitted),
            "the fitted grip still penetrates: {fitted:?}"
        );
        assert!(
            fitted.middle > 0.05 && fitted.middle < 1.0,
            "the middle finger should curl partway onto the handle, got {}",
            fitted.middle
        );
    }

    /// Nothing in the hand means nothing to stop against: every finger takes
    /// its family's cap.
    #[test]
    fn an_empty_hand_closes_all_the_way() {
        let fitted = fit(
            &mut FlatHand,
            &ContactMesh::new(Vec::new()),
            GripFamily::Cylindrical,
        );
        assert_eq!(fitted.middle, 1.0);
        assert_eq!(fitted.thumb, 1.0);
    }

    /// A ball wider than the palm reads as broad, and the hand rests on its
    /// surface rather than wrapping it.
    #[test]
    fn a_ball_is_broad_and_barely_curls() {
        let radius = meters(0.12);
        let mesh = ContactMesh::new(sphere_mesh(
            vec3(0.0, radius + meters(0.02), meters(-0.03)),
            radius,
        ));
        let extents = mesh.extents().expect("sphere has extents");

        assert_eq!(family_from_extents(extents), GripFamily::Broad);

        let fitted = fit(&mut FlatHand, &mesh, GripFamily::Broad);
        assert!(
            !penetrates(&mesh, &fitted),
            "hand inside the ball: {fitted:?}"
        );
        assert!(
            fitted.middle <= BROAD_CAP,
            "a broad grip must stay shallow, got {}",
            fitted.middle
        );
    }

    /// A slab thinner than a finger is pinched: thumb and index close on it,
    /// the other three hold their rest curl.
    #[test]
    fn a_thin_slab_is_pinched() {
        let mesh = ContactMesh::new(box_mesh(
            vec3(0.0, meters(0.03), meters(-0.05)),
            vec3(meters(0.06), meters(0.005), meters(0.04)),
        ));
        let extents = mesh.extents().expect("slab has extents");

        assert_eq!(family_from_extents(extents), GripFamily::Pinch);

        let fitted = fit(&mut FlatHand, &mesh, GripFamily::Pinch);
        assert_eq!(fitted.middle, PINCH_REST);
        assert_eq!(fitted.ring, PINCH_REST);
        assert_eq!(fitted.pinky, PINCH_REST);
        assert!(
            fitted.index < 1.0,
            "the index should stop on the slab, got {}",
            fitted.index
        );
    }

    /// A handle-sized item is neither thin nor fat: the ordinary wrap.
    #[test]
    fn a_handle_sized_item_is_cylindrical() {
        assert_eq!(
            family_from_extents(vec3(meters(0.04), meters(0.09), meters(0.04))),
            GripFamily::Cylindrical
        );
    }

    /// The trigger family holds the index out on the trigger while the rest of
    /// the hand closes on the grip - the shape that makes a held gun read as
    /// held rather than squeezed.
    #[test]
    fn a_trigger_grip_leaves_the_index_out() {
        let mesh = ContactMesh::new(cylinder_mesh(
            vec3(0.0, meters(0.028), meters(-0.008)),
            meters(0.008),
            meters(0.06),
        ));

        let fitted = fit(&mut FlatHand, &mesh, GripFamily::Trigger);
        assert!(fitted.index <= TRIGGER_INDEX_CAP);
        assert!(
            fitted.index < fitted.middle && fitted.index < fitted.ring,
            "index {} should trail the wrapping fingers ({}, {})",
            fitted.index,
            fitted.middle,
            fitted.ring
        );
    }

    /// A segment threading through a triangle is touching it, not a finger's
    /// width away from its nearest edge.
    #[test]
    fn a_segment_through_a_triangle_reads_as_contact() {
        let triangle = [
            point3(-1.0, 0.0, -1.0),
            point3(1.0, 0.0, -1.0),
            point3(0.0, 0.0, 1.0),
        ];
        assert_eq!(
            segment_triangle_distance(point3(0.0, -1.0, 0.0), point3(0.0, 1.0, 0.0), &triangle),
            0.0
        );
        assert!(
            (segment_triangle_distance(point3(0.0, 0.5, 0.0), point3(0.0, 1.5, 0.0), &triangle)
                - 0.5)
                .abs()
                < 1e-5
        );
    }
}
