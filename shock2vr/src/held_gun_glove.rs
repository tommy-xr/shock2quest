//! Where the player's glove goes on a VR-wielded gun.
//!
//! The 25AE first-person gun models bake a hand and forearm onto the weapon.
//! VR strips that geometry (`dark::importers::VrHeldGunModel`) and draws the
//! tracked glove instead, so the hand the player sees is the hand they are
//! moving. This module answers the one question that leaves: where does the
//! glove's wrist go?
//!
//! **On the tracked hand**, corrected per model. The obvious alternative was to
//! inherit the placement from the art - each baked hand carries a geometric
//! frame ([`dark`'s `HandFrame`]), so seating the glove's wrist on the baked
//! wrist would need no per-gun authoring at all. Measured against the shipped
//! set (`cargo run -p shock2vr --example gun_hand_islands`, plus captures of
//! the wield in `debug_weapons`), that frame is the wrong signal three ways:
//!
//! - **The baked hand is often not the trigger hand.** `ar15_h` draws exactly
//!   one, a rest hand laid over the receiver; `sg_h`'s rides the pump. Only
//!   `atek_h` bakes a hand on the grip - and it bakes three islands (a sleeve,
//!   the firing hand, and a spare parked 0.6 m behind the gun for the reload
//!   animation).
//! - **It is not life size.** The set is authored for a fixed flat camera,
//!   where an exaggerated view model reads better; `ar15_h`'s arm island spans
//!   1.074 world units elbow to fingertip (0.82 m against a real arm's ~0.45).
//! - **Its wrist is estimated, not authored.** `HandFrame` steps back a hand's
//!   length from the far end of the point cloud, so it lands mid-palm on an
//!   open hand and further out on a curled one.
//!
//! Whereas `vr_config::HAND_MODEL_POSITIONING` already carries a hand-fitted
//! grip per model - offsets tuned in PR #1023 to seat each weapon in the palm
//! *with the baked hand drawn*, which is exactly the alignment the glove wants.
//! So the tracked hand is the anchor, and [`GloveWristTune`] is where a gun
//! whose grip entry needs a nudge for the glove specifically records it.

use std::collections::HashMap;

use cgmath::{Deg, Matrix4, Vector3, vec3};
use once_cell::sync::Lazy;

use crate::vr_config::Handedness;

/// Per-model correction to the glove's placement on a wielded gun, in the
/// glove's own hand space: +X toward the thumb, +Y out of the back of the
/// hand, -Z along the fingers, and `roll_degrees` about that finger axis.
///
/// Applied in hand space, so a left-hand wield mirrors it along with the gun
/// rather than pushing the glove off the far side of the grip.
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

/// How much larger than life a first-person view model is drawn, and therefore
/// how much larger than life the glove holding one has to be.
///
/// The set is authored for a fixed flat camera, where an exaggerated weapon
/// reads better, and VR draws it at true world scale: `atek_h`'s weapon
/// geometry spans 0.66 world units - 0.51 m, against a real pistol's 0.20 -
/// and its baked hand is exaggerated to match, which is why the wield read
/// coherently before this change. A life-size glove on the same gun reads as a
/// doll's hand, so the glove joins the weapon at the weapon's own scale.
///
/// The number is `dark::SCALE_FACTOR`, which is what the measured ratio comes
/// out at across the set. It is a *presentation* correction for the glove
/// only: drawing the view models at life size is the real fix and a change of
/// its own, because it moves every grip offset, muzzle vhot and magazine
/// anchor with it.
const VIEW_MODEL_GLOVE_SCALE: f32 = dark::SCALE_FACTOR;

/// Corrections to the grip entry's placement, keyed by lowercased
/// `PropModelName` like the rest of the per-model VR tables. A gun with no
/// entry puts the glove on the tracked hand exactly.
///
/// Tuned by eye from `debug_weapons` captures. Slice 6 of the VR interaction
/// work moves these to a data file with the grip offsets themselves; until
/// then they live beside the code that reads them.
static GLOVE_WRIST_TUNING: Lazy<HashMap<&str, GloveWristTune>> = Lazy::new(HashMap::new);

fn glove_wrist_tuning(model_name: &str) -> GloveWristTune {
    GLOVE_WRIST_TUNING
        .get(model_name.to_ascii_lowercase().as_str())
        .copied()
        .unwrap_or_default()
}

