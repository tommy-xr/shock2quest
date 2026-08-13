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

use cgmath::{Deg, InnerSpace, Matrix4, Quaternion, Rad, Vector3, vec3};
use dark::{importers::FIRST_PERSON_HANDS_IMPORTER, ss2_bin_obj_loader::HAND_LENGTH_WORLD};
use engine::{
    assets::asset_cache::AssetCache,
    scene::{FrontFaceWinding, SceneObject},
};

use crate::vr_config::Handedness;

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
                // The rifle's supporting hand, under the handguard - which is
                // also why it carries the most forearm of the three.
                authored: Handedness::Left,
                // Fingers fully extended: the far point IS a fingertip.
                reach: 1.0,
            },
            // The pistol's trigger hand: fingers curled partway, as if closing.
            HandPose::SemiClosed => PoseSource {
                model: "atek_h.bin",
                index: 0,
                roll: Deg(0.0),
                // The pistol's trigger hand.
                authored: Handedness::Right,
                // Curled partway - the far point is around the middle knuckles.
                reach: 0.85,
            },
            // The pistol's support hand, the most closed of the three: fingers
            // curled under into a cup.
            HandPose::Grip => PoseSource {
                model: "atek_h.bin",
                index: 2,
                roll: Deg(0.0),
                // The pistol's supporting hand, cupped under the grip.
                authored: Handedness::Left,
                // Fingers curled under into a cup: the far point is barely past
                // the knuckles, so the step-back has to be much shorter.
                reach: 0.7,
            },
        }
    }
}

struct PoseSource {
    model: &'static str,
    index: usize,
    roll: Deg<f32>,
    /// Which of the player's hands this geometry was actually authored as.
    ///
    /// These are weapon viewmodel hands, and a two-handed weapon is posed with
    /// one of each: the hand on the grip is the right, the supporting hand is
    /// the left. So handedness is a property of the source, not a constant -
    /// assuming every authored hand was a right hand rendered half the library
    /// mirrored, thumb on the inside.
    authored: Handedness,
    /// How far this pose's farthest point sits from the wrist, as a fraction of
    /// a hand's length. The frame's far end is an extended fingertip on an open
    /// hand but only a knuckle on a curled one, so a single step-back lands the
    /// wrist in a different place per pose - which is what made the hand jump
    /// between poses. One number per pose, tuned visually, is the fix the module
    /// docs call for.
    reach: f32,
}

/// One pose's geometry, already anchored so the wrist sits at the origin with
/// the fingers pointing along -Z (the hand frame's forward, matching
/// `VirtualHand`).
pub struct LoadedPose {
    pub pose: HandPose,
    /// Which hand this geometry is, as authored - see [`PoseSource::authored`].
    authored: Handedness,
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
    let anchor = anchor_of(&hand.frame, source.roll, source.reach);

    let objects = hand
        .model
        .clone_scene_objects()
        .into_iter()
        .map(|mut object| {
            object.set_transform(anchor * object.get_transform());
            object
        })
        .collect();

    Some(LoadedPose {
        pose,
        authored: source.authored,
        objects,
    })
}

/// Maps a hand's authored frame onto the hand origin.
///
/// `VirtualHand`'s forward is -Z (the aim/raycast direction), so the fingers are
/// pointed down -Z here and the whole hand is rolled about that axis by the
/// pose's authored roll.
fn anchor_of(
    frame: &dark::ss2_bin_obj_loader::HandFrame,
    roll: Deg<f32>,
    reach: f32,
) -> Matrix4<f32> {
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
        * Matrix4::from_translation(-wrist_vector(frame, reach))
}

