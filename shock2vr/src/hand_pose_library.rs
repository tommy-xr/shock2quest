//! The free-hand pose library: 25AE authored hands, snapped between.
//!
//! The remaster's first-person weapon models each carry a hand posed onto that
//! weapon. Those hands have no finger joints (see
//! `~/notes/projects/shock2quest/vr-hand-rigging.md`), so we cannot blend
//! between poses - but the poses themselves are good, and there are enough of
//! them to cover the states a free hand needs. We pick one per state and snap.
//!
//! Held weapons do not come through here: for those the whole `_h` model is
//! drawn, hand included, which is already the artist's grip.
//!
//! # Aligning a pose
//!
//! Each hand is anchored by its geometric frame ([`dark`'s `HandFrame`]): the
//! wrist goes to the hand origin and the fingers point along the hand's forward
//! axis. That fixes position and two of three rotational degrees of freedom
//! from the geometry itself, so the only thing left to author per pose is the
//! **roll** about the finger axis - one number, tuned visually in
//! `debug_hand_poses`.
//!
//! # Known limitation: the wrist is estimated, not authored
//!
//! `HandFrame` derives the wrist by stepping back a hand's length from the
//! far end of the point cloud. That endpoint is an extended fingertip on one
//! hand and a knuckle on a curled one, so the estimate lands a few centimetres
//! differently per pose - visibly, the axis marker sits mid-palm on `ar15_h`
//! rather than at the wrist. It also means the authored `forward` is the
//! *forearm* axis on the models that include a forearm, not the finger axis.
//!
//! Consequences: the poses do not present the same amount of sleeve, and a
//! common-cuff trim built on this landmark cut into the wrist on the worst
//! case. A per-pose wrist offset alongside `roll`, tuned in the harness, is
//! the intended fix.

use cgmath::{Deg, InnerSpace, Matrix4, Rad, Vector3, vec3};
use dark::{importers::FIRST_PERSON_HANDS_IMPORTER, ss2_bin_obj_loader::HAND_LENGTH_WORLD};
use engine::{assets::asset_cache::AssetCache, scene::SceneObject};

/// A free-hand state we can show. Ordered open to closed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HandPose {
    /// Idle/empty hand, fingers extended.
    Rest,
    /// Reaching or about to grab - fingers curled partway.
    SemiClosed,
    /// Closed around something, or a trigger pull.
    Grip,
}

impl HandPose {
    pub const ALL: [HandPose; 3] = [HandPose::Rest, HandPose::SemiClosed, HandPose::Grip];

    /// Which authored hand supplies this pose, and how it is rolled into place.
    ///
    /// The sources are the 25AE first-person weapon models; the index selects
    /// among the hands that model draws (largest island first), so the pistol's
    /// support hand is `atek_h.bin` index 2.
    fn source(self) -> PoseSource {
        match self {
            // The only authored hand with extended fingers.
            HandPose::Rest => PoseSource {
                model: "ar15_h.bin",
                index: 0,
                roll: Deg(0.0),
            },
            // The pistol's trigger hand: fingers curled partway, as if closing.
            HandPose::SemiClosed => PoseSource {
                model: "atek_h.bin",
                index: 0,
                roll: Deg(0.0),
            },
            // The pistol's support hand, the most closed of the three: fingers
            // curled under into a cup.
            HandPose::Grip => PoseSource {
                model: "atek_h.bin",
                index: 2,
                roll: Deg(0.0),
            },
        }
    }
}

struct PoseSource {
    model: &'static str,
    index: usize,
    roll: Deg<f32>,
}

/// One pose's geometry, already anchored so the wrist sits at the origin with
/// the fingers pointing along -Z (the hand frame's forward, matching
/// `VirtualHand`).
pub struct LoadedPose {
    pub pose: HandPose,
    objects: Vec<SceneObject>,
}

impl LoadedPose {
    /// The pose's scene objects at a hand's world transform.
    pub fn at(&self, world: Matrix4<f32>) -> Vec<SceneObject> {
        self.objects
            .iter()
            .map(|object| {
                let mut clone = object.clone();
                clone.set_transform(world * object.get_transform());
                clone
            })
            .collect()
    }
}

/// Every pose we could load. Missing poses are skipped rather than fatal - a
/// non-25AE install has none of these models.
pub fn load(asset_cache: &mut AssetCache) -> Vec<LoadedPose> {
    HandPose::ALL
        .into_iter()
        .filter_map(|pose| load_pose(asset_cache, pose))
        .collect()
}

