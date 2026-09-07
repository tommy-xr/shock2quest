//! Where an item with no authored grip sits in the hand.
//!
//! The finger fit ([`crate::hand_fit`]) measures how far each finger closes on
//! the item's surface, which only says anything once the item is somewhere a
//! hand could hold it - and specifically somewhere the **open** hand is not
//! already touching, because a finger that starts in contact yields nothing and
//! keeps its authored wrap. A world pickup is drawn centred on the wrist, so
//! this module puts it against the open palm first, from the one thing every
//! model has: the box its geometry occupies.
//!
//! Hand space is the glove's own, and it is not axis-aligned to the palm: the
//! rig's palm plane runs obliquely through it. So the seat works in a **palm
//! frame** measured off the open glove ([`PALM_CENTRE`], [`PALM_NORMAL`],
//! [`PALM_CURL_AXIS`]) rather than in hand X/Y/Z. Units are world units
//! (1 ~ 0.762 m), the space the seat offsets in [`crate::vr_grips`] are
//! written in.

use cgmath::{InnerSpace, Matrix3, Quaternion, Rotation, SquareMatrix, Vector3, vec3};

use crate::hand_fit::{self, GripFamily};

/// Centre of the palm, in hand space: the mean of the four fingers'
/// metacarpal bases and knuckles at the open pose - the middle of the flat
/// the palm presents, roughly halfway between wrist and knuckles.
///
/// Measured off the shipped rig; `hand_glove`'s
/// `the_palm_frame_matches_the_glove_rig` re-measures the whole frame and
/// fails if the glove ever moves under it.
pub const PALM_CENTRE: Vector3<f32> = vec3(0.005287, -0.000243, -0.058655);

/// Out of the palm, in hand space - the side the fingers curl toward, so the
/// side a held item sits on.
pub const PALM_NORMAL: Vector3<f32> = vec3(-0.97836, 0.15474, -0.13728);

/// Across the palm, index knuckle to pinky knuckle: the axis the fingers curl
/// about, so the axis a rod lies along when the fist closes on it.
pub const PALM_CURL_AXIS: Vector3<f32> = vec3(-0.17354, -0.97693, 0.13556);

/// Where thumb and index meet when the open hand closes on something thin: the
/// midpoint of their tips at the open pose. A pinched item straddles this.
pub const PINCH_POINT: Vector3<f32> = vec3(-0.05139, 0.055755, -0.168875);

/// How far the palm's skin is from the joints the frame is measured through,
/// in world units - roughly half the thickness of a hand.
///
/// The frame runs through a plane of *joints*, which sit inside the flesh; an
/// item rested on it would be buried in the palm and the fingers would start
/// the fit already inside it (which the fit reads as "leave the authored wrap
/// alone"). Items sit on the skin instead.
const PALM_DEPTH: f32 = 0.012 / crate::METERS_PER_WORLD_UNIT;

/// How far toward the knuckles a wrapped item sits from the palm's centre, in
/// world units.
///
/// A grip is not held in the middle of the palm: it sits under the base of the
/// fingers, which is the only stretch of palm both the fingers and the *thumb*
/// close over. Centred on the palm proper, a handle falls behind the thumb's
/// whole swing and the thumb closes past it into a fist.
const GRIP_SEAT_ALONG: f32 = 0.015 / crate::METERS_PER_WORLD_UNIT;

/// The palm frame as the seat uses it: `(across, out, along)` - across the palm
/// (the curl axis), out of the palm, and along the fingers wrist-to-tips.
///
/// The written-down axes are transcribed measurements and so are only unit and
/// perpendicular to within a thousandth; everything here is a projection onto
/// them, which needs them exact. Orthonormalized about the normal, and the
/// third derived rather than written down so the three can never disagree.
pub fn palm_frame() -> (Vector3<f32>, Vector3<f32>, Vector3<f32>) {
    let out = PALM_NORMAL.normalize();
    let across = (PALM_CURL_AXIS - out * PALM_CURL_AXIS.dot(out)).normalize();
    (across, out, across.cross(out))
}

