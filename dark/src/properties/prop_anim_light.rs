use std::io;

use cgmath::Vector3;
use serde::{Deserialize, Serialize};
use shipyard::Component;

use crate::ss2_common::{
    read_bool, read_bytes, read_i16, read_i32, read_single, read_u16, read_vec3,
};

/// Dark's animated-light operating mode (`sAnimLight.mode`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AnimLightMode {
    Alternate,
    AlternateSmoothly,
    Random,
    MinBrightness,
    MaxBrightness,
    ZeroBrightness,
    BrightenSmoothly,
    DimSmoothly,
    SemiRandom,
    Flicker,
    Unknown(u16),
}

impl AnimLightMode {
    fn from_raw(raw: u16) -> Self {
        match raw {
            0 => Self::Alternate,
            1 => Self::AlternateSmoothly,
            2 => Self::Random,
            3 => Self::MinBrightness,
            4 => Self::MaxBrightness,
            5 => Self::ZeroBrightness,
            6 => Self::BrightenSmoothly,
            7 => Self::DimSmoothly,
            8 => Self::SemiRandom,
            9 => Self::Flicker,
            other => Self::Unknown(other),
        }
    }
}

/// Authored `P$AnimLight` state. `light_number` indexes the switchable
/// lightmap layers stored in the mission world-representation cells.
#[derive(Debug, Component, Clone, Serialize, Deserialize)]
pub struct PropAnimLight {
    pub offset: Vector3<f32>,
    pub cell_index: i16,
    pub hit_cells: i16,
    pub light_number: i16,
    pub mode: AnimLightMode,
    pub brighten_time_ms: i32,
    pub dim_time_ms: i32,
    pub min_brightness: f32,
    pub max_brightness: f32,
    /// Current brightness at byte 44, not the refresh flag (which is at 16).
    /// None supports saves written before the current brightness was retained.
    #[serde(default)]
    pub brightness: Option<f32>,
    pub rising: bool,
    pub countdown_ms: i32,
    /// Preserve sub-millisecond simulation time instead of losing 2/3 ms on
    /// every 60 Hz update. Stored with the phase so saving cannot reset it.
    #[serde(default)]
    pub countdown_fraction_ms: f64,
    pub inactive: bool,
    pub radius: f32,
}

impl PropAnimLight {
    pub fn read<T: io::Read + io::Seek>(reader: &mut T, len: u32) -> Self {
        let _base_brightness = read_single(reader);
        let offset = read_vec3(reader);
        let _refresh = read_i32(reader);
        let cell_index = read_i16(reader);
        let hit_cells = read_i16(reader);
        let light_number = read_i16(reader);
        let mode = AnimLightMode::from_raw(read_u16(reader));
        let brighten_time_ms = read_i32(reader);
        let dim_time_ms = read_i32(reader);
        let min_brightness = read_single(reader);
        let max_brightness = read_single(reader);
        let brightness = Some(read_single(reader));
        let rising = read_bool(reader);
        let countdown_ms = read_i32(reader);
        let inactive = read_bool(reader);
        let radius = read_single(reader);
        let _notify_script = read_i32(reader);

        const SS2_SIZE: u32 = 68;
        if len > SS2_SIZE {
            let _trailing = read_bytes(reader, (len - SS2_SIZE) as usize);
        }

        Self {
            offset,
            cell_index,
            hit_cells,
            light_number,
            mode,
            brighten_time_ms,
            dim_time_ms,
            min_brightness,
            max_brightness,
            brightness,
            rising,
            countdown_ms,
            countdown_fraction_ms: 0.0,
            inactive,
            radius,
        }
    }

    /// Intensity used by the switchable lightmap layer for steady modes.
    pub fn steady_intensity(&self) -> Option<f32> {
        match self.mode {
            AnimLightMode::MaxBrightness => Some(1.0),
            AnimLightMode::ZeroBrightness => Some(0.0),
            AnimLightMode::MinBrightness => {
                if self.max_brightness > 0.0 {
                    Some((self.min_brightness / self.max_brightness).clamp(0.0, 1.0))
                } else {
                    Some(0.0)
                }
            }
            _ => None,
        }
    }

