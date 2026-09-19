//! Transient controller output, separate from input and persistent game state.
use shipyard::{Unique, UniqueViewMut, World};

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize)]
pub struct HapticPulse {
    pub amplitude: f32,
    pub duration_ms: u32,
}

pub const SHOULDER_READY: HapticPulse = HapticPulse {
    amplitude: 0.45,
    duration_ms: 60,
};

pub const GUN_RECOIL: HapticPulse = HapticPulse {
    amplitude: 0.75,
    duration_ms: 45,
};
pub const GUN_SUPPORT: HapticPulse = HapticPulse {
    amplitude: 0.3,
    duration_ms: 30,
};

#[derive(Default, Unique, serde::Serialize)]
pub struct HapticFeedback {
    pub pending: [Option<HapticPulse>; 2],
    /// Monotonic request counters for headless/device diagnostics, left/right.
    pub sequence: [u64; 2],
}

impl HapticFeedback {
    pub fn request(&mut self, hand: crate::Handedness, pulse: HapticPulse) {
        let hand = crate::vr_config::hand_slot(hand);
        if !pulse.amplitude.is_finite() || pulse.amplitude <= 0.0 || pulse.duration_ms == 0 {
            return;
        }
        let pulse = HapticPulse {
            amplitude: pulse.amplitude.min(1.0),
            duration_ms: pulse.duration_ms,
        };
        self.sequence[hand] += 1;
        // Same-frame effects resolve deterministically: the stronger pulse
        // wins, then the longer one. A ready tick cannot overwrite an impact.
        if self.pending[hand].is_none_or(|current| {
            pulse.amplitude > current.amplitude
                || (pulse.amplitude == current.amplitude && pulse.duration_ms > current.duration_ms)
        }) {
            self.pending[hand] = Some(pulse);
        }
    }
}

/// Pure output arbitration. The runtime supplies a monotonic timestamp at
/// submission, after gameplay update, so slow frames cannot age a fresh pulse.
#[derive(Default)]
pub struct HapticMixer {
    active: [Option<(f32, std::time::Duration)>; 2],
}

impl HapticMixer {
    pub fn select(
        &mut self,
        now: std::time::Duration,
        requests: [Option<HapticPulse>; 2],
    ) -> [Option<HapticPulse>; 2] {
        std::array::from_fn(|hand| {
            let pulse = requests[hand]?;
            if self.active[hand]
                .is_some_and(|(amplitude, until)| now < until && amplitude > pulse.amplitude)
            {
                return None;
            }
            self.active[hand] = Some((
                pulse.amplitude,
                now.saturating_add(std::time::Duration::from_millis(pulse.duration_ms.into())),
            ));
            Some(pulse)
        })
    }
}

/// Consume once. A menu/transition world without gameplay output is silent.
pub fn take(world: &World) -> [Option<HapticPulse>; 2] {
    world
        .borrow::<UniqueViewMut<HapticFeedback>>()
        .map(|mut feedback| std::mem::take(&mut feedback.pending))
        .unwrap_or([None; 2])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn submitted_recoil_survives_weaker_later_requests_without_delaying_them() {
        use std::time::Duration;
        let mut mixer = HapticMixer::default();
        // A slow 100 ms update has just completed; the pulse starts NOW.
        let at = |ms| Duration::from_millis(ms);
        assert_eq!(
            mixer.select(at(100), [None, Some(GUN_RECOIL)]),
            [None, Some(GUN_RECOIL)]
        );
        assert_eq!(
            mixer.select(at(116), [Some(SHOULDER_READY), Some(SHOULDER_READY)]),
            [Some(SHOULDER_READY), None]
        );
        // Equal-strength shots retrigger; later weaker requests cannot truncate them.
        assert_eq!(
            mixer.select(at(120), [None, Some(GUN_RECOIL)])[1],
            Some(GUN_RECOIL)
        );
        assert_eq!(mixer.select(at(150), [None, Some(SHOULDER_READY)])[1], None);
        assert_eq!(
            mixer.select(at(165), [None; 2]),
            [None; 2],
            "no delayed tick"
        );
        assert_eq!(
            mixer.select(at(165), [None, Some(SHOULDER_READY)])[1],
            Some(SHOULDER_READY)
        );
        assert_eq!(
            mixer.select(at(166), [None, Some(GUN_RECOIL)])[1],
            Some(GUN_RECOIL)
        );
    }

    #[test]
    fn output_is_consumed_once_and_absent_worlds_are_silent() {
        let world = World::new();
        assert_eq!(take(&world), [None; 2]);
        world.add_unique(HapticFeedback::default());
        world
            .borrow::<UniqueViewMut<HapticFeedback>>()
            .unwrap()
            .request(crate::Handedness::Right, SHOULDER_READY);
        assert_eq!(take(&world), [None, Some(SHOULDER_READY)]);
        assert_eq!(take(&world), [None; 2]);
        assert_eq!(
            world
                .borrow::<shipyard::UniqueView<HapticFeedback>>()
                .unwrap()
                .sequence,
            [0, 1]
        );
    }

    #[test]
    fn strongest_same_frame_pulse_wins_in_either_order_and_hands_are_independent() {
        let impact = HapticPulse {
            amplitude: 0.8,
            duration_ms: 35,
        };
        for pulses in [[SHOULDER_READY, impact], [impact, SHOULDER_READY]] {
            let mut feedback = HapticFeedback::default();
            for pulse in pulses {
                feedback.request(crate::Handedness::Right, pulse);
            }
            feedback.request(crate::Handedness::Left, SHOULDER_READY);
            assert_eq!(feedback.pending, [Some(SHOULDER_READY), Some(impact)]);
            feedback.request(
                crate::Handedness::Right,
                HapticPulse {
                    amplitude: f32::NAN,
                    duration_ms: 60,
                },
            );
            assert_eq!(feedback.pending[1], Some(impact));
            assert_eq!(feedback.sequence, [1, 2]);
        }
    }
}
