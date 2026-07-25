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
use tracing::{trace, warn};

use crate::{
    SCALE_FACTOR,
    importers::TEXTURE_IMPORTER,
    motion::JointId,
    ss2_bin_header::SystemShock2BinHeader,
    ss2_common::{
        read_bytes, read_i8, read_i16, read_i32, read_packed_normal, read_point3, read_single,
        read_string_with_size, read_u8, read_u16, read_u32, read_vec2, read_vec3,
    },
    ss2_skeleton::Skeleton,
    util::load_multiple_textures_for_model,
};

/// Number of base joint slots in the skinning palette; stretchy frames for
/// joint `j` live at palette slot `MAX_JOINTS + j` (see
/// `ss2_skeleton::expand_skinning_palette`).
pub const MAX_JOINTS: usize = engine::scene::MAX_SKINNED_JOINTS;

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
    /// One weight per vertex of each stretchy segment (indexed by
    /// `AIJointInfo::weight_index + local vertex index`); rigid segments
    /// consume none. 0 = follow the joint's parent frame, 1 = the joint.
    pub weights: Vec<f32>,
}

impl SystemShock2AIMesh {
    /// All vertex positions assigned to each skeleton joint, grouped by joint id.
    ///
    /// Uses each joint's full `start_vertex..start_vertex+num_vertices` range
    /// (mapped through `joint_map`), i.e. the *complete* vertex set per joint -
    /// unlike the damage hitboxes built in `to_vertices`, which only sample one
    /// vertex per triangle. This is the source of truth for computing a tight
    /// per-limb bounding volume (see the `hitbox_analyzer` tool).
    pub fn joint_vertex_positions(&self) -> HashMap<u32, Vec<Point3<f32>>> {
        let mut out: HashMap<u32, Vec<Point3<f32>>> = HashMap::new();
        for joint in &self.joints {
            let mapper_id = joint.mapper_id as usize;
            let joint_id = match self.joint_map.get(mapper_id) {
                Some(entry) => entry.joint as i32,
                None => continue,
            };
            if joint_id < 0 {
                continue;
            }
            let start = joint.start_vertex.max(0) as usize;
            let count = joint.num_vertices.max(0) as usize;
            let end = (start + count).min(self.vertices.len());
            let entry = out.entry(joint_id as u32).or_default();
            for vertex in &self.vertices[start..end] {
                entry.push(*vertex);
            }
        }
        out
    }
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

/// A segment: a run of mesh geometry bound to one joint. Non-stretchy
/// segments follow their joint rigidly; a stretchy segment is the blend
/// region between `joint`'s parent and `joint`, with one per-vertex weight
/// (see `to_vertices`). Validated field-by-field against creature meshes
/// (byte-level dump of GRUNT_P.BIN: 29 segments in stretchy/rigid pairs,
/// stretchy vertex counts exactly matching the header weight count).
#[derive(Debug, Clone)]
pub struct AIJointMapEntry {
    pub joint: i8,
    #[allow(dead_code)]
    num_of_material_segments: i8,
    #[allow(dead_code)]
    map_start: i8,
    /// The segment blends between `joint`'s parent and `joint`; its vertices
    /// carry weights (0 = parent-frame, 1 = joint-frame).
    pub stretchy: bool,
}

pub fn read_joint_map_entry<T: Read + Seek>(reader: &mut T) -> AIJointMapEntry {
    let _bbox = read_i32(reader);
    let joint = read_i8(reader);
    let num_of_material_segments = read_i8(reader);
    let map_start = read_i8(reader);
    let flags = read_i8(reader);
    // The remaining 12 bytes are the segment's own geometry ranges, unused:
    // vertex/polygon ranges come from the material-segment table below.
    let _data_chunk = read_vec3(reader);

    AIJointMapEntry {
        joint,
        num_of_material_segments,
        map_start,
        stretchy: (flags & 0x1) != 0,
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
    /// Start of this run's weights in the mesh weight array; only meaningful
    /// when the owning segment is stretchy (garbage for rigid runs).
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

        // Allow a mod layer to supply this texture under a different extension. If
        // nothing resolves we fall back to the literal name so the failure surfaces
        // exactly as it did before.
        let resolved = crate::util::resolve_texture_name(asset_cache, &material_name)
            .unwrap_or_else(|| material_name.clone());
        let texture = asset_cache.get(&TEXTURE_IMPORTER, &resolved).clone();
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
        let skinning_data = Skeleton::expand_skinning_palette(&skeleton.get_transforms(), skeleton);
        scene_object.set_skinning_palette(skinning_data);
        scene_objects.push(scene_object);
    }

    trace!("ai_mesh produced {} scene objects", scene_objects.len());

    (scene_objects, hitboxes)
}

pub fn to_vertices(
    mesh: &SystemShock2AIMesh,
    _skeleton: &Skeleton,
) -> (
    Vec<(String, Vec<VertexPositionTextureSkinnedNormal>)>,
    HashMap<u32, Aabb3<f32>>,
) {
    let materials = &mesh.materials;
    let triangles = &mesh.triangles;
    let uvs = &mesh.uvs;
    let vertices = &mesh.vertices;
    let _normals = &mesh.normals;
    let joints = &mesh.joints;
    let joint_map = &mesh.joint_map;

    // Map each vertex to its joint binding: rigid vertices follow one joint;
    // vertices of a stretchy segment blend between the joint's parent frame
    // and the joint by their authored weight (see the palette layout in
    // `ss2_skeleton::expand_skinning_palette`: slot `40 + j` is joint j's
    // parent-oriented frame).
    let mut vertex_to_weights: HashMap<u16, (JointId, Option<f32>)> = HashMap::new();

    for joint in joints {
        let start_vertex = joint.start_vertex as u16;
        let end_vertex = start_vertex + (joint.num_vertices as u16);

        let segment = &joint_map[joint.mapper_id as usize];
        if segment.joint < 0 || (segment.joint as usize) >= MAX_JOINTS {
            // An out-of-range joint would index garbage in the bone palette;
            // bind rigidly to the root instead so the part stays attached.
            warn!(
                "AI mesh segment {} has out-of-range joint {}; binding to root",
                joint.mapper_id, segment.joint
            );
            for i in start_vertex..end_vertex {
                vertex_to_weights.insert(i, (0, None));
            }
            continue;
        }
        let joint_id = segment.joint as JointId;
        // A stretchy run's weights must fully cover its vertices. A malformed
        // range falls back to rigid binding LOUDLY - silently degrading would
        // be indistinguishable from intentionally rigid data.
        let weight_end = joint.weight_index as usize + joint.num_vertices.max(0) as usize;
        let stretchy = segment.stretchy
            && {
                let in_bounds = weight_end <= mesh.weights.len();
                if !in_bounds {
                    warn!(
                        "AI mesh stretchy run (segment {}, joint {}) weight range {}..{} exceeds weight array ({}); binding rigidly",
                        joint.mapper_id,
                        joint_id,
                        joint.weight_index,
                        weight_end,
                        mesh.weights.len()
                    );
                }
                in_bounds
            };
        for (local_idx, i) in (start_vertex..end_vertex).enumerate() {
            let stretch_weight = if stretchy {
                Some(mesh.weights[joint.weight_index as usize + local_idx])
            } else {
                None
            };
            vertex_to_weights.insert(i, (joint_id, stretch_weight));
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

            let (j1, w1) = *vertex_to_weights.get(&tri.vert_index0).unwrap();
            let (j2, w2) = *vertex_to_weights.get(&tri.vert_index1).unwrap();
            let (j3, w3) = *vertex_to_weights.get(&tri.vert_index2).unwrap();

            add_vertex_to_hitbox(&mut joint_to_hitbox, j1, v0);
            // add_vertex_to_hitbox(&mut joint_to_hitbox, j1, v1);
            // add_vertex_to_hitbox(&mut joint_to_hitbox, j1, v2);

            // add_vertex_to_hitbox(&mut joint_to_hitbox, j2, v0);
            add_vertex_to_hitbox(&mut joint_to_hitbox, j2, v1);
            // add_vertex_to_hitbox(&mut joint_to_hitbox, j2, v2);

            // add_vertex_to_hitbox(&mut joint_to_hitbox, j3, v0);
            // add_vertex_to_hitbox(&mut joint_to_hitbox, j3, v1);
            add_vertex_to_hitbox(&mut joint_to_hitbox, j3, v2);

            // let xform0 = skeleton.global_transform(j1);
            // let xform1 = skeleton.global_transform(j2);
            // let xform2 = skeleton.global_transform(j3);

            let uv0 = uvs[tri.vert_index0 as usize].uv;
            let uv1 = uvs[tri.vert_index1 as usize].uv;
            let uv2 = uvs[tri.vert_index2 as usize].uv;

            // Use per-vertex normals from packed UV data instead of triangle normals
            let normal0 = uvs[tri.vert_index0 as usize].normal; // Coordinate transform already applied
            let normal1 = uvs[tri.vert_index1 as usize].normal;
            let normal2 = uvs[tri.vert_index2 as usize].normal;

            verts.push(build_vertex(v0, uv0, normal0, skin_binding(j1, w1)));
            verts.push(build_vertex(v1, uv1, normal1, skin_binding(j2, w2)));
            verts.push(build_vertex(v2, uv2, normal2, skin_binding(j3, w3)));
        }
        material_to_verts.push((name.to_owned(), verts));
    }

    (material_to_verts, joint_to_hitbox)
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

/// Bone palette slots + weights for a vertex. Rigid vertices follow their
/// joint alone; a stretchy vertex blends the joint (weight `w`) with the
/// joint's parent-oriented frame at palette slot `40 + joint` (weight
/// `1 - w`) - see `ss2_skeleton::expand_skinning_palette`.
fn skin_binding(joint: JointId, stretch_weight: Option<f32>) -> ([u32; 4], [f32; 4]) {
    match stretch_weight {
        Some(w) => {
            let w = w.clamp(0.0, 1.0);
            (
                [joint, MAX_JOINTS as u32 + joint, 0, 0],
                [w, 1.0 - w, 0.0, 0.0],
            )
        }
        None => ([joint, 0, 0, 0], [1.0, 0.0, 0.0, 0.0]),
    }
}

fn build_vertex(
    vec: Point3<f32>,
    uv: Vector2<f32>,
    normal: Vector3<f32>,
    (bone_indices, bone_weights): ([u32; 4], [f32; 4]),
) -> VertexPositionTextureSkinnedNormal {
    VertexPositionTextureSkinnedNormal {
        position: vec.to_vec(),
        uv,
        bone_indices,
        bone_weights,
        normal,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rigid_vertices_bind_to_a_single_joint() {
        let (indices, weights) = skin_binding(7, None);
        assert_eq!(indices, [7, 0, 0, 0]);
        assert_eq!(weights, [1.0, 0.0, 0.0, 0.0]);
    }

    #[test]
    fn stretchy_vertices_blend_joint_with_its_parent_frame_slot() {
        let (indices, weights) = skin_binding(7, Some(0.75));
        assert_eq!(indices, [7, MAX_JOINTS as u32 + 7, 0, 0]);
        assert_eq!(weights, [0.75, 0.25, 0.0, 0.0]);

        // Degenerate weights from data are clamped, not propagated.
        let (_, weights) = skin_binding(3, Some(1.5));
        assert_eq!(weights, [1.0, 0.0, 0.0, 0.0]);
    }

    /// Structure-level invariants against a real creature mesh, mirroring the
    /// byte-level validation this work was built on: every stretchy-segment
    /// vertex has exactly one weight (the header's weight count), weights are
    /// sane blend factors, and every segment's joint is in palette range.
    #[test]
    fn grunt_mesh_weights_cover_exactly_the_stretchy_vertices() {
        // Same search the engine's data_root uses, minus the shock2vr
        // dependency (dark can't depend on it).
        let path = std::env::var("DARK_ASSET_PATH")
            .map(std::path::PathBuf::from)
            .into_iter()
            .chain(["Data", "../Data", "../../Data"].map(std::path::PathBuf::from))
            .map(|root| root.join("res/mesh/GRUNT_P.BIN"))
            .find(|p| p.exists());
        let Some(path) = path else {
            eprintln!("skipping: GRUNT_P.BIN not found (no game data present)");
            return;
        };
        let mut file = std::fs::File::open(path).unwrap();
        let common_header = crate::ss2_bin_header::read(&mut file);
        let mesh = read(&mut file, &common_header);

        assert!(!mesh.weights.is_empty(), "grunt has stretchy segments");
        assert!(
            mesh.weights.iter().all(|w| (0.0..=1.0).contains(w)),
            "weights are blend factors"
        );

        let stretchy_verts: usize = mesh
            .joints
            .iter()
            .filter(|smatseg| mesh.joint_map[smatseg.mapper_id as usize].stretchy)
            .map(|smatseg| smatseg.num_vertices as usize)
            .sum();
        assert_eq!(
            stretchy_verts,
            mesh.weights.len(),
            "one weight per stretchy vertex, none for rigid"
        );

        for segment in &mesh.joint_map {
            assert!(
                segment.joint >= 0 && (segment.joint as usize) < MAX_JOINTS,
                "segment joints stay in palette range"
            );
        }
    }
}

/// Build scene objects from an appended `PMNM` high-detail chunk, in its authored
/// rest pose.
///
/// Opt-in via `SS2_PMNM_MESHES=1`. Skeleton binding is unsolved (see
/// `ss2_bin_pmnm`), so these render unskinned and therefore do NOT animate -
/// this exists to prove the geometry and the upgraded `ND-*` textures parse and
/// render, ahead of solving the joint mapping.
pub fn pmnm_to_scene_objects(
    mesh: &crate::ss2_bin_pmnm::PmnmMesh,
    asset_cache: &mut AssetCache,
) -> Vec<SceneObject> {
    let mut scene_objects = Vec::new();
    for (material_name, vertices) in mesh.to_static_vertices() {
        if vertices.is_empty() {
            continue;
        }
        let geometry: Rc<Box<dyn engine::scene::Geometry>> =
            Rc::new(Box::new(engine::scene::mesh::create(vertices)));

        // PMNM material names carry their authoring extension (`ND-rumbler.psd`);
        // the shipped texture is `ND-rumbler.dds`, which the resolver finds by stem.
        let Some(texture) = crate::util::load_texture_with_fallback(asset_cache, &material_name)
        else {
            warn!("no texture for PMNM material \"{material_name}\"; dropping it");
            continue;
        };

        let diffuse: Rc<dyn TextureTrait> = texture;
        let material = RefCell::new(engine::scene::basic_material::create(diffuse, 0.0, 0.0));
        scene_objects.push(engine::scene::scene_object::SceneObject::create(
            material, geometry,
        ));
    }
    scene_objects
}

/// Whether the high-detail `PMNM` path is enabled.
///
/// An env var rather than the `--experimental` flag list because model loading
/// lives in `dark`, which has no access to `shock2vr`'s options (the same reason
/// `SS2_DEBUG_NORMALS` works this way).
pub fn pmnm_enabled() -> bool {
    std::env::var_os("SS2_PMNM_MESHES").is_some()
}