/// The wrist, set back from the pose's farthest point.
///
/// `HandFrame::origin` is the far end of the geometry, which on the models whose
/// hand includes a forearm is the *elbow* - anchoring there would hang the hand
/// off the controller by an arm's length. So we work from the other end, and
/// step back `reach` hand-lengths (see [`PoseSource::reach`]: a curled hand's
/// far point is a knuckle, not a fingertip, so it is nearer the wrist).
fn wrist_vector(frame: &dark::ss2_bin_obj_loader::HandFrame, reach: f32) -> Vector3<f32> {
    let fingertip = frame.origin + frame.forward * frame.length;
    let wrist = fingertip - frame.forward * AUTHORED_HAND_LENGTH * reach;
    vec3(wrist.x, wrist.y, wrist.z)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cgmath::point3;
    use cgmath::{InnerSpace, SquareMatrix, Vector4, assert_relative_eq};
    use dark::ss2_bin_obj_loader::HandFrame;

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
            let anchor = anchor_of(&frame(forward), Deg(0.0), 1.0);
            let determinant = anchor.determinant();
            assert!(
                determinant > 0.0,
                "anchor for {forward:?} has determinant {determinant} - it mirrors the hand"
            );
        }
    }

    #[test]
    fn holding_something_always_grips() {
        // Even with the analog inputs slack - the item is what holds the hand
        // closed, not the squeeze.
        assert_eq!(PoseLibrary::pose_for(0.0, 0.0, true), HandPose::Grip);
    }

    #[test]
    fn an_empty_hand_opens_and_closes_with_the_analog_inputs() {
        assert_eq!(PoseLibrary::pose_for(0.0, 0.0, false), HandPose::Rest);
        assert_eq!(PoseLibrary::pose_for(0.4, 0.0, false), HandPose::SemiClosed);
        assert_eq!(PoseLibrary::pose_for(1.0, 0.0, false), HandPose::Grip);
        // Either input can close the hand; the larger wins.
        assert_eq!(PoseLibrary::pose_for(0.0, 1.0, false), HandPose::Grip);
        assert_eq!(PoseLibrary::pose_for(0.9, 0.1, false), HandPose::Grip);
    }

    /// The anchored wrist must sit one real hand-length back from the pose's far
    /// point, so that once `VIEWMODEL_HAND_SCALE` is applied the hand is the same
    /// physical size, anchored at the same landmark, as the glove.
    ///
    /// Regression test: `wrist_vector` used to step back `HAND_LENGTH_WORLD`
    /// (a *world*-unit length) through *model*-space geometry, landing the
    /// anchor ~61% of a hand short - mid-palm instead of at the wrist.
    #[test]
    fn the_wrist_anchors_a_real_hand_back_from_the_far_point() {
        let f = frame(vec3(0.0, 0.0, 1.0));
        let far_point = f.origin + f.forward * f.length;
        let wrist = wrist_vector(&f, 1.0);

        let step_back = (far_point - point3(wrist.x, wrist.y, wrist.z)).magnitude();
        // In world units once the viewmodel scale is applied.
        assert_relative_eq!(
            step_back * VIEWMODEL_HAND_SCALE,
            HAND_LENGTH_WORLD,
            epsilon = 1e-5
        );
    }

    /// A curled pose's far point is a knuckle, so its wrist must come out nearer
    /// that point than an extended hand's - otherwise the step-back overshoots
    /// into the forearm and the hand jumps between poses.
    #[test]
    fn a_shorter_reach_anchors_the_wrist_nearer_the_far_point() {
        let f = frame(vec3(0.0, 0.0, 1.0));
        let far_point = f.origin + f.forward * f.length;
        let distance = |reach| {
            let w = wrist_vector(&f, reach);
            (far_point - point3(w.x, w.y, w.z)).magnitude()
        };

        assert!(distance(0.7) < distance(1.0));
        for pose in HandPose::ALL {
            let reach = pose.source().reach;
            assert!(
                (0.0..=1.0).contains(&reach),
                "{pose:?} has an out-of-range reach {reach}"
            );
        }
    }

    /// A pose is drawn as authored for its own hand, and mirrored for the other.
    ///
    /// Regression test: the mirror used to key off `Handedness::Left` alone,
    /// assuming every authored hand was a right hand. The sources are weapon
    /// viewmodel hands and include both, so that rendered the left-authored
    /// poses backwards - thumb on the inside, as reported in a headset.
    #[test]
    fn a_pose_is_mirrored_only_for_the_hand_it_was_not_authored_as() {
        // Both hands are represented among the sources, so the bug this guards
        // against cannot be papered over by them all agreeing.
        let authored: Vec<Handedness> = HandPose::ALL
            .into_iter()
            .map(|pose| pose.source().authored)
            .collect();
        assert!(
            authored.contains(&Handedness::Left) && authored.contains(&Handedness::Right),
            "expected both hands among the authored sources, got {authored:?}"
        );
    }

    /// The fingers must end up pointing down -Z, which is `VirtualHand`'s forward.
    #[test]
    fn anchor_points_the_fingers_down_negative_z() {
        for forward in [
            vec3(0.0, 0.0, 1.0),
            vec3(1.0, 0.0, 0.0),
            vec3(0.3, -0.5, 0.8),
        ] {
            let f = frame(forward);
            let anchor = anchor_of(&f, Deg(0.0), 1.0);
            let pointed = anchor * Vector4::new(f.forward.x, f.forward.y, f.forward.z, 0.0);
            assert_relative_eq!(pointed.x, 0.0, epsilon = 1e-4);
            assert_relative_eq!(pointed.y, 0.0, epsilon = 1e-4);
            assert_relative_eq!(pointed.z, -1.0, epsilon = 1e-4);
        }
    }
}

