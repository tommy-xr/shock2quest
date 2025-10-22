use std::{
    cell::RefCell,
    collections::HashMap,
    io::{Read, Seek, SeekFrom},
    rc::Rc,
    time::Duration,
};

use cgmath::prelude::*;
use cgmath::{Point3, Vector2, Vector3};
use collision::{Aabb, Aabb3};
use engine::{
    assets::asset_cache::AssetCache,
    scene::{SceneObject, VertexPositionTextureSkinnedNormal},
    texture::{AnimatedTexture, TextureTrait},
};
use tracing::trace;

use crate::{
    importers::TEXTURE_IMPORTER,
    motion::JointId,
    ss2_bin_header::SystemShock2BinHeader,
    ss2_common::{
        read_bytes, read_i16, read_i32, read_i8, read_packed_normal, read_point3, read_single,
        read_string_with_size, read_u16, read_u32, read_u8, read_vec2, read_vec3,
    },
    ss2_skeleton::Skeleton,
    util::load_multiple_textures_for_model,
    SCALE_FACTOR,
};

#[derive(Clone)]
pub struct SystemShock2AIMesh {
    // pub header: BinHeader,
    pub materials: Vec<AIMaterial>,
    pub uvs: Vec<AIUv>,
    pub vertices: Vec<Point3<f32>>,
    pub normals: Vec<Vector3<f32>>,
    pub triangles: Vec<AITriangle>,

    pub joints: Vec<AIJointInfo>,
    pub joint_map: Vec<AIJointMapEntry>,
    pub weights: Vec<f32>,
}

pub struct AIMeshHeader {
    offset_joint_remap: u32,
    offset_mappers: u32,
    offset_mats: u32,
    offset_joints: u32,
    offset_normals: u32,
    offset_triangles: u32,
    offset_vertices: u32,
    offset_uvs: u32,
    offset_weights: u32,

    num_joints: u8,
    num_mappers: u8,
    num_mats: u8,
    num_triangles: u16,
    num_vertices: u16,
    num_weights: u16,
}

pub fn read_header<T: Read + Seek>(reader: &mut T) -> AIMeshHeader {
    let _zero0 = read_u32(reader); // radius
    let _zero1 = read_u32(reader); // flags
    let _zero2 = read_u32(reader); // app data

    let _unk1 = read_u8(reader); // layout
                                 // segs: https://github.com/infernuslord/DarkEngine/blob/c8542d03825bc650bfd6944dc03da5b793c92c19/tech/libsrc/mm/mms.h#L28
                                 // mm_segment_list:
                                 // - https://github.com/infernuslord/DarkEngine/blob/c8542d03825bc650bfd6944dc03da5b793c92c19/tech/libsrc/mp/mpupdate.c
    let num_mappers = read_u8(reader);
    let num_mats = read_u8(reader);
    let num_joints = read_u8(reader);

    let num_triangles = read_u16(reader);
    let num_vertices = read_u16(reader);
    let num_weights = read_u16(reader);
    let _unk = read_u16(reader);

    let offset_joint_remap = read_u32(reader);
    let offset_mappers = read_u32(reader);
    let offset_mats = read_u32(reader);

    let offset_joints = read_u32(reader);
    let offset_triangles = read_u32(reader);
    let offset_normals = read_u32(reader);

    let offset_vertices = read_u32(reader);
    let offset_uvs = read_u32(reader);
    let offset_weights = read_u32(reader);

    AIMeshHeader {
        offset_joint_remap,
        offset_joints,
        offset_mappers,
        offset_mats,
        offset_normals,
        offset_triangles,
        offset_vertices,
        offset_uvs,
        offset_weights,

        num_joints,
        num_mappers,
        num_mats,
        num_triangles,
        num_vertices,
        num_weights,
    }
}

