use std::{
    cell::RefCell,
    collections::HashMap,
    io::{prelude::*, SeekFrom},
    rc::Rc,
    time::Duration,
};

use cgmath::{point3, prelude::*, vec3, Point3};
use cgmath::{vec4, Matrix4, Vector2, Vector3, Vector4};
use collision::Aabb3;
use engine::{
    assets::asset_cache::AssetCache,
    scene::{SceneObject, VertexPositionTexture, VertexPositionTextureSkinned},
    texture::{AnimatedTexture, TextureTrait},
};
use num_derive::{FromPrimitive, ToPrimitive};
use num_traits::FromPrimitive;

use crate::{
    importers::TEXTURE_IMPORTER,
    ss2_bin_header::SystemShock2BinHeader,
    ss2_common::{
        self, read_array_u16, read_bytes, read_i16, read_i32, read_matrix, read_point3,
        read_single, read_string_with_size, read_u16, read_u32, read_u8, read_vec3,
    },
    ss2_skeleton::{Bone, Skeleton},
    util::load_multiple_textures_for_model,
    SCALE_FACTOR,
};

#[derive(FromPrimitive, ToPrimitive, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum VhotType {
    Unknown = 0,
    LightSource = 1,
    Anchor = 2,
    Particle1 = 3,
    Particle2 = 4,
    Particle3 = 5,
    Particle4 = 6,
    Particle5 = 7,
    LightSource2 = 8,
}

#[derive(Debug, Clone)]
pub struct Vhot {
    pub vhot_type: VhotType,
    pub point: Point3<f32>,
}

impl Vhot {
    pub fn read<T: Read + Seek>(reader: &mut T) -> Vhot {
        let vhot_type_num = read_u32(reader);
        let vhot_type = VhotType::from_u32(vhot_type_num).unwrap();

        let point = read_point3(reader) / SCALE_FACTOR;
        Vhot { vhot_type, point }
    }
}

pub struct SystemShock2ObjectMesh {
    pub header: ObjBinHeader,
    pub version: u32,

    pub bounding_box: Aabb3<f32>,
    pub materials: Vec<SystemShock2MeshMaterial>,
    pub uvs: Vec<Vector2<f32>>,
    pub vertices: Vec<Vector3<f32>>,
    pub vhots: Vec<Vhot>,
    pub polygons: Vec<SystemShock2ObjectPolygon>,
    pub sub_objects: Vec<SubObjectHeader>,
}

pub fn read<T: Read + Seek>(
    reader: &mut T,
    common_header: &SystemShock2BinHeader,
) -> SystemShock2ObjectMesh {
    let header = read_header(reader, common_header);

    let vertices = read_vertices(&header, reader);

    let polygons: Vec<SystemShock2ObjectPolygon> =
        read_polygons(&header, reader, common_header.version);

    let uvs = read_uvs(&header, reader);

    let mut materials = read_materials(&header, reader);

    read_extended_materials(&header, &mut materials, reader, common_header.version);

    let objs = read_sub_objects(&header, reader);

    let vhots = read_vhots(&header, reader);

    let bounding_box = Aabb3::new(header.bbox_min, header.bbox_max);

    SystemShock2ObjectMesh {
        bounding_box,
        materials,
        vertices,
        polygons,
        header,
        uvs,
        vhots,
        sub_objects: objs,
        version: common_header.version,
    }
}

