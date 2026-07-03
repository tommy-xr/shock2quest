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
        // The chunk is the engine's particle-group struct written verbatim,
        // including runtime-only fields (vtable/list/heap pointers) that are
        // garbage in the file. Two struct versions ship: mission files write a
        // 324-byte layout, the gamesys writes a newer 380-byte layout that
        // inserts 8 extra bytes after `gravity` (and appends a longer tail).
        // Offsets below name the 324-byte layout; the 380-byte one is +8 from
        // the color field onward. Verified against the shipped data by
        // anchors: launch-info heap pointers, attach-object ids at the tail
        // (e.g. a steam puff attached to its emitter), sane sizes/spins, and
        // identity rotation matrices.
        let _particle_class = read_bytes(reader, 36); // per-class fn pointers (runtime)
        let _obj_id = read_u32(reader);

        let render_type = read_u32(reader); // 40
        let motion_type = read_u32(reader); // 44
        let animation_type = read_u32(reader); // 48
        let _placeholder_enums = read_bytes(reader, 8); // 52
        let num = read_u32(reader); // 60

        let _particle_list = read_bytes(reader, 24); // 64: per-particle arrays (runtime)

        let velocity = read_vec3(reader); // 88
        let gravity = read_vec3(reader); // 100

        if len >= 380 {
            // The newer (gamesys) layout only: 8 unknown bytes (zero in the
            // shipped data).
            let _unk = read_bytes(reader, 8);
        }

        // 112: global color. `r` is an index into the game master palette (the
        // renderer resolves it); `a` is the global alpha.
        let r = read_u8(reader);
        let g = read_u8(reader);
        let b = read_u8(reader);
        let a = read_u8(reader);

        let _always_simulate = read_bool_u8(reader); // 116
        let _always_simulate_group = read_bool_u8(reader);
        let _cell_sort = read_bool_u8(reader);
        let _zsort = read_bool_u8(reader);

        let _terrain_collide = read_bool_u8(reader); // 120
        let _accelerate_cell = read_bool_u8(reader);
        let _ignore_attach_refs = read_bool_u8(reader);
        let _pad = read_bool_u8(reader);

        let _launch_info_ptr = read_u32(reader); // 124 (runtime heap pointer)

        let spin = read_vec3(reader); // 128
        let _pulse_period_ms = read_u32(reader); // 140
        let _pulse_percentage = read_single(reader); // 144
        let _fixed_scale = read_single(reader); // 148

        let _pre_launch = read_bool_u8(reader); // 152
        let _spin_group = read_bool_u8(reader);
        let _tiny_alpha = read_bool_u8(reader);
        let _tiny_dropout = read_bool_u8(reader);

        let _shared_list = read_bool_u8(reader); // 156
        let is_worldspace = read_bool_u8(reader);
        let _launching = read_bool_u8(reader);
        let is_active = read_bool_u8(reader);

        let _ms_offset = read_u32(reader); // 160
        let size = read_single(reader); // 164
        let _reserved = read_bytes(reader, 8); // 168

        let prev_loc = read_vec3(reader); // 176
        let scale_vel = read_single(reader); // 188

        let _render_datum = read_u32(reader); // 192
        let bbox_min = read_vec3(reader); // 196
        let bbox_max = read_vec3(reader); // 208
        let radius = read_single(reader); // 220
        let _cur_scale = read_single(reader); // 224
        let _points_ptrs = read_bytes(reader, 8); // 228 (runtime pointers)
        let _derived_flags = read_bytes(reader, 4); // 236
        let _list_length = read_u32(reader); // 240
        let _delete_count = read_u32(reader); // 244
        let _next_launch = read_fixed(reader); // 248
        // 252: time between particle launches (fix seconds).
        let launch_period = read_fixed(reader);
        let model_name = read_string_with_size(reader, 16); // 256 (bitmap name)
        let _model_num = read_u32(reader); // 272
        let fade_time = read_fixed(reader); // 276

        // Remainder: rotation matrix, sim bookkeeping, attach object, and
        // (380-byte entries only) trailing fields.
        let consumed = if len >= 380 { 288u32 } else { 280u32 };
        let _rem = read_bytes(reader, len.saturating_sub(consumed) as usize);

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
            launch_time: launch_period,
            fade_time,
            model_name,
        }
    }
}
