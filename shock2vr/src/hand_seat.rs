//! Where an item with no authored grip sits in the hand.
//!
//! The finger fit ([`crate::hand_fit`]) measures how far each finger closes on
//! the item's surface, which only says anything once the item is somewhere a
//! hand could hold it. A world pickup is drawn centred on the wrist, so this
//! module puts it in the palm first, from the one thing every model has: the
//! box its geometry occupies.
//!
//! Hand space is the glove's own: **+X toward the thumb side** of the right
//! hand, **+Y out of the back of the hand** (so the palm faces -Y), **-Z along
//! the fingers**. Units are world units (1 ~ 0.762 m), the space the seat
//! offsets in [`crate::vr_grips`] are written in.

use cgmath::{Matrix3, Quaternion, SquareMatrix, Vector3, vec3};

use crate::hand_fit::{self, GripFamily};

/// Where the palm's surface is, in hand space: the midpoint of the index and
/// pinky knuckles at the glove's open pose - the line an item resting in the
/// hand touches.
///
/// Measured off the shipped rig; `hand_glove`'s
/// `the_palm_anchor_matches_the_glove_rig` re-measures it and fails if the
/// glove ever moves under it.
pub const PALM_ANCHOR: Vector3<f32> = vec3(0.0055, -0.0002, -0.0971);

/// A held item's placement in hand space: where its model origin goes, and how
/// its geometry is turned to get there.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Seat {
    pub offset: Vector3<f32>,
    pub rotation: Quaternion<f32>,
}

/// Seat a model-space box in the hand.
///
/// `authored_rotation` is the turn a grip profile already specifies; with none,
/// the box is turned so its **longest** axis runs across the palm (hand X, the
/// axis the fingers curl about - a rod lies through the fist), its **shortest**
/// faces the palm (hand Y), and the remaining one runs along the fingers. Either
/// way the box is then dropped onto the palm: its surface against
/// [`PALM_ANCHOR`], which is half its palm-facing extent below the anchor.
///
/// The family follows the seated box, so what the fingers are allowed to do
/// matches what they are closing on: thin is pinched, wider than the palm is
/// splayed on, anything else is wrapped.
pub fn seat(
    min: Vector3<f32>,
    max: Vector3<f32>,
    authored_rotation: Option<Quaternion<f32>>,
) -> (Seat, GripFamily) {
    let rotation = authored_rotation.unwrap_or_else(|| across_the_palm(max - min));

    // The box turned into hand space. A rotation of an axis-aligned box is not
    // axis-aligned, so re-bound the eight corners rather than permuting extents.
    let (hand_min, hand_max) = rotated_bounds(min, max, rotation);
    let extents = hand_max - hand_min;
    let centre = (hand_min + hand_max) * 0.5;

    // The palm faces -Y, so the item hangs below the anchor by half its depth.
    let target = PALM_ANCHOR - vec3(0.0, extents.y * 0.5, 0.0);
    (
        Seat {
            offset: target - centre,
            rotation,
        },
        hand_fit::family_from_extents(extents),
    )
}

/// The turn that lays a box's longest axis across the palm and its shortest
/// against it. A signed permutation of the model axes, kept a proper rotation
/// (determinant +1) so it never reflects the geometry.
fn across_the_palm(extents: Vector3<f32>) -> Quaternion<f32> {
    let mut order = [0usize, 1, 2];
    let size = [extents.x.abs(), extents.y.abs(), extents.z.abs()];
    order.sort_by(|a, b| size[*b].total_cmp(&size[*a]));
    let [longest, middle, shortest] = order;

    // Column j is where model axis j lands: longest across the palm, shortest
    // through it, the remainder along the fingers.
    let mut columns = [Vector3::unit_x(); 3];
    columns[longest] = Vector3::unit_x();
    columns[middle] = Vector3::unit_z();
    columns[shortest] = Vector3::unit_y();
    let mut matrix = Matrix3::from_cols(columns[0], columns[1], columns[2]);
    if matrix.determinant() < 0.0 {
        matrix.z = -matrix.z;
    }
    Quaternion::from(matrix)
}