// Converter
pub fn to_scene_objects(
    mesh: &SystemShock2ObjectMesh,
    asset_cache: &mut AssetCache,
) -> (Vec<SceneObject>, Skeleton) {
    let mut hash_to_material = HashMap::new();

    let material_len = mesh.materials.len();
    for idx in 0..material_len {
        let temp_material = &mesh.materials[idx];

        hash_to_material.insert(temp_material.slot_num as u16, temp_material.clone());
    }

    let slot_to_vertices = to_vertices(mesh);

    let vertices = slot_to_vertices
        .into_iter()
        .collect::<Vec<(u16, Vec<VertexPositionTextureSkinned>)>>();

    let mut bones = Vec::new();
    build_skeleton_for_obj_mesh(&mesh, 0, None, &mut bones);
    let is_skinned = bones.len() > 1;
    let skeleton = Skeleton::create_from_bones(bones);

    let mut mesh_objects = vertices
        .into_iter()
        .filter_map(|(slot, verts)| {
            if verts.is_empty() {
                return None;
            }

            let material = hash_to_material.get(&slot).unwrap();
            let mut tex_path = material.name.to_string();

            // HACK... for broken texture name
            if tex_path.to_ascii_lowercase().contains("soft12.pcx") {
                tex_path = "soft12 .pcx".to_owned();
            }

            let maybe_texture = asset_cache.get_opt(&TEXTURE_IMPORTER, &tex_path);

            if maybe_texture.is_none() {
                return None;
            }

            let texture = maybe_texture.unwrap();

            let geometry: Rc<Box<dyn engine::scene::Geometry>> = if is_skinned {
                Rc::new(Box::new(engine::scene::mesh::create(verts)))
            } else {
                // If not skinned, convert the vertices to non-skinned representation
                let simpler_vertices =
                    convert_skinned_vertices_to_static_vertices(&verts, &skeleton);
                Rc::new(Box::new(engine::scene::mesh::create(simpler_vertices)))
            };

            let diffuse_texture: Rc<dyn TextureTrait> = {
                let mut animation_frames = load_multiple_textures_for_model(asset_cache, &tex_path);
                if !animation_frames.is_empty() {
                    animation_frames.insert(0, texture);
                    Rc::new(AnimatedTexture::new(
                        animation_frames,
                        Duration::from_millis(200),
                    ))
                } else {
                    texture
                }
            };

            let mut transparency = material.transparency;

            // HACK: Why isn't GLAS_S01" loading transparency??
            if tex_path.contains("GLAS_S01") {
                transparency = 0.8
            }

            let mat = if is_skinned {
                engine::scene::SkinnedMaterial::create(
                    diffuse_texture,
                    material.emissivity,
                    transparency,
                )
            } else {
                engine::scene::basic_material::create(
                    diffuse_texture,
                    material.emissivity,
                    transparency,
                )
            };

            let material = RefCell::new(mat);
            let mut so = engine::scene::scene_object::SceneObject::create(material, geometry);

            so.set_skinning_data(skeleton.get_transforms());

            Some(so)
        })
        .collect::<Vec<SceneObject>>();

    let vhots = &mesh.vhots;
    let mut vhot_objs = vhots
        .iter()
        .map(|vhot| {
            let geometry = engine::scene::cube::create();
            let material = RefCell::new(engine::scene::color_material::create(vec3(0.0, 0.0, 1.0)));
            let mut scene_obj = SceneObject::create(material, Rc::new(Box::new(geometry)));
            scene_obj.set_local_transform(
                Matrix4::from_translation(vhot.point.to_vec()) * Matrix4::from_scale(0.025),
            );
            scene_obj
        })
        .collect::<Vec<SceneObject>>();

    mesh_objects.append(&mut vhot_objs);
    (mesh_objects, skeleton)
}

// Data
#[derive(Debug, Clone)]
pub struct SystemShock2MeshMaterial {
    pub name: String,
    material_type: u8, // TODO: Add real type
    pub slot_num: u8,
    ipal_index: u32, // unused

    color: Vector4<f32>,
    handle: u32,
    uv_scale: f32,

    pub transparency: f32,
    pub emissivity: f32,
}

fn read_material<T: Read>(reader: &mut T) -> SystemShock2MeshMaterial {
    let name = ss2_common::read_string_with_size(reader, 16);
    let material_type = ss2_common::read_u8(reader);
    let slot_num = ss2_common::read_u8(reader);

    let (color, ipal_index, handle, uv_scale) = if material_type == 1
    /* MD_MAT_COLOR */
    {
        let r = ss2_common::read_u8(reader);
        let g = ss2_common::read_u8(reader);
        let b = ss2_common::read_u8(reader);
        let a = ss2_common::read_u8(reader);

        let color = vec4(
            r as f32 / 255.0,
            g as f32 / 255.0,
            b as f32 / 255.0,
            a as f32 / 255.0,
        );
        let ipal_index = ss2_common::read_u32(reader);
        (color, ipal_index, 0, 1.0)
    } else if material_type == 0
    /* MD_MAT_TMAP */
    {
        let handle = ss2_common::read_u32(reader);
        let uv_scale = ss2_common::read_single(reader);
        let color = vec4(1.0, 1.0, 1.0, 1.0);
        (color, 0, handle, uv_scale)
    } else {
        panic!("Unknown material type: {material_type}");
    };

    SystemShock2MeshMaterial {
        name,
        material_type,
        slot_num,
        ipal_index,
        color,
        handle,
        uv_scale,
        emissivity: 0.0,
        transparency: 0.0,
    }
}

