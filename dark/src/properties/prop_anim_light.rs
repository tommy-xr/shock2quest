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
    pub rising: bool,
    pub countdown_ms: i32,
    pub inactive: bool,
    pub radius: f32,
}

impl PropAnimLight {
    pub fn read<T: io::Read + io::Seek>(reader: &mut T, len: u32) -> Self {
        let _object_id = read_i32(reader);
        let offset = read_vec3(reader);
        let _quad_lit = read_i32(reader);
        let cell_index = read_i16(reader);
        let hit_cells = read_i16(reader);
        let light_number = read_i16(reader);
        let mode = AnimLightMode::from_raw(read_u16(reader));
        let brighten_time_ms = read_i32(reader);
        let dim_time_ms = read_i32(reader);
        let min_brightness = read_single(reader);
        let max_brightness = read_single(reader);
        let _refresh = read_i32(reader);
        let rising = read_bool(reader);
        let countdown_ms = read_i32(reader);
        let inactive = read_bool(reader);
        let radius = read_single(reader);
        let _runtime_light_handle = read_i32(reader);

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
            rising,
            countdown_ms,
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

    /// Initial intensity represented by the persisted Dark property. Temporal
    /// cycling modes start at their authored minimum; their later animation is
    /// intentionally separate from the steady switched-light support.
    pub fn initial_intensity(&self) -> f32 {
        if self.inactive {
            return 0.0;
        }
        self.steady_intensity()
            .unwrap_or_else(|| self.minimum_intensity())
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
        bytes.extend_from_slice(&714i32.to_le_bytes());
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
        bytes.extend_from_slice(&0i32.to_le_bytes());
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
        assert!(light.rising);
        assert!(!light.inactive);
        assert_eq!(light.radius, 25.0);
    }

    #[test]
    fn minimum_brightness_is_relative_to_authored_maximum() {
        let light = PropAnimLight {
            offset: Vector3::new(0.0, 0.0, 0.0),
            cell_index: -1,
            hit_cells: 0,
            light_number: 0,
            mode: AnimLightMode::MinBrightness,
            brighten_time_ms: 0,
            dim_time_ms: 0,
            min_brightness: 25.0,
            max_brightness: 100.0,
            rising: false,
            countdown_ms: 0,
            inactive: false,
            radius: 0.0,
        };

        assert_eq!(light.steady_intensity(), Some(0.25));
    }
}