/// A held item's placement in hand space: where its model origin goes, and how
/// its geometry is turned to get there.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Seat {
    pub offset: Vector3<f32>,
    pub rotation: Quaternion<f32>,
}

/// Seat a model-space box against the **open** hand, for the finger fit to
/// close on.
///
/// The family is read off the box first, because it decides both how the box
/// is turned and where against the hand it goes:
///
/// - wrapped or splayed on (cylindrical, broad, and an unprofiled trigger):
///   the longest axis lies across the palm, the shortest faces it, and the
///   nearest face rests on the palm skin at [`PALM_CENTRE`] - out of the open
///   fingers' way, so every one of them has room to close;
/// - pinched: the slab stands on edge between the open thumb and index pads,
///   its thin axis along the palm normal and its long axis running out past
///   the fingertips, with its near edge at [`PINCH_POINT`].
///
/// `authored_rotation` is the turn a grip profile already specifies; it is kept
/// exactly, and only the placement is solved.
pub fn seat(
    min: Vector3<f32>,
    max: Vector3<f32>,
    authored_rotation: Option<Quaternion<f32>>,
) -> (Seat, GripFamily) {
    let half = (max - min) * 0.5;
    let family = hand_fit::family_from_extents(max - min);
    let rotation = authored_rotation.unwrap_or_else(|| seating_turn(max - min, family));

    // Support of the turned box along `axis`: how far its surface reaches from
    // its own centre that way. A rotated box is not axis-aligned in hand
    // space, and the palm frame is oblique to it either way, so this is
    // measured against the axis rather than read off a re-bound AABB.
    let support = |axis: Vector3<f32>| {
        let along = |unit: Vector3<f32>| axis.dot(rotation.rotate_vector(unit)).abs();
        along(Vector3::unit_x()) * half.x
            + along(Vector3::unit_y()) * half.y
            + along(Vector3::unit_z()) * half.z
    };

    let (_, out, along) = palm_frame();
    let target = match family {
        // The near face on the palm skin, the rest of the item out in front of
        // it - and pushed toward the knuckles, which is the stretch of palm the
        // fingers *and* the thumb can both reach round.
        GripFamily::Cylindrical | GripFamily::Broad | GripFamily::Trigger => {
            PALM_CENTRE + out * (PALM_DEPTH + support(out)) + along * GRIP_SEAT_ALONG
        }
        // Straddling the pinch line, running away from the hand: the pads meet
        // its near edge and close onto its faces.
        GripFamily::Pinch => PINCH_POINT + along * support(along),
    };

    let centre = rotation.rotate_vector((min + max) * 0.5);
    (
        Seat {
            offset: target - centre,
            rotation,
        },
        family,
    )
}

/// The turn that lays a box into the palm frame for its family.
///
/// A signed permutation of the model axes onto the frame's own axes, composed
/// with the frame. Kept a proper rotation (determinant +1) throughout, so it
/// never reflects the geometry.
fn seating_turn(extents: Vector3<f32>, family: GripFamily) -> Quaternion<f32> {
    let (across, out, along) = palm_frame();
    // Where the box's longest / shortest / middle axes are sent.
    let frame = match family {
        // Long axis across the palm (a rod through the fist), flat side down.
        GripFamily::Cylindrical | GripFamily::Broad | GripFamily::Trigger => {
            Matrix3::from_cols(across, out, along)
        }
        // Long axis out past the fingertips, thin axis facing the palm, so the
        // pads land on the faces. `-across` keeps the triple right-handed.
        GripFamily::Pinch => Matrix3::from_cols(along, out, -across),
    };
    Quaternion::from(frame * axis_order(extents))
}

/// The signed permutation that sends a box's longest model axis to local X,
/// its shortest to local Y and the remaining one to local Z.
fn axis_order(extents: Vector3<f32>) -> Matrix3<f32> {
    let mut order = [0usize, 1, 2];
    let size = [extents.x.abs(), extents.y.abs(), extents.z.abs()];
    order.sort_by(|a, b| size[*b].total_cmp(&size[*a]));
    let [longest, middle, shortest] = order;

    let mut columns = [Vector3::unit_x(); 3];
    columns[longest] = Vector3::unit_x();
    columns[middle] = Vector3::unit_z();
    columns[shortest] = Vector3::unit_y();
    let mut matrix = Matrix3::from_cols(columns[0], columns[1], columns[2]);
    if matrix.determinant() < 0.0 {
        matrix.z = -matrix.z;
    }
    matrix
}