pub fn read_materials<T: Read + Seek>(
    header: &ObjBinHeader,
    reader: &mut T,
) -> Vec<SystemShock2MeshMaterial> {
    reader
        .seek(SeekFrom::Start((header.offset_mats) as u64))
        .unwrap();

    let mut materials = Vec::new();
    let len = header.num_mats;

    for _idx in 0..len {
        let material = read_material(reader);
        materials.push(material);
    }

    materials
}

fn build_vertex(
    vec: Vector3<f32>,
    uv: Vector2<f32>,
    bone_idx: u32,
) -> VertexPositionTextureSkinned {
    VertexPositionTextureSkinned {
        position: vec,
        uv,
        bone_indices: [bone_idx, 0, 0, 0],
    }
}

pub fn to_vertices(
    mesh: &SystemShock2ObjectMesh,
) -> HashMap<u16, Vec<VertexPositionTextureSkinned>> {
    let polygons = &mesh.polygons;
    let uvs = &mesh.uvs;
    let vertices = &mesh.vertices;

    let mut hash_map = HashMap::new();

    for poly in polygons {
        let indices = &poly.vertex_indices;
        let uv_indices = &poly.uv_indices;
        let slot = poly.slot_index;

        let vec = Vec::new();
        hash_map.entry(slot).or_insert(vec);

        let verts = hash_map.get_mut(&slot).unwrap();

        let len = indices.len();
        let uv_len = uv_indices.len();
        if len > 1 && uv_len > 1 {
            for idx in 1..(len - 1) {
                let bone_idx = get_bone_index_for_point(mesh, indices[idx]);
                verts.push(build_vertex(
                    vertices[indices[idx] as usize],
                    uvs[uv_indices[idx] as usize],
                    bone_idx,
                ));
                verts.push(build_vertex(
                    vertices[indices[idx + 1] as usize],
                    uvs[uv_indices[idx + 1_usize] as usize],
                    bone_idx,
                ));
                verts.push(build_vertex(
                    vertices[indices[0] as usize],
                    uvs[uv_indices[0] as usize],
                    bone_idx,
                ));
            }
        }
    }

    hash_map
}

fn get_bone_index_for_point(header: &SystemShock2ObjectMesh, usize: u16) -> u32 {
    // TODO: Improve perf by caching, instead of O(N^2) iteration across sub objects
    let mut idx = 0;
    for so in &header.sub_objects {
        if usize >= so.point_start && usize < so.point_stop {
            return idx;
        }
        idx = idx + 1
    }

    0
}

fn build_skeleton_for_obj_mesh(
    header: &SystemShock2ObjectMesh,
    sub_object_idx: i32,
    current_parent: Option<u32>,
    bones: &mut Vec<Bone>,
) {
    if sub_object_idx == -1 {
        return;
    }

    if sub_object_idx as usize >= header.sub_objects.len() {
        return;
    }

    let sub_object = &header.sub_objects[sub_object_idx as usize];

    bones.push(Bone {
        joint_id: sub_object_idx as u32,
        local_transform: sub_object.transform,
        parent_id: current_parent,
    });

    // Add all children
    build_skeleton_for_obj_mesh(
        header,
        sub_object.child_sub_obj_idx as i32,
        Some(sub_object_idx as u32),
        bones,
    );

    // Add all peers
    build_skeleton_for_obj_mesh(
        header,
        sub_object.next_sub_obj_idx as i32,
        current_parent,
        bones,
    )
}

#[derive(Debug, Clone)]
pub struct SystemShock2ObjectPolygon {
    pub vertex_indices: Vec<u16>,
    pub uv_indices: Vec<u16>,
    pub slot_index: u16,
}

fn read_polygon<T: Read>(
    _header: &ObjBinHeader,
    reader: &mut T,
    version: u32,
) -> SystemShock2ObjectPolygon {
    let _index = ss2_common::read_u16(reader);
    let slot_index = ss2_common::read_u16(reader);

    let poly_type = ss2_common::read_u8(reader);
    let num_verts = ss2_common::read_u8(reader);

    // Plane info?
    let _norm = ss2_common::read_u16(reader);
    let _d = ss2_common::read_single(reader);

    // Read vert indices
    let vertex_indices = read_array_u16(reader, num_verts as u32);

    // Read normal indices
    let _normal_indices = read_array_u16(reader, num_verts as u32);

    // Read uv indices, maybe
    let mut uvs = vec![];
    if (poly_type & 3) == 3 {
        uvs = read_array_u16(reader, num_verts as u32);
    }

    if version == 4 {
        let _unknown = read_u8(reader);
    }

    SystemShock2ObjectPolygon {
        vertex_indices,
        uv_indices: uvs,
        slot_index,
    }
}

