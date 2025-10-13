use std::io::{Read, Seek};

use cgmath::Vector3;

use crate::ss2_common::{read_i32, read_single, read_u16, read_u32, read_vec3};

// What does CAL stand for?
pub struct SystemShock2Cal {
    pub version: i32,
    pub num_torsos: u32,
    pub num_limbs: u32,

    pub torsos: Vec<Torso>,
    pub limbs: Vec<Limb>,
}

pub fn read<T: Read + Seek>(reader: &mut T) -> SystemShock2Cal {
    let version = read_i32(reader);
    let num_torsos = read_u32(reader);
    let num_limbs = read_u32(reader);

    let mut torsos = Vec::new();
    for _ in 0..num_torsos {
        let torso = read_torso(reader);
        torsos.push(torso);
    }

    let mut limbs = Vec::new();
    for _ in 0..num_limbs {
        let limb = read_limb(reader);
        limbs.push(limb)
    }

    SystemShock2Cal {
        version,
        num_torsos,
        num_limbs,
        torsos,
        limbs,
    }
}

#[derive(Debug)]
pub struct Torso {
    pub joint: u32,
    pub parent: i32,
    pub fixed_count: i32,
    pub fixed_joints: Vec<u32>,
    pub fixed_joint_offset: Vec<Vector3<f32>>,
}


pub fn read_torso<T: Read + Seek>(reader: &mut T) -> Torso {
    let joint = read_u32(reader);
    let parent = read_i32(reader);
    let fixed_count = read_i32(reader);

    let mut fixed_joints = Vec::new();
    for _ in 0..16 {
        let id = read_u32(reader);
        fixed_joints.push(id);
    }

    let mut fixed_joint_offset = Vec::new();
    for _ in 0..16 {
        let offset = read_vec3(reader);
        fixed_joint_offset.push(offset);
    }

    Torso {
        joint,
        parent,
        fixed_count,
        fixed_joints,
        fixed_joint_offset,
    }
}

#[derive(Debug)]
pub struct Limb {
    pub torso_index: u32,
    pub bend: i32,
    pub num_segments: i32,
    pub attachment_joint: u16, // index or id?

    pub segments: Vec<u16>,
    pub segment_directions: Vec<Vector3<f32>>,
    pub segment_lengths: Vec<f32>,
}

const NUM_LIMB_JOINTS: usize = 16;

pub fn read_limb<T: Read + Seek>(reader: &mut T) -> Limb {
    let torso_index = read_u32(reader);
    let bend = read_i32(reader);
    let num_segments = read_i32(reader);
    let attachment_joint = read_u16(reader);

    let mut segments = Vec::new();
    for _ in 0..NUM_LIMB_JOINTS {
        segments.push(read_u16(reader))
    }

    let mut segment_directions = Vec::new();
    for _ in 0..NUM_LIMB_JOINTS {
        segment_directions.push(read_vec3(reader))
    }

    let mut segment_lengths = Vec::new();
    for _ in 0..NUM_LIMB_JOINTS {
        segment_lengths.push(read_single(reader))
    }

    Limb {
        torso_index,
        bend,
        num_segments,
        attachment_joint,
        segments,
        segment_directions,
        segment_lengths,
    }
}