/// The axis-aligned bounds of `[min, max]` after `rotation`.
fn rotated_bounds(
    min: Vector3<f32>,
    max: Vector3<f32>,
    rotation: Quaternion<f32>,
) -> (Vector3<f32>, Vector3<f32>) {
    use cgmath::Rotation;

    let mut lo = vec3(f32::INFINITY, f32::INFINITY, f32::INFINITY);
    let mut hi = vec3(f32::NEG_INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY);
    for corner in 0..8 {
        let pick = |axis: usize, low: f32, high: f32| {
            if corner & (1 << axis) == 0 { low } else { high }
        };
        let point = rotation.rotate_vector(vec3(
            pick(0, min.x, max.x),
            pick(1, min.y, max.y),
            pick(2, min.z, max.z),
        ));
        lo = vec3(lo.x.min(point.x), lo.y.min(point.y), lo.z.min(point.z));
        hi = vec3(hi.x.max(point.x), hi.y.max(point.y), hi.z.max(point.z));
    }
    (lo, hi)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cgmath::{Deg, InnerSpace, Rotation, Rotation3};

    /// Metres in the world units the hand frame is measured in.
    fn meters(m: f32) -> f32 {
        m / crate::METERS_PER_WORLD_UNIT
    }

    fn box_of(half: Vector3<f32>) -> (Vector3<f32>, Vector3<f32>) {
        (-half, half)
    }

    /// The seated box's own hand-space bounds - what the fingers see.
    fn seated_bounds(
        min: Vector3<f32>,
        max: Vector3<f32>,
        seat: Seat,
    ) -> (Vector3<f32>, Vector3<f32>) {
        let (lo, hi) = rotated_bounds(min, max, seat.rotation);
        (lo + seat.offset, hi + seat.offset)
    }

    #[test]
    fn a_rod_lies_across_the_palm() {
        let (min, max) = box_of(vec3(
            meters(0.15) * 0.5,
            meters(0.03) * 0.5,
            meters(0.04) * 0.5,
        ));

        let (seat, family) = seat(min, max, None);

        let (lo, hi) = seated_bounds(min, max, seat);
        let extents = hi - lo;
        assert!(
            extents.x > extents.y && extents.x > extents.z,
            "the long axis should run across the palm, got {extents:?}"
        );
        assert_eq!(family, GripFamily::Cylindrical);
    }

    #[test]
    fn a_slab_is_pinched_flat_against_the_palm() {
        let (min, max) = box_of(vec3(
            meters(0.35) * 0.5,
            meters(0.02) * 0.5,
            meters(0.48) * 0.5,
        ));

        let (seat, family) = seat(min, max, None);

        let (lo, hi) = seated_bounds(min, max, seat);
        let extents = hi - lo;
        assert!(
            extents.y < extents.x && extents.y < extents.z,
            "the thin axis should face the palm, got {extents:?}"
        );
        assert_eq!(family, GripFamily::Pinch);
    }

    #[test]
    fn a_ball_rests_on_the_palm() {
        let radius = meters(0.35) * 0.5;
        let (min, max) = box_of(vec3(radius, radius, radius));

        let (seat, family) = seat(min, max, None);

        let (_, hi) = seated_bounds(min, max, seat);
        assert!(
            (hi.y - PALM_ANCHOR.y).abs() < 1e-5,
            "the ball's top should touch the palm, got {}",
            hi.y
        );
        assert_eq!(family, GripFamily::Broad);
    }

    /// Everything sits against the palm, whatever its shape: the surface the
    /// fingers close onto is the anchor, not the model origin.
    #[test]
    fn every_seat_puts_the_surface_on_the_palm() {
        for half in [
            vec3(meters(0.08), meters(0.01), meters(0.02)),
            vec3(meters(0.02), meters(0.02), meters(0.02)),
            vec3(meters(0.3), meters(0.05), meters(0.1)),
        ] {
            let (min, max) = box_of(half);
            let (seat, _) = seat(min, max, None);
            let (_, hi) = seated_bounds(min, max, seat);
            assert!(
                (hi.y - PALM_ANCHOR.y).abs() < 1e-5,
                "half {half:?} seated with its top at {}",
                hi.y
            );
        }
    }

    /// An authored turn is kept as authored - the profile decides which way the
    /// item faces - and only the drop onto the palm is computed.
    #[test]
    fn an_authored_rotation_is_kept_and_only_the_drop_is_solved() {
        let (min, max) = box_of(vec3(meters(0.1), meters(0.02), meters(0.03)));
        let authored = Quaternion::from_angle_y(Deg(-90.0));

        let (seat, _) = seat(min, max, Some(authored));

        assert!((seat.rotation.s - authored.s).abs() < 1e-6);
        let (_, hi) = seated_bounds(min, max, seat);
        assert!((hi.y - PALM_ANCHOR.y).abs() < 1e-5);
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
            let rotation = across_the_palm(extents);
            let cross = rotation
                .rotate_vector(Vector3::unit_x())
                .cross(rotation.rotate_vector(Vector3::unit_y()));
            assert!(
                (cross - rotation.rotate_vector(Vector3::unit_z())).magnitude() < 1e-5,
                "{extents:?} produced a reflection"
            );
        }
    }
}
