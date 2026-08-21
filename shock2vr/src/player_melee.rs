//! Shared timing for the shipped player-melee swing.
//!
//! Flat presentation plays this exact clip and resolves damage from its
//! `MF_TRIGGER1` event. VR leaves the motion to the player's hand, but uses the
//! same authored contact duration and full-swing cadence for damage gating.

/// Shipped `+plyrmelee:2 +plyrmeleeswing` motion used by player weapons.
pub(crate) const SWING_CLIP: &str = "leftswing";

const SWING_FRAMES: f32 = 31.0;
const SWING_FRAMES_PER_SECOND: f32 = 30.0;
const CONTACT_START_FRAME: f32 = 20.0;

pub(crate) const CONTACT_WINDOW_SECONDS: f32 =
    (SWING_FRAMES - CONTACT_START_FRAME) / SWING_FRAMES_PER_SECOND;
pub(crate) const SWING_INTERVAL_SECONDS: f32 = SWING_FRAMES / SWING_FRAMES_PER_SECOND;