#[cfg(test)]
mod tests {
    use super::*;
    use cgmath::{Deg, Rotation3};

    /// Metres in the world units the hand frame is measured in.
    fn meters(m: f32) -> f32 {
        m / crate::METERS_PER_WORLD_UNIT
    }

    fn box_of(half: Vector3<f32>) -> (Vector3<f32>, Vector3<f32>) {
        (-half, half)
    }

    /// Where a seated box's corners land in the palm frame: `(normal, curl,
    /// fingerward)` components of each of the eight.
    fn seated_in_frame(min: Vector3<f32>, max: Vector3<f32>, seat: Seat) -> Vec<(f32, f32, f32)> {
        let (across, out, along) = palm_frame();
        let mut corners = Vec::new();
        for i in 0..8 {
            let corner = vec3(
                if i & 1 == 0 { min.x } else { max.x },
                if i & 2 == 0 { min.y } else { max.y },
                if i & 4 == 0 { min.z } else { max.z },
            );
            let p = seat.rotation.rotate_vector(corner) + seat.offset - PALM_CENTRE;
            corners.push((p.dot(out), p.dot(across), p.dot(along)));
        }
        corners
    }

    /// The palm frame is orthonormal and right-handed - every projection the
    /// seat takes assumes it, and the axes it is built from are transcribed
    /// measurements rather than exact numbers.
    #[test]
    fn the_palm_frame_is_an_orthonormal_right_handed_basis() {
        let (across, out, along) = palm_frame();
        for axis in [across, out, along] {
            assert!(
                (axis.magnitude() - 1.0).abs() < 1e-5,
                "{axis:?} is not unit"
            );
        }
        assert!(out.dot(across).abs() < 1e-5);
        assert!(out.dot(along).abs() < 1e-5);
        assert!(across.dot(along).abs() < 1e-5);
        assert!(
            (across.cross(out) - along).magnitude() < 1e-5,
            "the frame is left-handed"
        );
        // Orthonormalizing must not have turned the axes into different ones.
        assert!(out.dot(PALM_NORMAL.normalize()) > 0.999);
        assert!(across.dot(PALM_CURL_AXIS.normalize()) > 0.999);
    }

    /// A rod lies across the palm - along the axis the fingers curl about -
    /// with its near face on the palm skin.
    #[test]
    fn a_rod_lies_across_the_palm() {
        let (min, max) = box_of(vec3(
            meters(0.15) * 0.5,
            meters(0.03) * 0.5,
            meters(0.04) * 0.5,
        ));

        let (seat, family) = seat(min, max, None);

        let corners = seated_in_frame(min, max, seat);
        let span = |pick: fn(&(f32, f32, f32)) -> f32| {
            let lo = corners.iter().map(pick).fold(f32::INFINITY, f32::min);
            let hi = corners.iter().map(pick).fold(f32::NEG_INFINITY, f32::max);
            (lo, hi - lo)
        };
        let (near, depth) = span(|c| c.0);
        let (_, across) = span(|c| c.1);
        let (_, along) = span(|c| c.2);
        assert!(
            across > along && across > depth,
            "the long axis should run across the palm, got across {across} along {along} depth {depth}"
        );
        assert!(
            (near - PALM_DEPTH).abs() < 1e-5,
            "the rod's near face should rest on the palm skin, got {near}"
        );
        assert_eq!(family, GripFamily::Cylindrical);
    }