pub fn read<T: Read + Seek>(
    reader: &mut T,
    common_header: &SystemShock2BinHeader,
) -> SystemShock2AIMesh {
    let header = read_header(reader);

    let _ = reader.seek(SeekFrom::Start(header.offset_joint_remap as u64));

    let _joints_in = read_bytes(reader, header.num_joints as usize);
    let _joints_out = read_bytes(reader, header.num_joints as usize);

    // Read joint map
    let mut joint_map = Vec::new();
    let _ = reader.seek(SeekFrom::Start(header.offset_mappers as u64));

    for _ in 0..header.num_mappers {
        let joint_map_entry = read_joint_map_entry(reader);
        joint_map.push(joint_map_entry);
    }

    // Read materials
    let mut materials = Vec::new();
    let _ = reader.seek(SeekFrom::Start(header.offset_mats as u64));

    for _ in 0..header.num_mats {
        let material = read_material(reader, common_header.version);
        materials.push(material);
    }

    // Read joints
    let mut joints = Vec::new();
    let _ = reader.seek(SeekFrom::Start(header.offset_joints as u64));
    for _ in 0..header.num_joints {
        let joint = read_joint(reader);
        joints.push(joint);
    }

    // Read triangles
    let mut triangles = Vec::new();
    let _ = reader.seek(SeekFrom::Start(header.offset_triangles as u64));

    for _ in 0..header.num_triangles {
        let triangle = read_triangle(reader);
        triangles.push(triangle);
    }

    // Read vertices
    let mut vertices = Vec::new();
    let _ = reader.seek(SeekFrom::Start(header.offset_vertices as u64));

    for _ in 0..header.num_vertices {
        let vert = read_point3(reader) / SCALE_FACTOR;
        vertices.push(vert);
    }

    // Read uvs
    let mut uvs = Vec::new();
    let _ = reader.seek(SeekFrom::Start(header.offset_uvs as u64));

    for _ in 0..header.num_vertices {
        let uv = read_uv(reader);
        uvs.push(uv);
    }

    // Read normals
    let mut normals = Vec::new();
    let _ = reader.seek(SeekFrom::Start(header.offset_normals as u64));

    for _ in 0..header.num_triangles {
        let normal = read_vec3(reader);
        normals.push(normal);
    }

    // Read weights
    let mut weights = Vec::new();
    let _ = reader.seek(SeekFrom::Start(header.offset_weights as u64));
    for _ in 0..header.num_weights {
        let weight = read_single(reader);
        weights.push(weight);
    }

    SystemShock2AIMesh {
        joint_map,
        joints,
        materials,
        triangles,
        uvs,
        vertices,
        normals,
        weights,
    }
}

#[derive(Debug, Clone)]
pub struct AIJointMapEntry {
    pub joint: i8,
    #[allow(dead_code)]
    num_of_material_segments: i8,
    #[allow(dead_code)]
    map_start: i8,
    // en1: i8, // what is this for?
    // jother: i8,
    // en2: i8,
    // rotation: Vector3<f32>,
}

pub fn read_joint_map_entry<T: Read + Seek>(reader: &mut T) -> AIJointMapEntry {
    // Not convinced this is a 100% accurate, should revisit?
    let _bbox = read_i32(reader);
    let joint = read_i8(reader);
    let num_of_material_segments = read_i8(reader);
    let map_start = read_i8(reader);
    let _en2 = read_i8(reader);
    let _rotation = read_vec3(reader);

    AIJointMapEntry {
        joint,
        num_of_material_segments,
        map_start,
        // en1,
        // jother,
        // en2,
        // rotation,
    }
}

#[derive(Debug, Clone)]
pub struct AIMaterial {
    name: String,

    #[allow(dead_code)]
    dw_caps: u32,
    #[allow(dead_code)]
    transparency: f32,
    #[allow(dead_code)]
    illumination: f32,
    #[allow(dead_code)]
    dw_for_rent: u32,

    #[allow(dead_code)]
    handle: u32,
    #[allow(dead_code)]
    uv: f32,
    #[allow(dead_code)]
    material_type: u8,
    #[allow(dead_code)]
    smatsegs: u8,
    #[allow(dead_code)]
    map_start: u8,
    #[allow(dead_code)]
    flags: u8,

    polygons: u16,
    polygon_start: u16,
    #[allow(dead_code)]
    vertices: u16,
    #[allow(dead_code)]
    vertices_start: u16,
    #[allow(dead_code)]
    weight_start: u16,
}

