use byteorder::ReadBytesExt;
use cgmath::point3;
use cgmath::vec2;
use cgmath::vec3;
use cgmath::Decomposed;
use cgmath::Deg;
use cgmath::InnerSpace;

use cgmath::Point3;
use cgmath::Quaternion;
use cgmath::Rotation;

use cgmath::Vector2;
use cgmath::Vector3;
use collision::Plane;
use std::ffi::CString;
use std::io;
use std::time::Duration;

pub const CHUNK_HEADER_SIZE: u32 = 24;

#[derive(Debug, Clone)]
pub struct LevelChunk {
    pub offset: u32, // Offset of the chunk, in bytes
    pub length: u32, // Length of chunk, in bytes
}

pub fn read_u16_angle<T: io::Read>(reader: &mut T) -> Deg<f32> {
    let denom = 0x8000 as f32;
    let v = read_u16(reader) as f32;
    Deg(v * 180.0 / denom)
}

pub fn read_u16_vec3<T: io::Read>(reader: &mut T) -> Vector3<Deg<f32>> {
    let _denom = 0x8000 as f32;

    let x = read_u16_angle(reader) * -1.0;
    let z = read_u16_angle(reader);
    let y = read_u16_angle(reader);

    vec3(x, y, z)
}

pub fn read_bool_u8<T: io::Read>(reader: &mut T) -> bool {
    let b = read_u8(reader);
    b != 0
}

pub fn read_bool<T: io::Read>(reader: &mut T) -> bool {
    let b = reader.read_u32::<byteorder::LittleEndian>().unwrap();
    b != 0
}

pub fn read_vec3<T: io::Read>(reader: &mut T) -> Vector3<f32> {
    let neg_x = reader.read_f32::<byteorder::LittleEndian>().unwrap();
    let z = reader.read_f32::<byteorder::LittleEndian>().unwrap();
    let y = reader.read_f32::<byteorder::LittleEndian>().unwrap();
    vec3(-neg_x, y, z)
}

pub fn read_plane<T: io::Read>(reader: &mut T) -> Plane<f32> {
    let vec = read_vec3(reader).normalize();
    let d = read_single(reader);
    Plane::new(vec, d)
}

pub fn read_quat<T: io::Read>(reader: &mut T) -> Quaternion<f32> {
    let w = reader.read_f32::<byteorder::LittleEndian>().unwrap();
    let neg_x = reader.read_f32::<byteorder::LittleEndian>().unwrap();
    let z = reader.read_f32::<byteorder::LittleEndian>().unwrap();
    let y = reader.read_f32::<byteorder::LittleEndian>().unwrap();
    let q = Quaternion {
        v: vec3(-neg_x, y, z),
        s: w,
    };
    q.invert()
}

pub fn read_point3<T: io::Read>(reader: &mut T) -> Point3<f32> {
    let neg_x = read_single(reader);
    let z = read_single(reader);
    let y = read_single(reader);
    point3(-neg_x, y, z)
}

pub fn read_vec2<T: io::Read>(reader: &mut T) -> Vector2<f32> {
    let x = read_single(reader);
    let y = read_single(reader);
    vec2(x, y)
}

pub fn read_packed_normal(packed_vector: u32) -> Vector3<f32> {
    // Unpack using OpenDarkEngine's exact algorithm
    let raw_x = (((packed_vector & 0x0FFC) << 4) as i16) as f32 / 16384.0;
    let raw_y = (((packed_vector >> 6) & 0x0FFC0) as i16) as f32 / 16384.0;
    let raw_z = (((packed_vector >> 16) & 0x0FFC0) as i16) as f32 / 16384.0;

    Vector3::new(-raw_x, raw_z, raw_y)
}

pub fn read_duration<T: io::Read>(reader: &mut T) -> Duration {
    let dur_as_secs_f32 = read_single(reader);
    Duration::from_secs_f32(dur_as_secs_f32)
}

pub fn read_single<T: io::Read>(reader: &mut T) -> f32 {
    let v = reader.read_f32::<byteorder::LittleEndian>().unwrap();

    if v.is_nan() {
        0.0
    } else {
        v
    }
}
pub fn read_u8<T: io::Read>(reader: &mut T) -> u8 {
    reader.read_u8().unwrap()
}

