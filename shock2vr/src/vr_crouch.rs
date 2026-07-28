//! Turns floor-relative VR head height into the shared crouch request.

const CROUCH_ENTER_HEIGHT_RATIO: f32 = 0.70;
const CROUCH_EXIT_HEIGHT_RATIO: f32 = 0.85;

// Reject impossible floor-relative heights before they can permanently poison
// the session's max-height calibration. Zero is retained because putting a
// tracked headset on the floor is still a meaningful crouch observation.
const MAX_PLAUSIBLE_EYE_HEIGHT_METERS: f32 = 3.0;

/// Stateful physical-crouch detector for floor-relative tracked head heights.
///
/// The detector calibrates standing eye height from the tallest valid sample
/// seen in the current reference space. It uses separate enter/exit thresholds
/// so ordinary tracking noise around either boundary cannot chatter the player
/// collider between shapes.
#[derive(Debug, Default)]
pub struct VrCrouchDetector {
    standing_eye_height: Option<f32>,
    crouching: bool,
}

impl VrCrouchDetector {
    /// Observe a tracked, floor-relative eye height in meters.
    ///
    /// `None` means tracking is currently unavailable; the last request is
    /// retained until a valid sample returns.
    pub fn update(&mut self, tracked_eye_height: Option<f32>) -> bool {
        let Some(eye_height) = tracked_eye_height.filter(|height| {
            height.is_finite() && (0.0..=MAX_PLAUSIBLE_EYE_HEIGHT_METERS).contains(height)
        }) else {
            return self.crouching;
        };

        // The first pose may arrive while the headset is being donned or while
        // the user is already low. Let later, taller tracked poses refine the
        // calibration without requiring a separate calibration UI.
        let standing_eye_height = self
            .standing_eye_height
            .map_or(eye_height, |calibrated| calibrated.max(eye_height));
        self.standing_eye_height = Some(standing_eye_height);

        let height_ratio = eye_height / standing_eye_height;
        if self.crouching {
            if height_ratio > CROUCH_EXIT_HEIGHT_RATIO {
                self.crouching = false;
            }
        } else if height_ratio < CROUCH_ENTER_HEIGHT_RATIO {
            self.crouching = true;
        }

        self.crouching
    }

    /// Forget calibration after the runtime changes its floor reference space.
    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

#[cfg(test)]
mod tests {
    use super::VrCrouchDetector;

    #[test]
    fn crouches_and_stands_with_hysteresis() {
        let mut detector = VrCrouchDetector::default();

        assert!(!detector.update(Some(1.70)));
        assert!(!detector.update(Some(1.20)));
        assert!(detector.update(Some(1.18)));

        // Remain crouched throughout the dead band between 70% and 85%.
        assert!(detector.update(Some(1.30)));
        assert!(detector.update(Some(1.44)));
        assert!(!detector.update(Some(1.45)));

        // Remain standing throughout that same dead band.
        assert!(!detector.update(Some(1.30)));
    }

    #[test]
    fn calibration_adapts_when_the_first_sample_was_crouched() {
        let mut detector = VrCrouchDetector::default();

        assert!(!detector.update(Some(1.05)));
        assert!(!detector.update(Some(1.70)));
        assert!(detector.update(Some(1.10)));
    }

    #[test]
    fn unavailable_or_invalid_tracking_preserves_the_current_request() {
        let mut detector = VrCrouchDetector::default();
        detector.update(Some(1.70));
        assert!(detector.update(Some(1.00)));

        assert!(detector.update(None));
        assert!(detector.update(Some(f32::NAN)));
        assert!(detector.update(Some(f32::INFINITY)));
        assert!(detector.update(Some(-0.1)));
        assert!(detector.update(Some(3.1)));
    }

    #[test]
    fn reset_discards_state_and_calibration() {
        let mut detector = VrCrouchDetector::default();
        detector.update(Some(1.70));
        assert!(detector.update(Some(1.00)));

        detector.reset();

        assert!(!detector.update(Some(1.00)));
    }
}