/// The transform that takes the glove out of its own hand space and into the
/// **tracked hand's local space** for a wielded gun - the matrix
/// `hand_glove::GloveRenderer::render_held_gun_hand` draws through.
///
/// The result **includes** the glove's own reflection for a left-hand wield
/// ([`Handedness::mirror`]), and its determinant is therefore negative for
/// that hand. The renderer must not apply a mirror of its own on top.
pub fn glove_seat(handedness: Handedness, tune: GloveWristTune) -> Matrix4<f32> {
    // The scale is innermost, so the glove grows about its own wrist: it
    // cannot move where the hand is seated, and the tune stays in world units
    // along the hand's axes.
    handedness.mirror() * tune.transform() * Matrix4::from_scale(VIEW_MODEL_GLOVE_SCALE)
}

/// [`glove_seat`] for the gun `model_name`, in `handedness`.
pub fn glove_seat_for_wield(model_name: &str, handedness: Handedness) -> Matrix4<f32> {
    glove_seat(handedness, glove_wrist_tuning(model_name))
}

#[cfg(test)]
mod tests {
    use super::*;
    use cgmath::{InnerSpace, SquareMatrix, Transform, assert_relative_eq};

    /// An untuned gun puts the glove's wrist on the tracked hand exactly - the
    /// pose the grip offsets were fitted against - and leaves its orientation
    /// alone, carrying only the view-model size correction.
    #[test]
    fn an_untuned_gun_puts_the_glove_on_the_tracked_hand() {
        let seat = glove_seat(Handedness::Right, GloveWristTune::default());

        assert_relative_eq!(
            seat.transform_point(cgmath::point3(0.0, 0.0, 0.0)),
            cgmath::point3(0.0, 0.0, 0.0)
        );
        assert_relative_eq!(
            seat.transform_vector(vec3(0.0, 0.0, -1.0)).normalize(),
            vec3(0.0, 0.0, -1.0)
        );
    }

    /// The seat carries the hand's own reflection: the glove asset is a right
    /// hand, so a left-hand wield must reach it through a negative
    /// determinant or the player sees two right hands.
    #[test]
    fn the_left_hand_seat_reflects_the_right_handed_glove() {
        for tune in [
            GloveWristTune::default(),
            GloveWristTune {
                offset: vec3(0.02, -0.01, 0.03),
                roll_degrees: 12.0,
            },
        ] {
            assert!(glove_seat(Handedness::Right, tune).determinant() > 0.0);
            assert!(glove_seat(Handedness::Left, tune).determinant() < 0.0);
        }
    }

    /// A tune is a nudge in hand space, so it mirrors with the wield: the same
    /// distance in both hands, on opposite thumb sides. Without the mirror a
    /// left-hand tune would push the glove off the far side of the grip.
    #[test]
    fn a_tune_mirrors_with_the_wield() {
        let tune = GloveWristTune {
            offset: vec3(0.05, 0.01, 0.0),
            roll_degrees: 0.0,
        };

        let wrist = |handedness| {
            glove_seat(handedness, tune).transform_point(cgmath::point3(0.0, 0.0, 0.0))
        };

        assert_relative_eq!(wrist(Handedness::Right).x, 0.05);
        assert_relative_eq!(wrist(Handedness::Left).x, -0.05);
        // The nudge is in world units, not scaled by the size correction.
        assert_relative_eq!(wrist(Handedness::Right).y, 0.01);
        assert_relative_eq!(wrist(Handedness::Right).y, wrist(Handedness::Left).y);
    }

    /// The roll turns about the finger axis, which is hand-space Z, so it
    /// rotates the palm around the grip instead of swinging the hand off it.
    #[test]
    fn the_roll_turns_about_the_finger_axis() {
        let seat = glove_seat(
            Handedness::Right,
            GloveWristTune {
                offset: vec3(0.0, 0.0, 0.0),
                roll_degrees: 90.0,
            },
        );

        let fingers = seat.transform_vector(vec3(0.0, 0.0, -1.0)).normalize();
        assert_relative_eq!(fingers.z, -1.0, epsilon = 1e-6);
        let thumb = seat.transform_vector(vec3(1.0, 0.0, 0.0)).normalize();
        assert_relative_eq!(thumb.y, 1.0, epsilon = 1e-6);
    }
}