pub fn read_polygons<T: Read + Seek>(
    header: &ObjBinHeader,
    reader: &mut T,
    version: u32,
) -> Vec<SystemShock2ObjectPolygon> {
    let mut ret = Vec::new();

    reader
        .seek(SeekFrom::Start((header.offset_polygons) as u64))
        .unwrap();

    for _idx in 0..header.num_polygons {
        let polygon = read_polygon(header, reader, version);
        ret.push(polygon);
    }

    ret
}

pub fn read_vhots<T: Read + Seek>(header: &ObjBinHeader, reader: &mut T) -> Vec<Vhot> {
    let mut vhots = Vec::new();

    if header.num_vhots > 0 {
        reader
            .seek(SeekFrom::Start((header.offset_vhots) as u64))
            .unwrap();

        for _ in 0..header.num_vhots {
            vhots.push(Vhot::read(reader));
        }
    }
    vhots.sort_by(|a, b| a.vhot_type.cmp(&b.vhot_type));
    vhots
}

pub fn read_uvs<T: Read + Seek>(header: &ObjBinHeader, reader: &mut T) -> Vec<Vector2<f32>> {
    let mut uvs = Vec::new();

    let space = header.offset_vhots - header.offset_uvs;
    let num_uvs = space / (4 /* size of float */ * 2/* 2 floats in vector2 */);

    if num_uvs > 0 {
        reader
            .seek(SeekFrom::Start((header.offset_uvs) as u64))
            .unwrap();

        for _idx in 0..num_uvs {
            let uv = ss2_common::read_vec2(reader);
            uvs.push(uv);
        }
    }

    uvs
}

fn read_extended_materials<T: Read + Seek>(
    header: &ObjBinHeader,
    materials: &mut Vec<SystemShock2MeshMaterial>,
    reader: &mut T,
    version: u32,
) {
    assert!(version > 3 && header.size_mat_extra >= 8);
    if version > 3 && header.size_mat_extra >= 8 {
        reader
            .seek(SeekFrom::Start((header.offset_mat_extra) as u64))
            .unwrap();
        let remaining_size = (header.size_mat_extra - 8) as usize;

        let len = materials.len();
        for i in 0..len {
            let transparency = read_single(reader);
            let emissivity = read_single(reader);
            materials[i].transparency = transparency;
            materials[i].emissivity = emissivity;
        }

        if remaining_size > 0 {
            let _unk = read_bytes(reader, remaining_size);
        }
    }
}

fn read_vertices<T: Read + Seek>(header: &ObjBinHeader, reader: &mut T) -> Vec<Vector3<f32>> {
    reader
        .seek(SeekFrom::Start((header.offset_verts) as u64))
        .unwrap();

    let mut vertices = Vec::new();

    let len = header.num_verts;
    for _idx in 0..len {
        let vertex_position = read_vec3(reader) / SCALE_FACTOR;
        vertices.push(vertex_position);
    }

    vertices
}

#[derive(Debug)]
pub struct SubObjectHeader {
    idx: u32,
    parent_idx: i32,
    name: String,
    transform: Matrix4<f32>,
    min_range: f32,
    max_range: f32,
    child_sub_obj_idx: i16,
    next_sub_obj_idx: i16,
    point_start: u16,
    point_stop: u16,
}

fn read_sub_objects<T: Read + Seek>(header: &ObjBinHeader, reader: &mut T) -> Vec<SubObjectHeader> {
    reader
        .seek(SeekFrom::Start((header.offset_objs) as u64))
        .unwrap();

    let _obj_size = (header.offset_mats - header.offset_objs) / (header.num_objs as u32);

    let mut objs = Vec::new();
    for i in 0..header.num_objs {
        let name = read_string_with_size(reader, 8);
        let _obj_type = read_u8(reader);
        let parent_idx = read_i32(reader);
        let min_range = read_single(reader);
        let max_range = read_single(reader);

        // Transform
        let mut decomposed = read_matrix(reader);
        decomposed.disp /= SCALE_FACTOR;
        let transform: Matrix4<f32> = decomposed.into();

        let child_sub_obj_idx = read_i16(reader);
        let next_sub_obj_idx = read_i16(reader);
        let _vhot_start = read_i16(reader);
        let _num_vhots = read_i16(reader);
        let point_start = read_u16(reader);
        let sub_num_points = read_u16(reader);

        // Not sure what this is
        let _ = read_bytes(reader, 12);

        let soh = SubObjectHeader {
            idx: i as u32,
            parent_idx,
            child_sub_obj_idx,
            next_sub_obj_idx,
            min_range,
            max_range,
            name,
            transform,
            point_start,
            point_stop: point_start + sub_num_points,
        };
        objs.push(soh);
    }
    objs
}

