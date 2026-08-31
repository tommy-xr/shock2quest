///
/// light_table.rs
///
/// The object-lighting light table, stored in the mission's world-rep chunk
/// right after the BSP tree. This is a second light database, parallel to the
/// baked lightmaps: lightmaps light world geometry, this table lights *objects*
/// (props, creatures, held items). Each cell carries a list of indices into it
/// (`Cell::light_indices`) naming the lights that reach that cell, so lighting
/// an object is "find its cell, evaluate that cell's lights".
///
/// The table is a fixed-size array regardless of how many lights the mission
/// actually uses; `num_static` names the used prefix and the dynamic lights the
/// engine appends at runtime live directly above it.
use byteorder::ReadBytesExt;
use cgmath::{Vector3, vec3};

use std::io;

use crate::SCALE_FACTOR;
use crate::ss2_common::{read_single, read_vec3};

/// The table is always written at full size, used entries or not.
pub const LIGHT_TABLE_LEN: usize = 768;

/// A scratch array of the same record type follows the table and carries no
/// meaning - it is whatever the writer happened to have in its working buffer.
const SCRATCH_LEN: usize = 32;

/// Bytes per record: position, direction, rgb brightness, two cone cosines and
/// a radius.
const RECORD_SIZE: usize = 48;

/// Sentinel in `inner`: this light is an omni, not a spotlight.
const NOT_A_SPOTLIGHT: f32 = -1.0;

/// One light as it lights objects. Brightness arrives pre-divided by the
/// engine's light scale, so it is already in the 0..1-ish range the shader
/// wants rather than the authored brightness.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WorldLight {
    pub position: Vector3<f32>,
    /// Direction the cone points. Meaningless unless [`WorldLight::is_spotlight`].
    pub direction: Vector3<f32>,
    pub brightness: Vector3<f32>,
    /// Cosine of the cone angle at which the light is still at full strength.
    pub inner: f32,
    /// Cosine of the cone angle at which the light has fallen to nothing.
    pub outer: f32,
    /// Hard cutoff distance. Zero means the light has no distance cutoff at all
    /// (the common case - most authored lights rely on falloff instead).
    pub radius: f32,
}

impl WorldLight {
    pub fn is_spotlight(&self) -> bool {
        self.inner != NOT_A_SPOTLIGHT
    }

    /// Whether this light can reach `position` at all. Only the radius cutoff -
    /// cone and falloff are the renderer's business.
    pub fn reaches(&self, position: Vector3<f32>) -> bool {
        if self.radius == 0.0 {
            return true;
        }
        let delta = position - self.position;
        cgmath::dot(delta, delta) <= self.radius * self.radius
    }
}

#[derive(Debug, Clone)]
pub struct LightTable {
    lights: Vec<WorldLight>,
    num_static: u32,
    num_dynamic: u32,
}

impl LightTable {
    pub fn read<T: io::Read>(reader: &mut T, num_static: u32, num_dynamic: u32) -> LightTable {
        let mut lights = Vec::with_capacity(LIGHT_TABLE_LEN);
        for _ in 0..LIGHT_TABLE_LEN {
            lights.push(read_light(reader));
        }

        for _ in 0..(SCRATCH_LEN * RECORD_SIZE) {
            let _ = reader.read_u8();
        }

        LightTable {
            lights,
            num_static,
            num_dynamic,
        }
    }

    /// An empty table, for scenes with no world rep of their own.
    pub fn empty() -> LightTable {
        LightTable {
            lights: Vec::new(),
            num_static: 0,
            num_dynamic: 0,
        }
    }

    /// Look up a light by the index a cell's light list carries. Out-of-range
    /// indices return `None` rather than panicking: the lists are authored data.
    pub fn get(&self, index: u16) -> Option<&WorldLight> {
        self.lights.get(index as usize)
    }

    /// The used prefix of the table - the lights the mission actually authored.
    pub fn static_lights(&self) -> &[WorldLight] {
        let end = (self.num_static as usize).min(self.lights.len());
        &self.lights[..end]
    }

    pub fn num_static(&self) -> u32 {
        self.num_static
    }

    pub fn num_dynamic(&self) -> u32 {
        self.num_dynamic
    }
}

