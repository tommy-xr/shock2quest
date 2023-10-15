use std::io;

use cgmath::Vector3;
use shipyard::Component;

use crate::ss2_common::*;

use serde::{Deserialize, Serialize};

#[derive(Debug, Component, Clone, Serialize, Deserialize)]
pub struct PropPhysAttr {
    pub gravity_scale: f32,
    pub mass: f32,
    pub density: f32,
    pub elasticity: f32,
    pub friction: f32,
    pub cog: Vector3<f32>,  // ?
    pub rotation_axes: u32, // ?
    pub rest_axes: u32,     // ?
    pub climbable: u32,     // ?
    pub edge_trigger: bool,
}

impl PropPhysAttr {
    pub fn read<T: io::Read + io::Seek>(reader: &mut T, _len: u32) -> PropPhysAttr {
        let gravity_scale = read_single(reader) / 100.0;
        let mass = read_single(reader);
        let density = read_single(reader);
        let elasticity = read_single(reader);
        let friction = read_single(reader);
        let cog = read_vec3(reader);

        let rotation_axes = read_u32(reader);
        let rest_axes = read_u32(reader);
        let climbable = read_u32(reader);
        let edge_trigger = read_bool(reader);

        let size = 48;
        let remainder = _len - size;

        // HACK: I'm unsure why this property can be variable length. Sometimes, it's length is reported
        // as 48, and others as 52. To handle this - we'll eat up the remaining bytes. But we could be
        // missing an interesting property.
        if remainder > 0 {
            read_bytes(reader, remainder as usize);
        }

        PropPhysAttr {
            gravity_scale,
            mass,
            density,
            elasticity,
            friction,
            cog,
            rotation_axes,
            rest_axes,
            climbable,
            edge_trigger,
        }
    }
}