    /// A ball's surface touches the palm skin too - the seat drops the surface,
    /// not the origin, so shape does not change where contact happens.
    #[test]
    fn a_ball_rests_on_the_palm() {
        let radius = meters(0.35) * 0.5;
        let (min, max) = box_of(vec3(radius, radius, radius));

        let (seat, family) = seat(min, max, None);

        let near = seated_in_frame(min, max, seat)
            .iter()
            .map(|c| c.0)
            .fold(f32::INFINITY, f32::min);
        assert!(
            (near - PALM_DEPTH).abs() < 1e-5,
            "the ball should touch the palm, got {near}"
        );
        assert_eq!(family, GripFamily::Broad);
    }

    /// A slab stands on edge at the pinch line: thin side facing the palm, near
    /// edge at the pads, the rest of it out past the fingertips.
    #[test]
    fn a_slab_stands_on_edge_at_the_pinch_line() {
        let (min, max) = box_of(vec3(
            meters(0.35) * 0.5,
            meters(0.02) * 0.5,
            meters(0.48) * 0.5,
        ));

        let (seat, family) = seat(min, max, None);
        assert_eq!(family, GripFamily::Pinch);

        let corners = seated_in_frame(min, max, seat);
        let pinch = PINCH_POINT - PALM_CENTRE;
        let (_, out, fingerward) = palm_frame();
        let (pinch_n, pinch_f) = (pinch.dot(out), pinch.dot(fingerward));

        let depth = corners
            .iter()
            .map(|c| c.0)
            .fold(f32::NEG_INFINITY, f32::max)
            - corners.iter().map(|c| c.0).fold(f32::INFINITY, f32::min);
        let along_lo = corners.iter().map(|c| c.2).fold(f32::INFINITY, f32::min);
        let along_hi = corners
            .iter()
            .map(|c| c.2)
            .fold(f32::NEG_INFINITY, f32::max);
        let across = corners
            .iter()
            .map(|c| c.1)
            .fold(f32::NEG_INFINITY, f32::max)
            - corners.iter().map(|c| c.1).fold(f32::INFINITY, f32::min);

        assert!(
            depth < across && depth < (along_hi - along_lo),
            "the thin axis should face the palm, got depth {depth}"
        );
        assert!(
            (along_lo - pinch_f).abs() < 1e-5,
            "the near edge should sit at the pinch point, got {along_lo} vs {pinch_f}"
        );
        let mid_n = 0.5
            * (corners.iter().map(|c| c.0).fold(f32::INFINITY, f32::min)
                + corners
                    .iter()
                    .map(|c| c.0)
                    .fold(f32::NEG_INFINITY, f32::max));
        assert!(
            (mid_n - pinch_n).abs() < 1e-5,
            "the slab's mid-plane should be on the pinch line, got {mid_n} vs {pinch_n}"
        );
    }

    /// An authored turn is kept as authored - the profile decides which way the
    /// item faces - and only the placement is solved.
    #[test]
    fn an_authored_rotation_is_kept_and_only_the_placement_is_solved() {
        let (min, max) = box_of(vec3(meters(0.1), meters(0.02), meters(0.03)));
        let authored = Quaternion::from_angle_y(Deg(-90.0));

        let (seat, _) = seat(min, max, Some(authored));

        assert!((seat.rotation.s - authored.s).abs() < 1e-6);
        assert_eq!(seat.rotation, authored);
    }

    /// A permutation with an odd number of swaps must not sneak in as a
    /// reflection: mirrored geometry renders inside-out.
    #[test]
    fn the_seating_turn_never_reflects_the_model() {
        for extents in [
            vec3(3.0, 2.0, 1.0),
            vec3(1.0, 2.0, 3.0),
            vec3(2.0, 3.0, 1.0),
            vec3(1.0, 3.0, 2.0),
            vec3(3.0, 1.0, 2.0),
            vec3(2.0, 1.0, 3.0),
        ] {
            for family in GripFamily::ALL {
                let rotation = seating_turn(extents, family);
                let cross = rotation
                    .rotate_vector(Vector3::unit_x())
                    .cross(rotation.rotate_vector(Vector3::unit_y()));
                assert!(
                    (cross - rotation.rotate_vector(Vector3::unit_z())).magnitude() < 1e-3,
                    "{extents:?} produced a reflection for {}",
                    family.as_str()
                );
            }
        }
    }
}