fn read_light<T: io::Read>(reader: &mut T) -> WorldLight {
    let position = read_vec3(reader) / SCALE_FACTOR;
    let direction = read_vec3(reader);
    // Stored in the same three-float shape as a position, but it is a colour -
    // reading it as a vector would swizzle the channels and negate red.
    let brightness = vec3(
        read_single(reader),
        read_single(reader),
        read_single(reader),
    );
    let inner = read_single(reader);
    let outer = read_single(reader);
    let radius = read_single(reader) / SCALE_FACTOR;

    WorldLight {
        position,
        direction,
        brightness,
        inner,
        outer,
        radius,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use byteorder::{LittleEndian, WriteBytesExt};
    use cgmath::vec3;

    fn record(marker: f32, inner: f32, radius: f32) -> Vec<u8> {
        let mut bytes = Vec::new();
        // position, direction, brightness - all marked so a stride error shows
        // up as the wrong field rather than as plausible-looking numbers.
        for value in [
            marker,
            marker + 1.0,
            marker + 2.0,
            0.0,
            0.0,
            -1.0,
            0.5,
            0.25,
            0.125,
        ] {
            bytes.write_f32::<LittleEndian>(value).unwrap();
        }
        bytes.write_f32::<LittleEndian>(inner).unwrap();
        bytes.write_f32::<LittleEndian>(0.7).unwrap();
        bytes.write_f32::<LittleEndian>(radius).unwrap();
        assert_eq!(bytes.len(), RECORD_SIZE);
        bytes
    }

    fn table_bytes() -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend(record(10.0, NOT_A_SPOTLIGHT, 0.0));
        bytes.extend(record(20.0, 0.9, 32.0));
        for _ in 2..LIGHT_TABLE_LEN {
            bytes.extend(record(0.0, NOT_A_SPOTLIGHT, 0.0));
        }
        // The scratch array the table is followed by.
        for _ in 0..SCRATCH_LEN {
            bytes.extend(record(999.0, NOT_A_SPOTLIGHT, 0.0));
        }
        bytes
    }

    #[test]
    fn reads_a_light_record_field_by_field() {
        let bytes = table_bytes();
        let table = LightTable::read(&mut io::Cursor::new(bytes), 2, 0);

        // Positions and directions come back in engine axes: the file's
        // (x, y, z) is read as (-x, z, y), and positions are scaled.
        let light = table.get(0).expect("light 0 must parse");
        assert_eq!(light.position, vec3(-10.0, 12.0, 11.0) / SCALE_FACTOR);
        assert_eq!(light.direction, vec3(0.0, -1.0, 0.0));
        assert_eq!(light.brightness, vec3(0.5, 0.25, 0.125));
        assert_eq!(light.outer, 0.7);
        assert!(!light.is_spotlight(), "inner -1.0 means omni");
        assert_eq!(light.radius, 0.0);
    }

    /// The stride check: light 1 is only where we expect it if the record is
    /// exactly 48 bytes.
    #[test]
    fn later_records_land_at_the_right_stride() {
        let bytes = table_bytes();
        let table = LightTable::read(&mut io::Cursor::new(bytes), 2, 0);

        let light = table.get(1).expect("light 1 must parse");
        assert_eq!(light.position, vec3(-20.0, 22.0, 21.0) / SCALE_FACTOR);
        assert!(light.is_spotlight());
        assert_eq!(light.inner, 0.9);
        assert_eq!(light.radius, 32.0 / SCALE_FACTOR);
    }

    /// The table is fixed-length and the scratch array that follows it is not
    /// part of it, so the last slot must be a real record and the marker value
    /// from the scratch array must never appear.
    #[test]
    fn table_is_fixed_length_and_excludes_the_scratch_array() {
        let bytes = table_bytes();
        let consumed = bytes.len();
        let mut cursor = io::Cursor::new(bytes);
        let table = LightTable::read(&mut cursor, 2, 0);

        assert!(table.get(LIGHT_TABLE_LEN as u16 - 1).is_some());
        assert!(table.get(LIGHT_TABLE_LEN as u16).is_none());
        assert!(
            table
                .static_lights()
                .iter()
                .all(|l| l.position.x != -999.0 / SCALE_FACTOR)
        );
        assert_eq!(
            cursor.position() as usize,
            consumed,
            "the scratch array must be consumed too, or every chunk after the \
             world rep reads from the wrong offset"
        );
    }

    #[test]
    fn static_lights_are_the_used_prefix() {
        let bytes = table_bytes();
        let table = LightTable::read(&mut io::Cursor::new(bytes), 2, 4);

        assert_eq!(table.static_lights().len(), 2);
        assert_eq!(table.num_dynamic(), 4);
    }

    #[test]
    fn radius_zero_reaches_everywhere() {
        let omni = WorldLight {
            position: vec3(0.0, 0.0, 0.0),
            direction: vec3(0.0, 0.0, -1.0),
            brightness: vec3(1.0, 1.0, 1.0),
            inner: NOT_A_SPOTLIGHT,
            outer: 0.0,
            radius: 0.0,
        };
        assert!(omni.reaches(vec3(1000.0, 0.0, 0.0)));

        let bounded = WorldLight {
            radius: 10.0,
            ..omni
        };
        assert!(bounded.reaches(vec3(9.0, 0.0, 0.0)));
        assert!(!bounded.reaches(vec3(11.0, 0.0, 0.0)));
    }
}
