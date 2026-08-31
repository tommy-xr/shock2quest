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
/// The world rep declares how many lights it holds; the array on disk is longer
/// than that and its full length varies by mission (WRRGB and WREXT missions
/// differ), so read only the declared count - everything past it is unrelated
/// data that would decode as plausible-looking junk.
use cgmath::{Vector3, vec3};

use std::io;

use crate::SCALE_FACTOR;
use crate::ss2_common::{read_single, read_vec3};

/// Bytes per record: position, direction, rgb brightness, two cone cosines and
/// a radius.
pub const RECORD_SIZE: usize = 48;

/// Sentinel in `inner`: this light is an omni, not a spotlight.
const NOT_A_SPOTLIGHT: f32 = -1.0;

/// One light as it lights objects. Brightness arrives pre-divided by the
/// engine's light scale, so it is much smaller than the authored brightness -
/// but it is NOT normalized: values run past 1.0 (up to ~12.5 on medsci1), so
/// whatever consumes this has to scale or tone-map rather than assume 0..1.
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
        self.inner > NOT_A_SPOTLIGHT
    }
}

#[derive(Debug, Clone)]
pub struct LightTable {
    /// The static lights, then the dynamic ones the engine keeps above them.
    lights: Vec<WorldLight>,
    num_static: usize,
}

impl LightTable {
    /// Reads the declared lights. `max_records` bounds the read to what is left
    /// in the world-rep chunk, so a mission whose counts overstate its table is
    /// short rather than a read past the end of the file.
    pub fn read<T: io::Read>(
        reader: &mut T,
        num_static: u32,
        num_dynamic: u32,
        max_records: usize,
    ) -> LightTable {
        let declared = num_static as usize + num_dynamic as usize;
        let to_read = declared.min(max_records);
        if to_read < declared {
            tracing::warn!(
                "Light table declares {} lights but only {} fit in the world rep",
                declared,
                to_read
            );
        }

        let lights = (0..to_read).map(|_| read_light(reader)).collect();

        LightTable {
            lights,
            num_static: (num_static as usize).min(to_read),
        }
    }

    /// Look up a light by the index a cell's light list carries. Out-of-range
    /// indices return `None` rather than panicking: the lists are authored data.
    pub fn get(&self, index: u16) -> Option<&WorldLight> {
        self.lights.get(index as usize)
    }

    /// The lights authored into the level, which is what the cells' index lists
    /// address.
    pub fn static_lights(&self) -> &[WorldLight] {
        &self.lights[..self.num_static]
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

    /// A table of `count` records, followed by unrelated data that must never
    /// be decoded as a light.
    fn table_bytes(count: usize) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend(record(10.0, NOT_A_SPOTLIGHT, 0.0));
        bytes.extend(record(20.0, 0.9, 32.0));
        for _ in 2..count {
            bytes.extend(record(0.0, NOT_A_SPOTLIGHT, 0.0));
        }
        for _ in 0..4 {
            bytes.extend(record(999.0, NOT_A_SPOTLIGHT, 0.0));
        }
        bytes
    }

    #[test]
    fn reads_a_light_record_field_by_field() {
        let table = LightTable::read(&mut io::Cursor::new(table_bytes(8)), 8, 0, usize::MAX);

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
        let table = LightTable::read(&mut io::Cursor::new(table_bytes(8)), 8, 0, usize::MAX);

        let light = table.get(1).expect("light 1 must parse");
        assert_eq!(light.position, vec3(-20.0, 22.0, 21.0) / SCALE_FACTOR);
        assert!(light.is_spotlight());
        assert_eq!(light.inner, 0.9);
        assert_eq!(light.radius, 32.0 / SCALE_FACTOR);
    }

    /// The array on disk runs past the declared count, and what follows is not
    /// lights - so reading more than was declared yields junk.
    #[test]
    fn reads_only_the_declared_lights() {
        let table = LightTable::read(&mut io::Cursor::new(table_bytes(8)), 8, 0, usize::MAX);

        assert_eq!(table.static_lights().len(), 8);
        assert!(table.get(8).is_none(), "must not decode past the count");
    }

    /// Dynamic lights sit above the static ones and are not part of the prefix
    /// the cells' index lists address.
    #[test]
    fn dynamic_lights_follow_the_static_ones() {
        let table = LightTable::read(&mut io::Cursor::new(table_bytes(8)), 6, 2, usize::MAX);

        assert_eq!(table.static_lights().len(), 6);
        assert!(table.get(7).is_some());
        assert!(table.get(8).is_none());
    }

    /// A mission whose counts overstate its table must come back short rather
    /// than reading off the end of the chunk.
    #[test]
    fn a_short_chunk_bounds_the_read() {
        let table = LightTable::read(&mut io::Cursor::new(table_bytes(4)), 900, 0, 4);

        assert_eq!(table.static_lights().len(), 4);
    }

    #[test]
    fn the_spotlight_sentinel_is_the_only_omni_case() {
        let omni = WorldLight {
            position: vec3(0.0, 0.0, 0.0),
            direction: vec3(0.0, 0.0, -1.0),
            brightness: vec3(1.0, 1.0, 1.0),
            inner: NOT_A_SPOTLIGHT,
            outer: 0.0,
            radius: 0.0,
        };
        assert!(!omni.is_spotlight());
        assert!(
            !WorldLight {
                inner: NOT_A_SPOTLIGHT - 0.0001,
                ..omni
            }
            .is_spotlight(),
            "a value below the sentinel is not a cone"
        );
        assert!(WorldLight { inner: 0.5, ..omni }.is_spotlight());
    }
}
