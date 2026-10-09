//! Deterministic NewDark diffuse-UV offsets. Lightmap coordinates never move.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Waveform {
    Sine,
    Sawtooth,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Wave {
    pub shape: Waveform,
    pub bias: f32,
    pub amplitude: f32,
    pub phase: f32,
    pub period_ms: u32,
}
impl Wave {
    pub fn evaluate(self, seconds: f32) -> f32 {
        if self.period_ms == 0 || !seconds.is_finite() {
            return self.bias;
        }
        let phase =
            (seconds as f64 * 1000.0 / self.period_ms as f64 + self.phase as f64).rem_euclid(1.0);
        // NewDark waves have a normalized [0,1] range, including SINE.
        let value = match self.shape {
            Waveform::Sine => (1.0 + (phase * std::f64::consts::TAU).sin()) * 0.5,
            Waveform::Sawtooth => phase,
        };
        (self.bias as f64 + self.amplitude as f64 * value) as f32
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum UvMotion {
    #[default]
    None,
    Scroll([f32; 2]),
    OffsetWaves([Option<Wave>; 2]),
}
impl UvMotion {
    pub fn offset(self, seconds: f32) -> [f32; 2] {
        if !seconds.is_finite() {
            return [0.0; 2];
        }
        match self {
            Self::None => [0.0; 2],
            // All supported animated terrain passes wrap. Reduce on the CPU in
            // double precision to avoid large UVs losing precision on GLES.
            Self::Scroll(speed) => {
                speed.map(|v| (v as f64 * seconds as f64).rem_euclid(1.0) as f32)
            }
            Self::OffsetWaves(waves) => {
                waves.map(|w| w.map_or(0.0, |w| w.evaluate(seconds).rem_euclid(1.0)))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn waves_use_normalized_amplitude_phase_and_millisecond_periods() {
        let sine = Wave {
            shape: Waveform::Sine,
            bias: -1.0,
            amplitude: 0.25,
            phase: 0.0,
            period_ms: 1000,
        };
        assert_eq!(sine.evaluate(0.0), -0.875);
        assert_eq!(sine.evaluate(0.25), -0.75);
        assert_eq!(sine.evaluate(0.75), -1.0);
        assert_eq!(sine.evaluate(1.25), sine.evaluate(0.25));
        let saw = Wave {
            shape: Waveform::Sawtooth,
            bias: 0.0,
            amplitude: 0.1,
            phase: 0.1,
            period_ms: 20000,
        };
        assert!((saw.evaluate(10.0) - 0.06).abs() < 0.00001);
        assert!((saw.evaluate(18.0)).abs() < 0.00001);
        let offsets = UvMotion::OffsetWaves([Some(sine), Some(saw)]).offset(0.25);
        assert_eq!(offsets[0], 0.25);
        assert!((offsets[1] - saw.evaluate(0.25)).abs() < 0.00001);
    }
    #[test]
    fn scroll_wraps_both_directions_and_invalid_clock_is_safe() {
        let scroll = UvMotion::Scroll([0.5, -0.5]);
        assert_eq!(scroll.offset(0.5), [0.25, 0.75]);
        assert_eq!(scroll.offset(2.0), [0.0, 0.0]);
        assert_eq!(scroll.offset(f32::NAN), [0.0, 0.0]);
        assert!(scroll.offset(f32::MAX).iter().all(|v| v.is_finite()));
    }
}