pub fn read_i8<T: io::Read>(reader: &mut T) -> i8 {
    reader.read_i8().unwrap()
}

pub fn read_char<T: io::Read>(reader: &mut T) -> char {
    reader.read_u8().unwrap() as char
}

pub fn read_u16<T: io::Read>(reader: &mut T) -> u16 {
    reader.read_u16::<byteorder::LittleEndian>().unwrap()
}

pub fn read_i16<T: io::Read>(reader: &mut T) -> i16 {
    reader.read_i16::<byteorder::LittleEndian>().unwrap()
}

pub fn read_array_u16<T: io::Read>(reader: &mut T, count: u32) -> Vec<u16> {
    let mut ret = Vec::new();
    for _idx in 0..count {
        ret.push(reader.read_u16::<byteorder::LittleEndian>().unwrap());
    }
    ret
}

pub fn read_array_u32<T: io::Read>(reader: &mut T, count: u32) -> Vec<u32> {
    let mut ret = Vec::new();
    for _idx in 0..count {
        ret.push(reader.read_u32::<byteorder::LittleEndian>().unwrap());
    }
    ret
}

pub fn read_fixed<T: io::Read>(reader: &mut T) -> f32 {
    let num = read_u32(reader) as f64;
    (num / 65536.0f64) as f32
}

pub fn read_u32<T: io::Read>(reader: &mut T) -> u32 {
    reader.read_u32::<byteorder::LittleEndian>().unwrap()
}

pub fn read_u64<T: io::Read>(reader: &mut T) -> u64 {
    reader.read_u64::<byteorder::LittleEndian>().unwrap()
}

pub fn read_i32<T: io::Read>(reader: &mut T) -> i32 {
    reader.read_i32::<byteorder::LittleEndian>().unwrap()
}

pub fn read_string_with_size<T: io::Read>(reader: &mut T, size: usize) -> String {
    let mut c_str = vec![0; size];
    reader.read_exact(&mut c_str).unwrap();

    let c_str_sub = match c_str.iter().position(|v| *v == 0u8) {
        None => c_str.to_vec(),
        Some(idx) => c_str[..idx].to_vec(),
    };

    let name = unsafe { CString::from_vec_unchecked(c_str_sub) };
    name.to_str().unwrap().to_owned()
}

pub fn read_bytes<T: io::Read>(reader: &mut T, size: usize) -> Vec<u8> {
    let mut vec = vec![0; size];
    reader.read_exact(&mut vec).unwrap();
    vec
}

pub fn read_matrix<T: io::Read>(reader: &mut T) -> Decomposed<Vector3<f32>, Quaternion<f32>> {
    let mut sum: f32 = 0.0;
    let mut vals: [f32; 12] = [0.0; 12];
    for i in 0..12 {
        vals[i] = read_single(reader);
        sum += vals[i];
    }
    if sum == 0.0 {
        Decomposed {
            scale: 1.0,
            rot: Quaternion {
                v: vec3(0.0, 0.0, 0.0),
                s: 1.0,
            },
            disp: vec3(0.0, 0.0, 0.0),
        }
    } else {
        let _scale = vec3(vals[0], vals[4], vals[8]);
        let pre_translation = vec3(vals[9], vals[10], vals[11]);

        let rot_matrix = cgmath::Matrix3::<f32>::new(
            vals[0], vals[3], vals[6], vals[1], vals[4], vals[7], vals[2], vals[5], vals[8],
        );

        let pre_quat: Quaternion<f32> = rot_matrix.into();
        // Matrix4::new(
        //     vals[0], vals[3], vals[6], vals[9], vals[1], vals[4], vals[7], vals[10], vals[2],
        //     vals[5], vals[8], vals[11], 0.0, 0.0, 0.0, 1.0,
        // );
        Decomposed {
            scale: 1.0,
            rot: Quaternion {
                v: vec3(-pre_quat.v.x, pre_quat.v.z, pre_quat.v.y),
                s: pre_quat.s,
            }
            .invert(),
            disp: vec3(-pre_translation.x, pre_translation.z, pre_translation.y),
        }
    }
}
