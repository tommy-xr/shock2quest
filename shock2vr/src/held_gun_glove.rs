//! Where the player's glove goes on a VR-wielded gun.
//!
//! The 25AE first-person gun models bake a hand and forearm onto the grip.
//! VR strips that geometry (`dark::importers::VrHeldGunModel`) and draws the
//! tracked glove instead, so the hand the player sees is the hand they are
//! moving. This module answers the one question that leaves: where does the
//! glove's wrist go?
//!
//! The answer is inherited from the art. Each baked hand comes with a
//! geometric frame ([`dark`'s `HandFrame`]), so putting the glove's wrist on
//! the baked hand's wrist with the fingers along its axis reproduces the
//! artist's grip for free - no per-gun placement to author. What that frame
//! cannot supply is **roll** about the finger axis (the frame fixes two of
//! three rotational degrees of freedom), and its wrist is *estimated* rather
//! than authored - `HandFrame` steps back a hand's length from the far end of
//! the point cloud, which lands a few centimetres differently depending on how
//! curled the hand is. [`GloveWristTune`] is the correction for both, per
//! model, tuned by eye from captures and zero by default.
//!
//! The guns that bake no hand at all (`gren_h`, `sfg_h`, `fsn_h`, `al_h`,
//! `viro_h`) have nothing to inherit - but they need nothing: their
//! `vr_config::HAND_MODEL_POSITIONING` offsets already seat the grip in the
//! palm, so the glove belongs at the tracked hand itself.

use std::collections::HashMap;

use cgmath::{Deg, Matrix4, SquareMatrix, Transform, Vector3, vec3};
use dark::{importers::FIRST_PERSON_HANDS_IMPORTER, ss2_bin_obj_loader::HandFrame};
use engine::assets::asset_cache::AssetCache;
use once_cell::sync::Lazy;

use crate::hand_pose_library::{anchor_of, wrist_vector};
use crate::vr_config::{Handedness, VRHandModelPerHandAdjustments};

/// Per-model correction to the inherited wrist placement, in the glove's own
/// hand space: +X toward the thumb, +Y out of the back of the hand, -Z along
/// the fingers, and `roll_degrees` about that finger axis.
///
/// Applied in hand space (innermost), so a left-hand wield mirrors it along
/// with the gun rather than pushing the glove the wrong way.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GloveWristTune {
    pub offset: Vector3<f32>,
    pub roll_degrees: f32,
}

impl Default for GloveWristTune {
    fn default() -> Self {
        GloveWristTune {
            offset: vec3(0.0, 0.0, 0.0),
            roll_degrees: 0.0,
        }
    }
}

impl GloveWristTune {
    fn transform(self) -> Matrix4<f32> {
        Matrix4::from_translation(self.offset) * Matrix4::from_angle_z(Deg(self.roll_degrees))
    }
}

/// Corrections to the inherited placement, keyed by lowercased `PropModelName`
/// like the rest of the per-model VR tables. Anything absent inherits the
/// baked hand's frame unaltered.
///
/// Slice 6 of the VR interaction work moves these to a data file with the grip
/// offsets; until then they live beside the code that reads them.
static GLOVE_WRIST_TUNING: Lazy<HashMap<&str, GloveWristTune>> = Lazy::new(|| {
    HashMap::from([
        // The pistol's baked hand is the most curled of the set, so its
        // estimated wrist sits furthest forward - slide the glove back down
        // the grip and roll the palm onto it.
        (
            "atek_h",
            GloveWristTune {
                offset: vec3(0.0, -0.01, 0.03),
                roll_degrees: -10.0,
            },
        ),
        // Shotgun and assault rifle bake a hand *with* a forearm, where the
        // frame's forward is the forearm axis rather than the finger axis;
        // both need the wrist nudged forward onto the grip.
        (
            "sg_h",
            GloveWristTune {
                offset: vec3(0.0, -0.02, -0.02),
                roll_degrees: 0.0,
            },
        ),
        (
            "ar15_h",
            GloveWristTune {
                offset: vec3(0.0, -0.02, -0.02),
                roll_degrees: 0.0,
            },
        ),
    ])
});

