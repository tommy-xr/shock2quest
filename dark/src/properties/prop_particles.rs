use std::io;

use cgmath::Vector3;

use shipyard::Component;

use crate::ss2_common::*;

use serde::{Deserialize, Serialize};

#[derive(Debug, Component, Clone, Deserialize, Serialize)]
pub struct PropParticleLaunchInfo {
    pub launch_type: u32, // TODO: Flags
    pub loc_min: Vector3<f32>,
    pub loc_max: Vector3<f32>,
    pub vel_min: Vector3<f32>,
    pub vel_max: Vector3<f32>,
    pub min_radius: f32,
    pub max_radius: f32,
    pub min_time: f32,
    pub max_time: f32,
}

impl PropParticleLaunchInfo {
    pub fn read<T: io::Seek + io::Read>(reader: &mut T, _len: u32) -> PropParticleLaunchInfo {
        let launch_type = read_u32(reader);
        let loc_min = read_vec3(reader);
        let loc_max = read_vec3(reader);

        let vel_min = read_vec3(reader);
        let vel_max = read_vec3(reader);

        let min_radius = read_single(reader);
        let max_radius = read_single(reader);

        let min_time = read_single(reader);
        let max_time = read_single(reader);

        let _unk1 = read_u32(reader);
        let _unk2 = read_u32(reader);

        let _unk3 = read_bytes(reader, 64);
        PropParticleLaunchInfo {
            launch_type,
            loc_min,
            loc_max,
            vel_min,
            vel_max,
            min_radius,
            max_radius,
            min_time,
            max_time,
        }
    }
}

#[derive(Debug, Component, Clone, Serialize, Deserialize)]
pub struct PropParticleGroup {
    pub render_type: u32,
    pub motion_type: u32,
    pub animation_type: u32,
    pub num: u32,
    pub velocity: Vector3<f32>,
    pub gravity: Vector3<f32>,
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
    pub spin: Vector3<f32>,
    pub is_active: bool,
    pub is_worldspace: bool,
    pub size: f32,
    pub scale_vel: f32,
    pub prev_loc: Vector3<f32>,

    pub bbox_min: Vector3<f32>,
    pub bbox_max: Vector3<f32>,
    pub radius: f32,

    pub launch_time: f32,
    pub fade_time: f32,
    pub model_name: String,
}

impl PropParticleGroup {
    pub fn read<T: io::Seek + io::Read>(reader: &mut T, len: u32) -> PropParticleGroup {
        let _unk1 = read_bytes(reader, 36);
        let _unk2 = read_u32(reader);

        // let active = read_u32(reader);
        let render_type = read_u32(reader);
        let motion_type = read_u32(reader);
        let animation_type = read_u32(reader);

        let _unk3 = read_u32(reader);
        let _unk4 = read_u32(reader);

        let num = read_u32(reader);

        let _unk5 = read_bytes(reader, 24);

        let velocity = read_vec3(reader);
        let gravity = read_vec3(reader);

        let r = read_u8(reader);
        let g = read_u8(reader);
        let b = read_u8(reader);
        let a = read_u8(reader);

        let _always_simulate = read_bool_u8(reader);
        let _unk = read_bool_u8(reader);
        let _unk = read_bool_u8(reader);
        let _unk = read_bool_u8(reader);

        let _terrain_collide = read_bool_u8(reader);
        let _unk = read_bool_u8(reader);
        let _ignore_attach_refs = read_bool_u8(reader);
        let _unk = read_bool_u8(reader);

        let _unk_launch_info = read_u32(reader);
        let spin = read_vec3(reader);
        let _pulse_period = read_u32(reader);
        let _unk = read_u32(reader);
        let _unk = read_u32(reader);

        let _unk = read_u32(reader);

        let _unk = read_bool_u8(reader);
        let is_worldspace = read_bool_u8(reader);
        let _unk = read_bool_u8(reader);
        let is_active = read_bool_u8(reader);

        let _ms_offset = read_u32(reader);
        let size = read_single(reader);
        let _unk = read_u32(reader);
        let _unk = read_u32(reader);

        let prev_loc = read_vec3(reader);
        let scale_vel = read_single(reader);

        let _unk = read_u32(reader);
        let bbox_min = read_vec3(reader);
        let bbox_max = read_vec3(reader);
        let radius = read_single(reader);

        // Runtime metadata?
        let _unk = read_u32(reader);
        let _unk = read_u32(reader);
        let _unk = read_u32(reader);

        // More runtime data?
        let _unk = read_u8(reader);
        let _unk = read_u8(reader);
        let _unk = read_u8(reader);
        let _unk = read_u8(reader);

        // Even mor eruntime data?
        let _unk = read_u32(reader);
        let _unk = read_u32(reader);
        let maybe_launch_time1 = read_fixed(reader);
        let maybe_launch_time2 = read_fixed(reader);

        let model_name = read_string_with_size(reader, 16);
        let _unk = read_u32(reader);
        let fade_time = read_fixed(reader);
        // panic!(
        //     "launch time: {} or {} or {}",
        //     maybe_launch_time1, maybe_launch_time2, model_name
        // );

        // let motion = read_u32(reader);
        // let num_particles = read_u32(reader);
        // let particle_size = read_single(reader);
        // let bitmap_name = read_string_with_size(reader, 16);

        let consumed = 64 + 24 + 12 + 12 + 4 + 44 + 32 + 32 + 48 + 8;

        let _rem = read_bytes(reader, (len - consumed) as usize);
        PropParticleGroup {
            render_type,
            motion_type,
            animation_type,
            num,
            velocity,
            gravity,
            r,
            g,
            b,
            a,
            spin,
            is_active,
            is_worldspace,
            size,
            scale_vel,
            prev_loc,
            bbox_min,
            bbox_max,
            radius,
            launch_time: maybe_launch_time2.max(maybe_launch_time1),
            fade_time,
            model_name,
        }
    }
}