pub struct ObjBinHeader {
    bbox_min: Point3<f32>,
    bbox_max: Point3<f32>,
    obj_name: String,
    num_mats: u8,
    num_objs: u8,
    num_polygons: u16,
    num_verts: u16,
    num_vhots: u8,

    offset_mats: u32,
    offset_mat_extra: u32,
    offset_objs: u32,
    mat_flags: u32,
    size_mat_extra: u32,
    offset_polygons: u32,
    offset_verts: u32,
    offset_vhots: u32,
    offset_uvs: u32,
}

pub fn read_header<T: Read>(reader: &mut T, common_header: &SystemShock2BinHeader) -> ObjBinHeader {
    let version = common_header.version;
    let obj_name = ss2_common::read_string_with_size(reader, 8);

    let _sphere_rad = ss2_common::read_single(reader) / SCALE_FACTOR;
    let _max_poly_rad: f32 = ss2_common::read_single(reader) / SCALE_FACTOR;

    let bbox_max_initial = ss2_common::read_point3(reader) / SCALE_FACTOR;
    let bbox_min_initial = ss2_common::read_point3(reader) / SCALE_FACTOR;

    // Because of the tweaks to the coordinate system, there is no guarantee that the
    // provided min/max are actually the min/max - so we need to normalize them.
    let bbox_min = point3(
        bbox_min_initial.x.min(bbox_max_initial.x),
        bbox_min_initial.y.min(bbox_max_initial.y),
        bbox_min_initial.z.min(bbox_max_initial.z),
    );

    let bbox_max = point3(
        bbox_min_initial.x.max(bbox_max_initial.x),
        bbox_min_initial.y.max(bbox_max_initial.y),
        bbox_min_initial.z.max(bbox_max_initial.z),
    );
    let _parent_center = ss2_common::read_vec3(reader) / SCALE_FACTOR;

    let num_polygons = ss2_common::read_u16(reader);
    let num_verts = ss2_common::read_u16(reader);
    let _num_params = ss2_common::read_u16(reader);

    let num_mats = ss2_common::read_u8(reader);
    let _num_vcalls = ss2_common::read_u8(reader);
    let num_vhots = ss2_common::read_u8(reader);
    let num_objs = ss2_common::read_u8(reader);

    let offset_objs = ss2_common::read_u32(reader);
    let offset_mats = ss2_common::read_u32(reader);
    let offset_uvs = ss2_common::read_u32(reader);
    let offset_vhots = ss2_common::read_u32(reader);
    let offset_verts = ss2_common::read_u32(reader);
    let _offset_lights = ss2_common::read_u32(reader);
    let _offset_normals = ss2_common::read_u32(reader);
    let offset_polygons = ss2_common::read_u32(reader);
    let _offset_nodes = ss2_common::read_u32(reader);
    let _model_size = ss2_common::read_u32(reader);

    let mut offset_mat_extra = 0;
    let mut size_mat_extra = 0;
    let mut mat_flags = 0;

    if version > 3 {
        mat_flags = read_u32(reader);
        offset_mat_extra = read_u32(reader);
        size_mat_extra = read_u32(reader);
        assert!(size_mat_extra >= 8);
    }

    ObjBinHeader {
        bbox_min,
        bbox_max,
        obj_name,
        offset_mats,
        offset_objs,
        offset_verts,
        offset_vhots,
        offset_uvs,
        offset_polygons,
        num_mats,
        num_objs,
        num_polygons,
        num_verts,
        num_vhots,
        offset_mat_extra,
        size_mat_extra,
        mat_flags,
    }
}

fn convert_skinned_vertices_to_static_vertices(
    vertices: &Vec<VertexPositionTextureSkinned>,
    skeleton: &Skeleton,
) -> Vec<VertexPositionTexture> {
    let mut v = Vec::new();

    let bone_transform = skeleton.get_transforms()[0];
    for vertex in vertices {
        let _bone_indices = vertex.bone_indices;
        let position = bone_transform
            .transform_point(point3(
                vertex.position.x,
                vertex.position.y,
                vertex.position.z,
            ))
            .to_vec();
        v.push(VertexPositionTexture {
            position,
            uv: vertex.uv,
        });
    }

    v
}