    pub fn minimum_intensity(&self) -> f32 {
        if self.max_brightness > 0.0 {
            (self.min_brightness / self.max_brightness).clamp(0.0, 1.0)
        } else {
            0.0
        }
    }

    /// Current intensity, including a frozen animation's persisted brightness.
    /// The fallback preserves the interpretation used by older port saves.
    pub fn initial_intensity(&self) -> f32 {
        if let Some(brightness) = self.brightness {
            return if self.max_brightness > 0.0 {
                (brightness / self.max_brightness).clamp(0.0, 1.0)
            } else {
                0.0
            };
        }
        if self.inactive {
            return 0.0;
        }
        self.steady_intensity()
            .unwrap_or_else(|| self.minimum_intensity())
    }

    /// Advance Dark's FLIP/Alternate mode (`render/animlgt.c`,
    /// `AnimLightUpdateTimer`). Rising is the *low* interval, then
    /// falling holds the maximum. The reference switches only after countdown
    /// becomes negative, and replaces durations below 5 ms with 63 ms.
    /// Other temporal modes are deliberately left untouched in this first pass.
    pub fn advance_alternating(&mut self, elapsed: std::time::Duration) -> bool {
        if self.inactive || self.mode != AnimLightMode::Alternate || elapsed.is_zero() {
            return false;
        }
        let before = self.initial_intensity();
        if self.brighten_time_ms < 5 {
            self.brighten_time_ms = 63;
        }
        if self.dim_time_ms < 5 {
            self.dim_time_ms = 63;
        }
        let period = f64::from(self.brighten_time_ms) + f64::from(self.dim_time_ms);
        let mut remaining = f64::from(self.countdown_ms) + self.countdown_fraction_ms
            - elapsed.as_secs_f64() * 1000.0;
        // Skip complete cycles so a large time step cannot run an unbounded
        // loop. Only the final visible phase produces a sound/render change.
        if remaining < -period {
            remaining += (-remaining / period).floor() * period;
        }
        while remaining < 0.0 {
            self.rising = !self.rising;
            remaining += f64::from(if self.rising {
                self.brighten_time_ms
            } else {
                self.dim_time_ms
            });
        }
        self.countdown_ms = remaining.floor() as i32;
        self.countdown_fraction_ms = remaining.fract();
        self.brightness = Some(if self.rising {
            self.min_brightness
        } else {
            self.max_brightness
        });
        before != self.initial_intensity()
    }