fn glove_wrist_tuning(model_name: &str) -> GloveWristTune {
    GLOVE_WRIST_TUNING
        .get(model_name.to_ascii_lowercase().as_str())
        .copied()
        .unwrap_or_default()
}

/// The transform that takes the glove out of its own hand space and into the
/// **tracked hand's local space** for a wielded gun - the matrix
/// `hand_glove::GloveRenderer::render_held_hand` draws through.
///
/// It reproduces the wield's own composition exactly (`VirtualHand` places the
/// held entity at `hand + hand_rotation * adjustments.offset` with rotation
/// `hand_rotation * adjustments.rotation`, and `Effect::ChangeModel` bakes
/// [`Handedness::gun_mirror`] into the mesh), then walks back from the gun's
/// model space to the baked hand's frame. So the glove lands where the
/// authored hand was drawn, for any grip entry, in either hand.
///
/// The result **includes** the reflection for a left-hand wield - either the
/// gun mirror or, with no baked hand, the glove's own
/// [`Handedness::mirror`] - and its determinant is therefore negative for the
/// left hand. The renderer must not apply a mirror of its own on top.
pub fn glove_seat(
    baked_hand: Option<&HandFrame>,
    adjustments: &VRHandModelPerHandAdjustments,
    handedness: Handedness,
    tune: GloveWristTune,
) -> Matrix4<f32> {
    match baked_hand {
        Some(frame) => {
            // `anchor_of` maps model space into hand space; the glove needs
            // the other direction. It is a rigid motion, so it always inverts.
            let to_model = anchor_of(frame, Deg(0.0))
                .invert()
                .unwrap_or_else(Matrix4::identity);
            Matrix4::from_translation(adjustments.offset)
                * Matrix4::from(adjustments.rotation)
                * handedness.gun_mirror()
                * to_model
                * tune.transform()
        }
        None => handedness.mirror() * tune.transform(),
    }
}

/// [`glove_seat`] for a wielded gun, resolved from its model.
///
/// `None` when the model's hands cannot be read at all (a classic install has
/// none of these meshes), which leaves the wield drawing no glove - exactly
/// what it did before this path existed.
pub fn glove_seat_for_wield(
    asset_cache: &mut AssetCache,
    model_name: &str,
    handedness: Handedness,
) -> Option<Matrix4<f32>> {
    let hands = asset_cache.get_opt(&FIRST_PERSON_HANDS_IMPORTER, &format!("{model_name}.bin"))?;
    let adjustments =
        crate::vr_config::get_vr_hand_model_adjustments_from_model(model_name, handedness);
    // The islands are in unmirrored model space, so the pick must be made
    // against the unmirrored grip - otherwise a left-hand wield could choose a
    // different hand than the right one does.
    let right_grip =
        crate::vr_config::get_vr_hand_model_adjustments_from_model(model_name, Handedness::Right);
    Some(glove_seat(
        trigger_hand(hands.0.iter().map(|hand| &hand.frame), &right_grip),
        &adjustments,
        handedness,
        glove_wrist_tuning(model_name),
    ))
}

/// Which of a model's baked hands is the one on the trigger: the hand whose
/// wrist is nearest where the tracked hand sits in model space.
///
/// Neither material nor island size answers this. `atek_h` draws its firing
/// arm as two islands (a `ND-arm.psd` sleeve and a `ND-arm_atek.psd` hand, 445
/// and 440 polys) plus a third `ND-arm.psd` island that is a spare hand parked
/// 0.6 m behind the grip for the reload animation - so the largest island is
/// the sleeve, and the material is shared by the spare. Distance to the grip
/// separates them cleanly: the two firing-arm islands land within 7 cm of the
/// tracked hand and the spare 92 cm away.
fn trigger_hand<'a>(
    frames: impl IntoIterator<Item = &'a HandFrame>,
    right_grip: &VRHandModelPerHandAdjustments,
) -> Option<&'a HandFrame> {
    let grip = hand_origin_in_model_space(right_grip);
    let distance = |frame: &HandFrame| cgmath::InnerSpace::magnitude2(wrist_vector(frame) - grip);
    frames
        .into_iter()
        .min_by(|a, b| distance(a).total_cmp(&distance(b)))
}