pub fn read_material<T: Read + Seek>(reader: &mut T, version: u32) -> AIMaterial {
    let name = read_string_with_size(reader, 16);
    let mut dw_caps = 0;
    let mut transparency = 0.0;
    let mut illumination = 0.0;
    let mut dw_for_rent = 0;

    if version > 1 {
        dw_caps = read_u32(reader);
        transparency = read_single(reader);
        illumination = read_single(reader);
        dw_for_rent = read_u32(reader)
    }

    let handle = read_u32(reader);
    let uv = read_single(reader);
    let material_type = read_u8(reader);
    let smatsegs = read_u8(reader);
    let map_start = read_u8(reader);
    let flags = read_u8(reader);

    let polygons = read_u16(reader);
    let polygon_start = read_u16(reader);

    let vertices = read_u16(reader);
    let vertices_start = read_u16(reader);

    let weight_start = read_u16(reader);

    let _pad = read_u16(reader);

    AIMaterial {
        name,
        dw_caps,
        transparency,
        illumination,
        dw_for_rent,

        handle,
        uv,
        material_type,
        smatsegs,
        map_start,
        flags,
        polygons,
        polygon_start,

        vertices,
        vertices_start,

        weight_start,
    }
}

#[derive(Debug, Clone)]
pub struct AIJointInfo {
    #[allow(dead_code)]
    num_polys: i16,
    #[allow(dead_code)]
    start_poly: i16,
    num_vertices: i16,
    start_vertex: i16,
    #[allow(dead_code)]
    weight_index: u16,
    #[allow(dead_code)]
    flag: i16,
    mapper_id: i16,
}

pub fn read_joint<T: Read + Seek>(reader: &mut T) -> AIJointInfo {
    let num_polys = read_i16(reader);
    let start_poly = read_i16(reader);
    let num_vertices = read_i16(reader);
    let start_vertex = read_i16(reader);
    let weight = read_u16(reader);
    let _pad = read_u16(reader);
    let flag = read_i16(reader);
    let mapper_id = read_i16(reader);

    AIJointInfo {
        num_polys,
        start_poly,
        num_vertices,
        start_vertex,
        weight_index: weight,
        flag,
        mapper_id,
    }
}

#[derive(Debug, Clone)]
pub struct AITriangle {
    vert_index0: u16,
    vert_index1: u16,
    vert_index2: u16,
    #[allow(dead_code)]
    material_id: u16,
    #[allow(dead_code)]
    plane_coefficient: f32,
    #[allow(dead_code)]
    normal_index: u16,
    #[allow(dead_code)]
    flags: u16,
}

pub fn read_triangle<T: Read + Seek>(reader: &mut T) -> AITriangle {
    let vert_index0 = read_u16(reader);
    let vert_index1 = read_u16(reader);
    let vert_index2 = read_u16(reader);
    let material_id = read_u16(reader);
    let plane_coefficient = read_single(reader);
    let normal_index = read_u16(reader);
    let flags = read_u16(reader);
    AITriangle {
        vert_index0,
        vert_index1,
        vert_index2,
        material_id,
        plane_coefficient,
        normal_index,
        flags,
    }
}

#[derive(Clone, Debug)]
pub struct AIUv {
    uv: Vector2<f32>,
    normal: Vector3<f32>,
}

pub fn read_uv<T: Read + Seek>(reader: &mut T) -> AIUv {
    let uv = read_vec2(reader);

    // Read packed normal from SystemShock2VR implementation
    // Reference: https://github.com/Kernvirus/SystemShock2VR/blob/5f0f7d054e79c2e36d9661f4ca62ab95ae69de0b/Assets/Scripts/Editor/DarkEngine/DarkDataConverter.cs#L12
    let packed_normal = read_u32(reader);
    let normal = read_packed_normal(packed_normal);

    AIUv { uv, normal }
}

