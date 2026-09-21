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

/// Smasher feedback follows charge transitions, never the per-frame readout.
pub(crate) fn melee_charge_pulse(previous: Option<f32>, next: Option<f32>) -> Option<HapticPulse> {
    match (previous, next) {
        (previous, Some(0.0)) if previous != Some(0.0) => Some(HapticPulse {
            amplitude: 0.2,
            duration_ms: 30,
        }),
        (Some(previous), Some(next)) if previous < 1.0 && next >= 1.0 => Some(HapticPulse {
            amplitude: 0.6,
            duration_ms: 70,
        }),
        _ => None,
    }
}

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
        // Same-frame effects resolve deterministically: the stronger pulse
        // wins, then the longer one. A ready tick cannot overwrite an impact.
        if self.pending[hand].is_none_or(|current| {
            pulse.amplitude > current.amplitude
                || (pulse.amplitude == current.amplitude && pulse.duration_ms > current.duration_ms)
        }) {
            self.pending[hand] = Some(pulse);
        }
        self.sequence[hand] += 1;
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
    fn smasher_charge_cues_edges_only() {
        let start = melee_charge_pulse(None, Some(0.0)).expect("charge starts with a pulse");
        let ready = melee_charge_pulse(Some(0.99), Some(1.0)).expect("full charge cues ready");
        assert!(ready.amplitude > start.amplitude);
        assert!(ready.duration_ms > start.duration_ms);
        assert_eq!(melee_charge_pulse(Some(0.0), Some(1.0)), Some(ready));
        for (before, after) in [
            (Some(0.0), Some(0.0)),
            (Some(0.1), Some(0.9)),
            (Some(1.0), Some(1.0)),
            (Some(0.2), None),
            (Some(1.0), None),
            (None, None),
            (None, Some(1.0)),
        ] {
            assert_eq!(melee_charge_pulse(before, after), None);
        }
        assert_eq!(melee_charge_pulse(Some(1.0), Some(0.0)), Some(start));
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