fn load_pose(asset_cache: &mut AssetCache, pose: HandPose) -> Option<LoadedPose> {
    let source = pose.source();
    let hands = asset_cache.get_opt(&FIRST_PERSON_HANDS_IMPORTER, source.model)?;
    let hand = hands.0.get(source.index)?;

    // Take the hand out of model space and into hand space: wrist to the
    // origin, fingers down the frame's forward axis, then the authored roll.
    let anchor = anchor_of(&hand.frame, source.roll);

    let objects = hand
        .model
        .clone_scene_objects()
        .into_iter()
        .map(|mut object| {
            object.set_transform(anchor * object.get_transform());
            object
        })
        .collect();

    Some(LoadedPose { pose, objects })
}

/// Maps a hand's authored frame onto the hand origin.
///
/// `VirtualHand`'s forward is -Z (the aim/raycast direction), so the fingers are
/// pointed down -Z here and the whole hand is rolled about that axis by the
/// pose's authored roll.
fn anchor_of(frame: &dark::ss2_bin_obj_loader::HandFrame, roll: Deg<f32>) -> Matrix4<f32> {
    let forward = frame.forward;
    let reference = if forward.y.abs() > 0.9 {
        vec3(1.0, 0.0, 0.0)
    } else {
        vec3(0.0, 1.0, 0.0)
    };
    let right = reference.cross(forward).normalize();
    let up = forward.cross(right);

    // Rows are the target basis, so this maps the frame into hand space.
    //
    // `right` is negated to keep the basis right-handed. Mapping `forward` to
    // -Z while sending `right` to +X and `up` to +Y is geometrically impossible
    // without a flip: with `up = forward x right`, the rows (right, up,
    // -forward) have determinant -1. That reflection renders the authored right
    // hand as a left hand and reverses triangle winding, so backface culling
    // shows the inside of the mesh - see `anchor_is_a_rotation_not_a_reflection`.
    let to_hand = Matrix4::from_cols(
        vec3(-right.x, up.x, -forward.x).extend(0.0),
        vec3(-right.y, up.y, -forward.y).extend(0.0),
        vec3(-right.z, up.z, -forward.z).extend(0.0),
        cgmath::Vector4::new(0.0, 0.0, 0.0, 1.0),
    );

    Matrix4::from_angle_z(Rad::from(roll))
        * to_hand
        * Matrix4::from_translation(-wrist_vector(frame))
}

/// The wrist, set back from the fingertips by a hand's length.
///
/// `HandFrame::origin` is the far end of the geometry, which on the models whose
/// hand includes a forearm is the *elbow* - anchoring there would hang the hand
/// off the controller by an arm's length.
fn wrist_vector(frame: &dark::ss2_bin_obj_loader::HandFrame) -> Vector3<f32> {
    let fingertip = frame.origin + frame.forward * frame.length;
    let wrist = fingertip - frame.forward * HAND_LENGTH_WORLD;
    vec3(wrist.x, wrist.y, wrist.z)
}


#[cfg(test)]
mod tests {
    use super::*;
    use cgmath::{SquareMatrix, Vector4, assert_relative_eq};
    use dark::ss2_bin_obj_loader::HandFrame;
    use cgmath::point3;

    fn frame(forward: Vector3<f32>) -> HandFrame {
        HandFrame {
            name: String::new(),
            origin: point3(0.0, 0.0, 0.0),
            forward: forward.normalize(),
            length: 1.0,
        }
    }

    /// The anchor must be a *rotation*. A negative determinant is a reflection:
    /// it renders the right hand as a left hand and reverses triangle winding,
    /// so backface culling shows the inside of the mesh.
    #[test]
    fn anchor_is_a_rotation_not_a_reflection() {
        for forward in [
            vec3(0.0, 0.0, 1.0),
            vec3(1.0, 0.0, 0.0),
            vec3(0.3, -0.5, 0.8),
            vec3(0.0, 1.0, 0.0),
        ] {
            let anchor = anchor_of(&frame(forward), Deg(0.0));
            let determinant = anchor.determinant();
            assert!(
                determinant > 0.0,
                "anchor for {forward:?} has determinant {determinant} - it mirrors the hand"
            );
        }
    }

    /// The fingers must end up pointing down -Z, which is `VirtualHand`'s forward.
    #[test]
    fn anchor_points_the_fingers_down_negative_z() {
        for forward in [vec3(0.0, 0.0, 1.0), vec3(1.0, 0.0, 0.0), vec3(0.3, -0.5, 0.8)] {
            let f = frame(forward);
            let anchor = anchor_of(&f, Deg(0.0));
            let pointed = anchor * Vector4::new(f.forward.x, f.forward.y, f.forward.z, 0.0);
            assert_relative_eq!(pointed.x, 0.0, epsilon = 1e-4);
            assert_relative_eq!(pointed.y, 0.0, epsilon = 1e-4);
            assert_relative_eq!(pointed.z, -1.0, epsilon = 1e-4);
        }
    }
}