/// Where the tracked hand's origin falls in the gun's model space - the
/// inverse of the wield's own hand-local grip placement.
fn hand_origin_in_model_space(adjustments: &VRHandModelPerHandAdjustments) -> Vector3<f32> {
    let rotation: Matrix4<f32> = adjustments.rotation.into();
    rotation
        .invert()
        .unwrap_or_else(Matrix4::identity)
        .transform_vector(-adjustments.offset)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cgmath::{InnerSpace, Point3, Vector4, assert_relative_eq, point3};

    fn frame(origin: Point3<f32>, forward: Vector3<f32>) -> HandFrame {
        HandFrame {
            name: String::new(),
            origin,
            forward: forward.normalize(),
            length: dark::ss2_bin_obj_loader::HAND_LENGTH_WORLD,
        }
    }

    /// The pistol's authored grip, as `vr_config` has it.
    fn pistol_grip(handedness: Handedness) -> VRHandModelPerHandAdjustments {
        crate::vr_config::get_vr_hand_model_adjustments_from_model("atek_h", handedness)
    }

    fn apply(transform: Matrix4<f32>, point: Vector3<f32>) -> Vector3<f32> {
        let v = transform * Vector4::new(point.x, point.y, point.z, 1.0);
        vec3(v.x, v.y, v.z)
    }

    /// The glove's wrist must land exactly where the baked hand's wrist
    /// renders. Computed here from the wield's own composition rather than
    /// from `glove_seat`'s internals, so the two derivations have to agree.
    #[test]
    fn the_glove_wrist_lands_on_the_baked_hands_wrist() {
        let hand = frame(point3(0.31, -0.21, -0.05), vec3(-0.93, 0.33, 0.13));

        for handedness in [Handedness::Right, Handedness::Left] {
            let grip = pistol_grip(handedness);
            let seat = glove_seat(Some(&hand), &grip, handedness, GloveWristTune::default());

            // Where the wield draws that wrist: mirror it with the mesh, then
            // seat it by the grip entry.
            let wrist = handedness
                .gun_mirror()
                .transform_vector(wrist_vector(&hand));
            let expected = grip.offset + Matrix4::from(grip.rotation).transform_vector(wrist);

            let drawn = apply(seat, vec3(0.0, 0.0, 0.0));
            assert_relative_eq!(drawn.x, expected.x, epsilon = 1e-5);
            assert_relative_eq!(drawn.y, expected.y, epsilon = 1e-5);
            assert_relative_eq!(drawn.z, expected.z, epsilon = 1e-5);
        }
    }

    /// The fingers must run down the baked hand's own axis, so the glove
    /// closes along the grip rather than across it. The glove's fingers point
    /// down hand-space -Z.
    #[test]
    fn the_glove_fingers_follow_the_baked_hands_axis() {
        let hand = frame(point3(0.31, -0.21, -0.05), vec3(-0.93, 0.33, 0.13));

        for handedness in [Handedness::Right, Handedness::Left] {
            let grip = pistol_grip(handedness);
            let seat = glove_seat(Some(&hand), &grip, handedness, GloveWristTune::default());

            let fingers = seat * Vector4::new(0.0, 0.0, -1.0, 0.0);
            let expected = Matrix4::from(grip.rotation)
                .transform_vector(handedness.gun_mirror().transform_vector(hand.forward));
            assert_relative_eq!(fingers.x, expected.x, epsilon = 1e-5);
            assert_relative_eq!(fingers.y, expected.y, epsilon = 1e-5);
            assert_relative_eq!(fingers.z, expected.z, epsilon = 1e-5);
        }
    }

    /// The seat carries the hand's own reflection: the glove asset is a right
    /// hand, so a left-hand wield must reach it through a negative
    /// determinant or the player sees two right hands. Pinned for both the
    /// inherited placement and the no-baked-hand fallback.
    #[test]
    fn the_left_hand_seat_reflects_the_right_handed_glove() {
        let hand = frame(point3(0.31, -0.21, -0.05), vec3(-0.93, 0.33, 0.13));

        for baked in [Some(&hand), None] {
            let right = glove_seat(
                baked,
                &pistol_grip(Handedness::Right),
                Handedness::Right,
                GloveWristTune::default(),
            );
            let left = glove_seat(
                baked,
                &pistol_grip(Handedness::Left),
                Handedness::Left,
                GloveWristTune::default(),
            );
            assert!(right.determinant() > 0.0, "the right hand is not mirrored");
            assert!(left.determinant() < 0.0, "the left hand must mirror");
        }
    }

    /// A gun with no baked hand puts the glove on the tracked hand itself -
    /// the identity placement the grip offsets were fitted against.
    #[test]
    fn a_gun_with_no_baked_hand_puts_the_glove_on_the_tracked_hand() {
        let seat = glove_seat(
            None,
            &pistol_grip(Handedness::Right),
            Handedness::Right,
            GloveWristTune::default(),
        );

        assert_relative_eq!(seat, Matrix4::identity());
    }

    /// The tune is applied in hand space, so a left-hand wield mirrors it with
    /// the gun instead of pushing the glove off the far side of the grip.
    #[test]
    fn the_tune_mirrors_with_the_wield() {
        let hand = frame(point3(0.31, -0.21, -0.05), vec3(-0.93, 0.33, 0.13));
        let tune = GloveWristTune {
            offset: vec3(0.05, 0.0, 0.0),
            roll_degrees: 0.0,
        };

        let mut nudged = Vec::new();
        for handedness in [Handedness::Right, Handedness::Left] {
            let grip = pistol_grip(handedness);
            let plain = glove_seat(Some(&hand), &grip, handedness, GloveWristTune::default());
            let tuned = glove_seat(Some(&hand), &grip, handedness, tune);
            nudged.push(apply(tuned, vec3(0.0, 0.0, 0.0)) - apply(plain, vec3(0.0, 0.0, 0.0)));
        }

        // Same magnitude in both hands, and mirrored across the gun rather
        // than identical - the thumb side swaps with the geometry.
        assert_relative_eq!(nudged[0].magnitude(), nudged[1].magnitude(), epsilon = 1e-5);
        assert!(
            (nudged[0] - nudged[1]).magnitude() > 1e-3,
            "the tune did not mirror: {nudged:?}"
        );
    }

    /// The trigger hand is the one at the grip, not the largest island and not
    /// the spare hand the reload animation parks out of frame. Distances are
    /// `atek_h`'s, from `cargo run -p shock2vr --example gun_hand_islands`.
    #[test]
    fn the_trigger_hand_is_the_one_nearest_the_grip() {
        let grip = pistol_grip(Handedness::Right);
        let firing = frame(point3(0.313, -0.211, -0.052), vec3(-0.934, 0.330, 0.133));
        let spare = frame(point3(0.961, 0.087, 0.181), vec3(-0.848, -0.001, -0.531));

        // Ordered largest-island first, as the importer serves them - the
        // spare hand here stands in for the sleeve/spare that outranks the
        // firing hand by size.
        let hands = [spare.clone(), firing.clone()];

        let picked = trigger_hand(hands.iter(), &grip).expect("a hand was picked");
        assert_relative_eq!(picked.origin.x, firing.origin.x);
    }
}