// Converter
pub fn to_scene_objects(
    mesh: &SystemShock2AIMesh,
    skeleton: &Skeleton,
    asset_cache: &mut AssetCache,
) -> (Vec<SceneObject>, HashMap<u32, Aabb3<f32>>) {
    let (material_to_vertices, hitboxes) = to_vertices(mesh, skeleton);

    let mut scene_objects = Vec::new();
    for (material_name, vertices) in material_to_vertices {
        let geometry: Rc<Box<dyn engine::scene::Geometry>> =
            Rc::new(Box::new(engine::scene::mesh::create(vertices)));

        let texture = asset_cache.get(&TEXTURE_IMPORTER, &material_name).clone();
        let diffuse_texture: Rc<dyn TextureTrait> = {
            let mut animation_frames =
                load_multiple_textures_for_model(asset_cache, &material_name);
            if !animation_frames.is_empty() {
                animation_frames.insert(0, texture.clone());
                Rc::new(AnimatedTexture::new(
                    animation_frames,
                    Duration::from_millis(200),
                ))
            } else {
                texture
            }
        };

        let material = RefCell::new(engine::scene::SkinnedMaterial::create(
            diffuse_texture,
            0.0,
            0.0,
        ));

        let mut scene_object = engine::scene::scene_object::SceneObject::create(material, geometry);
        let skinning_data = skeleton.get_transforms();
        scene_object.set_skinning_data(skinning_data);
        scene_objects.push(scene_object);
    }

    trace!("ai_mesh produced {} scene objects", scene_objects.len());

    (scene_objects, hitboxes)
}

pub fn to_vertices(
    mesh: &SystemShock2AIMesh,
    skeleton: &Skeleton,
) -> (
    Vec<(String, Vec<VertexPositionTextureSkinnedNormal>)>,
    HashMap<u32, Aabb3<f32>>,
) {
    let materials = &mesh.materials;
    let triangles = &mesh.triangles;
    let uvs = &mesh.uvs;
    let vertices = &mesh.vertices;
    let normals = &mesh.normals;
    let joints = &mesh.joints;
    let joint_map = &mesh.joint_map;
    let weights = &mesh.weights;

    // vertex index -> influences
    let mut vertex_influences: HashMap<u16, Vec<(JointId, f32)>> = HashMap::new();

    for joint in joints {
        if joint.mapper_id < 0 {
            continue;
        }

        let mapper_index = joint.mapper_id as usize;
        if mapper_index >= joint_map.len() {
            continue;
        }

        let raw_child_joint = joint_map[mapper_index].joint;
        if raw_child_joint < 0 {
            continue;
        }

        let child_joint = raw_child_joint as JointId;
        let parent_joint = skeleton.parent_of(child_joint).unwrap_or(child_joint);

        let start_vertex = joint.start_vertex.max(0) as usize;
        let count = joint.num_vertices.max(0) as usize;
        let weight_start = joint.weight_index as usize;

        for i in 0..count {
            let vertex_idx = start_vertex + i;
            let weight_idx = weight_start + i;
            let raw_weight = weights
                .get(weight_idx)
                .copied()
                .unwrap_or(0.0)
                .clamp(0.0, 1.0);

            let parent_weight = if parent_joint == child_joint {
                1.0
            } else {
                (1.0 - raw_weight).max(0.0)
            };
            let child_weight = if parent_joint == child_joint {
                0.0
            } else {
                raw_weight
            };

            let entry = vertex_influences.entry(vertex_idx as u16).or_default();

            accumulate_influence(entry, parent_joint, parent_weight);
            accumulate_influence(entry, child_joint, child_weight);
        }
    }

    let mut material_to_verts = Vec::new();
    let mut joint_to_hitbox = HashMap::new();

    for material in materials {
        let name = &material.name;
        let mut verts = Vec::new();
        for tri_index in material.polygon_start..(material.polygon_start + material.polygons) {
            let tri = &triangles[tri_index as usize];
            let v0 = vertices[tri.vert_index0 as usize];
            let v1 = vertices[tri.vert_index1 as usize];
            let v2 = vertices[tri.vert_index2 as usize];

            let (bone_indices0, bone_weights0, dominant_joint0) =
                build_skinning_for_vertex(tri.vert_index0, &vertex_influences);
            let (bone_indices1, bone_weights1, dominant_joint1) =
                build_skinning_for_vertex(tri.vert_index1, &vertex_influences);
            let (bone_indices2, bone_weights2, dominant_joint2) =
                build_skinning_for_vertex(tri.vert_index2, &vertex_influences);

            add_vertex_to_hitbox(&mut joint_to_hitbox, dominant_joint0, v0);
            add_vertex_to_hitbox(&mut joint_to_hitbox, dominant_joint1, v1);
            add_vertex_to_hitbox(&mut joint_to_hitbox, dominant_joint2, v2);

            let uv0 = uvs[tri.vert_index0 as usize].uv;
            let uv1 = uvs[tri.vert_index1 as usize].uv;
            let uv2 = uvs[tri.vert_index2 as usize].uv;

            // Use per-vertex normals from packed UV data instead of triangle normals
            let normal0 = uvs[tri.vert_index0 as usize].normal; // Coordinate transform already applied
            let normal1 = uvs[tri.vert_index1 as usize].normal;
            let normal2 = uvs[tri.vert_index2 as usize].normal;

            verts.push(build_vertex(v0, uv0, normal0, bone_indices0, bone_weights0));
            verts.push(build_vertex(v1, uv1, normal1, bone_indices1, bone_weights1));
            verts.push(build_vertex(v2, uv2, normal2, bone_indices2, bone_weights2));
        }
        material_to_verts.push((name.to_owned(), verts));
    }

    (material_to_verts, joint_to_hitbox)
}