    pub fn turn_off_intensity(&self) -> f32 {
        match self.mode {
            AnimLightMode::MinBrightness
            | AnimLightMode::BrightenSmoothly
            | AnimLightMode::DimSmoothly => self.minimum_intensity(),
            _ => 0.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    #[test]
    fn reads_ss2_animated_light_layout() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&0.0f32.to_le_bytes());
        for value in [1.0f32, 2.0, 3.0] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes.extend_from_slice(&1i32.to_le_bytes());
        bytes.extend_from_slice(&12i16.to_le_bytes());
        bytes.extend_from_slice(&3i16.to_le_bytes());
        bytes.extend_from_slice(&42i16.to_le_bytes());
        bytes.extend_from_slice(&4u16.to_le_bytes());
        bytes.extend_from_slice(&250i32.to_le_bytes());
        bytes.extend_from_slice(&500i32.to_le_bytes());
        bytes.extend_from_slice(&10.0f32.to_le_bytes());
        bytes.extend_from_slice(&100.0f32.to_le_bytes());
        bytes.extend_from_slice(&75.0f32.to_le_bytes());
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(&125i32.to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(&25.0f32.to_le_bytes());
        bytes.extend_from_slice(&(-1i32).to_le_bytes());
        assert_eq!(bytes.len(), 68);

        let light = PropAnimLight::read(&mut Cursor::new(bytes), 68);

        // read_vec3 applies the repository's Dark -> world axis conversion.
        assert_eq!(light.offset, Vector3::new(-1.0, 3.0, 2.0));
        assert_eq!(light.cell_index, 12);
        assert_eq!(light.hit_cells, 3);
        assert_eq!(light.light_number, 42);
        assert_eq!(light.mode, AnimLightMode::MaxBrightness);
        assert_eq!(light.brighten_time_ms, 250);
        assert_eq!(light.dim_time_ms, 500);
        assert_eq!(light.steady_intensity(), Some(1.0));
        assert_eq!(light.brightness, Some(75.0));
        assert!(light.rising);
        assert!(!light.inactive);
        assert_eq!(light.radius, 25.0);
    }

    fn light() -> PropAnimLight {
        PropAnimLight {
            offset: Vector3::new(0.0, 0.0, 0.0),
            cell_index: -1,
            hit_cells: 0,
            light_number: 0,
            mode: AnimLightMode::MinBrightness,
            brighten_time_ms: 0,
            dim_time_ms: 0,
            min_brightness: 25.0,
            max_brightness: 100.0,
            brightness: None,
            rising: false,
            countdown_ms: 0,
            countdown_fraction_ms: 0.0,
            inactive: false,
            radius: 0.0,
        }
    }

    #[test]
    fn minimum_brightness_is_relative_to_authored_maximum() {
        assert_eq!(light().steady_intensity(), Some(0.25));
    }

    #[test]
    fn alternate_obeys_asymmetric_intervals_and_exact_boundary() {
        let mut light = light();
        light.mode = AnimLightMode::Alternate;
        light.brighten_time_ms = 1000;
        light.dim_time_ms = 100;
        light.rising = true;
        light.brightness = Some(25.0);
        assert!(light.advance_alternating(std::time::Duration::from_millis(1)));
        assert_eq!(light.initial_intensity(), 1.0);
        assert_eq!(light.countdown_ms, 99);
        assert!(!light.advance_alternating(std::time::Duration::from_millis(99)));
        assert_eq!(light.initial_intensity(), 1.0);
        assert!(light.advance_alternating(std::time::Duration::from_millis(1)));
        assert_eq!(light.initial_intensity(), 0.25);
        assert_eq!(light.countdown_ms, 999);
    }

    #[test]
    fn frozen_lights_retain_brightness_and_phase() {
        let mut light = light();
        light.mode = AnimLightMode::Alternate;
        light.inactive = true;
        light.brightness = Some(75.0);
        light.countdown_ms = 42;
        assert!(!light.advance_alternating(std::time::Duration::from_secs(1)));
        assert_eq!(light.initial_intensity(), 0.75);
        assert_eq!(light.countdown_ms, 42);
    }

    #[test]
    fn fractional_time_and_phase_survive_save_and_match_one_large_step() {
        let mut stepped = light();
        stepped.mode = AnimLightMode::Alternate;
        stepped.brighten_time_ms = 500;
        stepped.dim_time_ms = 500;
        let mut single = stepped.clone();
        let frame = std::time::Duration::from_secs_f64(1.0 / 60.0);
        for _ in 0..17 {
            stepped.advance_alternating(frame);
        }
        let encoded = serde_json::to_string(&stepped).unwrap();
        let mut restored: PropAnimLight = serde_json::from_str(&encoded).unwrap();
        for _ in 17..601 {
            restored.advance_alternating(frame);
        }
        single.advance_alternating(frame * 601);
        assert_eq!(restored.rising, single.rising);
        assert_eq!(restored.initial_intensity(), single.initial_intensity());
        let time_left =
            |light: &PropAnimLight| f64::from(light.countdown_ms) + light.countdown_fraction_ms;
        assert!((time_left(&restored) - time_left(&single)).abs() < 1e-6);
    }

    #[test]
    fn legacy_saves_and_zero_durations_are_supported() {
        let mut json = serde_json::to_value(light()).unwrap();
        json.as_object_mut().unwrap().remove("brightness");
        json.as_object_mut()
            .unwrap()
            .remove("countdown_fraction_ms");
        let mut light: PropAnimLight = serde_json::from_value(json).unwrap();
        assert_eq!(light.initial_intensity(), 0.25);
        light.mode = AnimLightMode::Alternate;
        light.advance_alternating(std::time::Duration::from_millis(1));
        assert_eq!(light.brighten_time_ms, 63);
        assert_eq!(light.dim_time_ms, 63);
        assert_eq!(light.countdown_ms, 62);
    }
}