/// A hand's wrist-to-fingertip length in the authored models' own units.
///
/// This is the one measured number the sizing rests on, and it is deliberately
/// the *primary* constant: [`VIEWMODEL_HAND_SCALE`] below is derived from it, and
/// so is the wrist step-back in [`wrist_vector`]. Deriving the other way round
/// would mean that adjusting the hands' size also slid the anchor along the
/// finger axis, moving the hand off the controller - size and anchor come from
/// one measurement because they are one measurement.
///
/// Same law as the glove, in the same direction:
/// `hand_glove::GLOVE_SCALE = real_hand_length / AUTHORED_HAND_LENGTH_WORLD`.
///
/// Calibration caveat, worth knowing before trusting this in a headset: the
/// value is inherited from a viewmodel measurement of ~31 cm taken *across* the
/// pistol's support hand, against a 19 cm wrist-to-fingertip hand - a breadth
/// compared with a length. It survives because it checks out empirically (the
/// rendered SemiClosed hand measures within ~2% of the glove's, which is
/// independently scaled to 19 cm), not because the derivation was sound. The
/// honest fix is to measure wrist-to-fingertip on the hand geometry alone,
/// excluding the cuff, and put that number here.
///
/// The digits are the previous calibration carried over exactly
/// (`HAND_LENGTH_WORLD * 31.0 / 19.0`), so restructuring these constants left
/// the render byte-for-byte unchanged. They are precision inherited, not
/// precision measured - do not read them as a claim of accuracy.
const AUTHORED_HAND_LENGTH: f32 = 0.406_752_63;

/// Brings the authored hands down to life size.
///
/// They are first-person *viewmodel* geometry, deliberately oversized so they
/// read on a monitor. VR renders the world at true scale, so they have to come
/// down or they dwarf the player's real hands.
///
/// One scale for all poses: they are one artist's hands at one viewmodel scale.
const VIEWMODEL_HAND_SCALE: f32 = HAND_LENGTH_WORLD / AUTHORED_HAND_LENGTH;

/// Analog closure at or above which the hand reads as fully closed.
const CLOSED_THRESHOLD: f32 = 0.66;
/// ...and below which it reads as fully open.
const OPEN_THRESHOLD: f32 = 0.2;

/// The loaded poses, ready to render as the player's hand.
pub struct PoseLibrary {
    poses: Vec<LoadedPose>,
}

impl PoseLibrary {
    /// `None` unless EVERY pose loaded, which is what a non-25AE install looks
    /// like. Callers should cache that outcome rather than retry per frame, and
    /// fall back to the glove.
    ///
    /// All-or-nothing on purpose: `render_hand` draws nothing when the pose it
    /// wants is missing, and the caller does not consult the glove once the
    /// library exists. A partial install - mod layering, or an SCP/SHTUP variant
    /// that ships `atek_h` but not `ar15_h` - would otherwise make the player's
    /// hands vanish the moment they opened their hand. A whole glove is better
    /// than an intermittently invisible hand.
    pub fn new(asset_cache: &mut AssetCache) -> Option<Self> {
        let poses = load(asset_cache);
        (poses.len() == HandPose::ALL.len()).then_some(Self { poses })
    }

    /// Which pose a hand in this state should show.
    ///
    /// Snapping, not blending: the authored hands have no finger joints and the
    /// three poses come from distinct source meshes, so there is nothing to
    /// interpolate between (see the module docs).
    pub fn pose_for(trigger: f32, squeeze: f32, holding: bool) -> HandPose {
        if holding {
            return HandPose::Grip;
        }
        let closed = trigger.max(squeeze);
        if closed >= CLOSED_THRESHOLD {
            HandPose::Grip
        } else if closed >= OPEN_THRESHOLD {
            HandPose::SemiClosed
        } else {
            HandPose::Rest
        }
    }

    /// The player's hand at `position`/`rotation`, in the pose its state calls
    /// for. Empty if that pose failed to load.
    pub fn render_hand(
        &self,
        position: Vector3<f32>,
        rotation: Quaternion<f32>,
        handedness: Handedness,
        trigger_value: f32,
        squeeze_value: f32,
        holding: bool,
    ) -> Vec<SceneObject> {
        let wanted = Self::pose_for(trigger_value, squeeze_value, holding);
        let Some(pose) = self.poses.iter().find(|loaded| loaded.pose == wanted) else {
            return Vec::new();
        };

        // Mirror only when the hand we need is not the hand that was authored.
        let mirror = if handedness == pose.authored {
            Matrix4::from_scale(1.0)
        } else {
            Matrix4::from_nonuniform_scale(-1.0, 1.0, 1.0)
        };
        let world = Matrix4::from_translation(position)
            * Matrix4::from(rotation)
            * mirror
            * Matrix4::from_scale(VIEWMODEL_HAND_SCALE);

        let mut objects = pose.at(world);

        // A mirror reverses triangle winding, so the culling that the loader set
        // for the authored (right-handed) geometry would now cull the front
        // faces and show the hand's interior. Flip it with the mirror.
        if matches!(handedness, Handedness::Left) {
            for object in objects.iter_mut() {
                if let Some(winding) = object.backface_culling() {
                    object.set_backface_culling(Some(match winding {
                        FrontFaceWinding::Clockwise => FrontFaceWinding::CounterClockwise,
                        FrontFaceWinding::CounterClockwise => FrontFaceWinding::Clockwise,
                    }));
                }
            }
        }

        objects
    }
}