fn accumulate_influence(entry: &mut Vec<(JointId, f32)>, joint: JointId, weight: f32) {
    if weight <= 0.0 {
        return;
    }

    if let Some(existing) = entry.iter_mut().find(|(j, _)| *j == joint) {
        existing.1 += weight;
    } else {
        entry.push((joint, weight));
    }
}

fn add_vertex_to_hitbox(
    joint_to_hitbox: &mut HashMap<u32, Aabb3<f32>>,
    joint: u32,
    point: Point3<f32>,
) {
    let entry = joint_to_hitbox.entry(joint).or_insert(Aabb3 {
        min: point,
        max: point,
    });
    *entry = entry.grow(point);
}

fn build_skinning_for_vertex(
    vertex_index: u16,
    vertex_influences: &HashMap<u16, Vec<(JointId, f32)>>,
) -> ([u32; 4], [f32; 4], JointId) {
    let mut influences = vertex_influences
        .get(&vertex_index)
        .cloned()
        .unwrap_or_else(|| vec![(0, 1.0)]);

    // Combine duplicate joints if any slipped through cloning
    let mut combined: HashMap<JointId, f32> = HashMap::new();
    for (joint, weight) in influences.drain(..) {
        let entry = combined.entry(joint).or_insert(0.0);
        *entry += weight;
    }

    let mut pairs: Vec<(JointId, f32)> = combined.into_iter().collect();
    pairs.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    if pairs.is_empty() {
        pairs.push((0, 1.0));
    }

    let dominant_joint = pairs[0].0;

    let mut bone_indices = [0u32; 4];
    let mut bone_weights = [0.0f32; 4];

    let mut total_weight = 0.0;
    for (slot, (joint, weight)) in pairs.into_iter().take(4).enumerate() {
        bone_indices[slot] = joint;
        bone_weights[slot] = weight;
        total_weight += weight;
    }

    if total_weight > 0.0 {
        for weight in &mut bone_weights {
            *weight /= total_weight;
        }
    } else {
        bone_weights[0] = 1.0;
    }

    (bone_indices, bone_weights, dominant_joint as JointId)
}

fn build_vertex(
    vec: Point3<f32>,
    uv: Vector2<f32>,
    normal: Vector3<f32>,
    bone_indices: [u32; 4],
    bone_weights: [f32; 4],
) -> VertexPositionTextureSkinnedNormal {
    VertexPositionTextureSkinnedNormal {
        position: vec.to_vec(),
        uv,
        bone_indices,
        bone_weights,
        normal,
    }
}
