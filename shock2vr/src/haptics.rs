//! Transient controller output, separate from input and persistent game state.
use shipyard::{Unique, UniqueViewMut, World};

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize)]
pub struct HapticPulse {
    pub amplitude: f32,
    pub duration_ms: u32,
}

/// One psi selector step: a light detent, like a notched dial.
pub const PSI_NAV_TICK: HapticPulse = HapticPulse {
    amplitude: 0.25,
    duration_ms: 12,
};

pub const SHOULDER_READY: HapticPulse = HapticPulse {
    amplitude: 0.45,
    duration_ms: 60,
};

/// A held item now points at a target that will take it on release.
pub const TOOL_READY: HapticPulse = HapticPulse {
    amplitude: 0.45,
    duration_ms: 60,
};
/// A released item was consumed by its target: firmer than the ready tick.
pub const TOOL_APPLIED: HapticPulse = HapticPulse {
    amplitude: 0.7,
    duration_ms: 90,
};

/// Ready cue on the entered edge only, like the shoulder slot: `next` is the
/// accepting release target this frame, `previous` last frame's.
pub(crate) fn tool_ready_pulse(
    previous: Option<shipyard::EntityId>,
    next: Option<shipyard::EntityId>,
) -> Option<HapticPulse> {
    (next.is_some() && next != previous).then_some(TOOL_READY)
}

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

/// Psi amp charge, sent every frame of the hold (each pulse just outlasts a
/// frame): a rumble swelling toward the overload zone, a detent on entering
/// it, then a steady buzz that says "release now".
pub(crate) fn psi_charge_pulse(previous: f32, fraction: f32) -> HapticPulse {
    let zone = crate::psi::OVERLOAD_ZONE_START;
    if previous < zone && fraction >= zone {
        PSI_OVERLOAD_READY
    } else if fraction >= zone {
        HapticPulse {
            amplitude: 0.45,
            duration_ms: 20,
        }
    } else {
        HapticPulse {
            amplitude: 0.1 + 0.25 * (fraction / zone).clamp(0.0, 1.0),
            duration_ms: 20,
        }
    }
}

pub const PSI_OVERLOAD_READY: HapticPulse = HapticPulse {
    amplitude: 0.8,
    duration_ms: 40,
};
pub const PSI_CAST: HapticPulse = HapticPulse {
    amplitude: 0.7,
    duration_ms: 50,
};
pub const PSI_OVERLOAD_CAST: HapticPulse = HapticPulse {
    amplitude: 1.0,
    duration_ms: 120,
};
pub const PSI_BURNOUT: HapticPulse = HapticPulse {
    amplitude: 1.0,
    duration_ms: 350,
};
/// A cast that went nowhere: a long dull buzz, unlike a cast's sharp kick.
/// Still louder than the charge, which the mixer would otherwise let mask it.
pub const PSI_FIZZLE: HapticPulse = HapticPulse {
    amplitude: 0.5,
    duration_ms: 150,
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
    /// The latest winning request per hand, never consumed (read with
    /// `sequence` to tell new from stale): the debug runtime's idle zero-dt
    /// frames `take` a pulse before a headless client can read it.
    pub last: [Option<HapticPulse>; 2],
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
        self.last[hand] = self.pending[hand];
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
    fn tool_ready_cues_on_entry_only() {
        let crate_a = shipyard::EntityId::new_from_index_and_gen(1, 0);
        let crate_b = shipyard::EntityId::new_from_index_and_gen(2, 0);
        assert_eq!(tool_ready_pulse(None, Some(crate_a)), Some(TOOL_READY));
        // Lingering over the same target, leaving it, and nothing at all are silent.
        assert_eq!(tool_ready_pulse(Some(crate_a), Some(crate_a)), None);
        assert_eq!(tool_ready_pulse(Some(crate_a), None), None);
        assert_eq!(tool_ready_pulse(None, None), None);
        // Sweeping straight onto another accepting target is a new entry.
        assert_eq!(
            tool_ready_pulse(Some(crate_a), Some(crate_b)),
            Some(TOOL_READY)
        );
        assert!(TOOL_APPLIED.amplitude > TOOL_READY.amplitude);
    }

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
    fn psi_charge_swells_then_detents_once_at_the_overload_zone() {
        let zone = crate::psi::OVERLOAD_ZONE_START;
        let early = psi_charge_pulse(0.0, 0.1);
        let late = psi_charge_pulse(0.7, 0.8);
        assert!(late.amplitude > early.amplitude);
        assert_eq!(psi_charge_pulse(zone - 0.01, zone), PSI_OVERLOAD_READY);
        let held = psi_charge_pulse(zone, zone + 0.01);
        assert_ne!(held, PSI_OVERLOAD_READY);
        assert!(held.amplitude > late.amplitude && held.amplitude < PSI_OVERLOAD_READY.amplitude);
    }

    #[test]
    fn psi_outcomes_cut_through_a_charge_still_playing() {
        let zone = crate::psi::OVERLOAD_ZONE_START;
        for outcome in [PSI_CAST, PSI_OVERLOAD_CAST, PSI_BURNOUT, PSI_FIZZLE] {
            let mut mixer = HapticMixer::default();
            let buzz = psi_charge_pulse(zone, 1.0);
            mixer.select(std::time::Duration::ZERO, [None, Some(buzz)]);
            // One 72 Hz frame later the 20 ms buzz is still active.
            let next = mixer.select(
                std::time::Duration::from_micros(13_900),
                [None, Some(outcome)],
            );
            assert_eq!(next, [None, Some(outcome)]);
        }
    }

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
