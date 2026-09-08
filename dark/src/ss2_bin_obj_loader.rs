use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    env,
    io::{SeekFrom, prelude::*},
    rc::Rc,
    time::Duration,
};

use cgmath::{Matrix4, Vector2, Vector3, Vector4, vec4};
use cgmath::{Point3, point3, prelude::*, vec3};
use collision::Aabb3;
use engine::{
    assets::asset_cache::AssetCache,
    scene::{
        FrontFaceWinding, SceneObject, VertexPositionTextureNormal,
        VertexPositionTextureSkinnedNormal,
    },
    texture::{AnimatedTexture, TextureTrait},
};
use tracing::warn;

use crate::{
    SCALE_FACTOR,
    importers::TEXTURE_IMPORTER,
    ss2_bin_header::SystemShock2BinHeader,
    ss2_common::{
        self, read_array_u16, read_bytes, read_i16, read_i32, read_matrix, read_packed_normal,
        read_point3, read_single, read_string_with_size, read_u8, read_u16, read_u32, read_vec3,
    },
    ss2_skeleton::{Bone, Skeleton},
    util::{load_multiple_textures_for_model, resolve_object_material_texture_name},
};

// Dark LGMD polygons use clockwise front faces.
const DARK_OBJECT_FRONT_FACE: FrontFaceWinding = FrontFaceWinding::Clockwise;

#[derive(Debug, Clone)]
pub struct Vhot {
    /// Authored attachment ID, not a type or an index into the file's table.
    /// Dark's evaluated vhot table is keyed by this integer (`md_eval_vhots`).
    pub id: u32,
    pub point: Point3<f32>,
}

impl Vhot {
    pub fn read<T: Read + Seek>(reader: &mut T) -> Vhot {
        let id = read_u32(reader);
        let point = read_point3(reader) / SCALE_FACTOR;
        Vhot { id, point }
    }
}

#[derive(Clone)]
pub struct SystemShock2ObjectMesh {
    pub header: ObjBinHeader,
    pub version: u32,

    pub bounding_box: Aabb3<f32>,
    pub materials: Vec<SystemShock2MeshMaterial>,
    pub uvs: Vec<Vector2<f32>>,
    pub vertices: Vec<Vector3<f32>>,
    pub normals: Vec<Vector3<f32>>,
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
    let normals = read_lights(&header, reader);

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
        normals,
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
        .collect::<Vec<(u16, Vec<VertexPositionTextureSkinnedNormal>)>>();

    let mut bones = Vec::new();
    build_skeleton_for_obj_mesh(&mesh, 0, None, &mut bones);
    let is_skinned = bones.len() > 1;
    let skeleton = Skeleton::create_from_bones(bones);

    let mut mesh_objects = vertices
        .into_iter()
        .filter_map(|(slot, verts)| {
            if verts.is_empty() {
                warn!("no vertices produced for mesh slot {slot}; dropping it");
                return None;
            }

            let material = hash_to_material.get(&slot).unwrap();
            let mut tex_path = material.name.to_string();

            // HACK... for broken texture name
            if tex_path.to_ascii_lowercase().contains("soft12.pcx") {
                tex_path = "soft12 .pcx".to_owned();
            }

            let Some(resolved_tex_path) =
                resolve_object_material_texture_name(asset_cache, &tex_path)
            else {
                // Dropping the slot here makes part (or all) of a prop silently
                // vanish from the world, so it is worth surfacing.
                warn!("no texture for material \"{tex_path}\"; dropping mesh slot {slot}");
                return None;
            };

            let Some(texture) = asset_cache.get_opt(&TEXTURE_IMPORTER, &resolved_tex_path) else {
                warn!("could not load resolved texture \"{resolved_tex_path}\"; dropping mesh slot {slot}");
                return None;
            };

            let geometry: Rc<Box<dyn engine::scene::Geometry>> = if is_skinned {
                Rc::new(Box::new(engine::scene::mesh::create(verts)))
            } else {
                // If not skinned, convert the vertices to non-skinned representation
                let simpler_vertices =
                    convert_skinned_vertices_to_static_vertices(&verts, &skeleton);
                Rc::new(Box::new(engine::scene::mesh::create(simpler_vertices)))
            };

            let debug_normals_enabled = env::var_os("SS2_DEBUG_NORMALS").is_some();

            let diffuse_texture: Option<Rc<dyn TextureTrait>> = if debug_normals_enabled
                && !is_skinned
            {
                None
            } else {
                let mut animation_frames = load_multiple_textures_for_model(asset_cache, &tex_path);
                let texture = if !animation_frames.is_empty() {
                    animation_frames.insert(0, texture.clone());
                    Rc::new(AnimatedTexture::new(
                        animation_frames,
                        Duration::from_millis(200),
                    )) as Rc<dyn TextureTrait>
                } else {
                    texture.clone()
                };
                Some(texture)
            };

            let mut transparency = material.transparency;

            // HACK: Why isn't GLAS_S01" loading transparency??
            if tex_path.contains("GLAS_S01") {
                transparency = 0.8
            }

            let mat: Box<dyn engine::scene::Material> = if debug_normals_enabled {
                if is_skinned {
                    engine::scene::debug_normal_material::create_skinned()
                } else {
                    engine::scene::debug_normal_material::create()
                }
            } else if is_skinned {
                engine::scene::SkinnedMaterial::create(
                    diffuse_texture
                        .as_ref()
                        .expect("diffuse texture should exist when debug normals disabled")
                        .clone(),
                    material.emissivity,
                    transparency,
                )
            } else {
                engine::scene::basic_material::create(
                    diffuse_texture
                        .as_ref()
                        .expect("diffuse texture should exist when debug normals disabled")
                        .clone(),
                    material.emissivity,
                    transparency,
                )
            };

            let material = RefCell::new(mat);
            let mut so = create_dark_object_scene_object(material, geometry);

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

fn create_dark_object_scene_object(
    material: RefCell<Box<dyn engine::scene::Material>>,
    geometry: Rc<Box<dyn engine::scene::Geometry>>,
) -> SceneObject {
    let mut scene_object = SceneObject::create(material, geometry);
    scene_object.set_backface_culling(Some(DARK_OBJECT_FRONT_FACE));
    scene_object
}

// Data
#[derive(Debug, Clone)]
pub struct SystemShock2MeshMaterial {
    pub name: String,
    #[allow(dead_code)]
    material_type: u8, // TODO: Add real type
    pub slot_num: u8,
    #[allow(dead_code)]
    ipal_index: u32, // unused

    #[allow(dead_code)]
    color: Vector4<f32>,
    #[allow(dead_code)]
    handle: u32,
    #[allow(dead_code)]
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
    normal: Vector3<f32>,
    bone_idx: u32,
) -> VertexPositionTextureSkinnedNormal {
    VertexPositionTextureSkinnedNormal {
        position: vec,
        uv,
        bone_indices: [bone_idx, 0, 0, 0],
        bone_weights: [1.0, 0.0, 0.0, 0.0], // Single bone weighting for obj files
        normal,
    }
}

/// Drops every polygon whose material name `keep` rejects, leaving the part of
/// the model that the accepted materials draw.
///
/// Filtering *polygons* is enough to isolate a part, and is why this is cheap:
/// `to_vertices` walks the polygon list and only emits the vertices those
/// polygons reference, so the shared vertex/uv/normal arrays stay valid as-is
/// and the entries nothing references are simply never read. Sub-objects are
/// left intact so the skeleton - and hence the per-vertex bone index that
/// places each sub-object at its rest transform - still resolves.
pub fn retain_materials(
    mesh: SystemShock2ObjectMesh,
    keep: impl Fn(&str) -> bool,
) -> SystemShock2ObjectMesh {
    let kept_slots = material_slots(&mesh, keep);
    retain_polygons(mesh, |polygon| kept_slots.contains(&polygon.slot_index))
}

/// The material slots whose name `keep` accepts.
fn material_slots(mesh: &SystemShock2ObjectMesh, keep: impl Fn(&str) -> bool) -> HashSet<u16> {
    mesh.materials
        .iter()
        .filter(|material| keep(&material.name))
        .map(|material| material.slot_num as u16)
        .collect()
}

/// Drops every polygon `keep` rejects. See [`retain_materials`] for why
/// filtering polygons alone is sufficient.
fn retain_polygons(
    mut mesh: SystemShock2ObjectMesh,
    keep: impl Fn(&SystemShock2ObjectPolygon) -> bool,
) -> SystemShock2ObjectMesh {
    mesh.polygons.retain(&keep);
    mesh
}

/// Wrist-to-fingertip length of an adult hand, in world units (~19 cm).
/// Anthropometric, not asset-specific - the yardstick for placing and trimming
/// any hand mesh.
pub const HAND_LENGTH_WORLD: f32 = 0.2493;

/// Splits the geometry `keep` selects into connected pieces - one per hand.
///
/// A model can draw more than one hand with the same material (the pistol has a
/// trigger hand and a support hand), and they are separate islands of geometry,
/// so connectivity is what tells them apart. Returns one mesh per island,
/// largest first, each still carrying the full material table.
pub fn split_connected(
    mesh: &SystemShock2ObjectMesh,
    keep: impl Fn(&str) -> bool,
) -> Vec<SystemShock2ObjectMesh> {
    connected_islands(mesh, keep)
        .into_iter()
        .map(|polygons| {
            let wanted = polygons.into_iter().collect::<HashSet<usize>>();
            let mut island = mesh.clone();
            island.polygons = mesh
                .polygons
                .iter()
                .enumerate()
                .filter(|(index, _)| wanted.contains(index))
                .map(|(_, polygon)| polygon.clone())
                .collect();
            island
        })
        .collect()
}

/// Polygon indices of each connected island over the polygons `keep` selects,
/// ordered largest first (then by earliest polygon, so equal-sized islands are
/// stable across runs).
pub fn connected_islands(
    mesh: &SystemShock2ObjectMesh,
    keep: impl Fn(&str) -> bool,
) -> Vec<Vec<usize>> {
    let kept_slots = material_slots(mesh, keep);

    // Union-find over vertex indices, joined along every kept polygon's edges.
    let mut parent: HashMap<u16, u16> = HashMap::new();
    fn find(parent: &mut HashMap<u16, u16>, mut node: u16) -> u16 {
        while let Some(&next) = parent.get(&node) {
            if next == node {
                break;
            }
            let grand = *parent.get(&next).unwrap_or(&next);
            parent.insert(node, grand);
            node = grand;
        }
        parent.entry(node).or_insert(node);
        node
    }

    for polygon in &mesh.polygons {
        if !kept_slots.contains(&polygon.slot_index) {
            continue;
        }
        let Some(&first) = polygon.vertex_indices.first() else {
            continue;
        };
        for index in &polygon.vertex_indices {
            let (a, b) = (find(&mut parent, first), find(&mut parent, *index));
            if a != b {
                parent.insert(a, b);
            }
        }
    }

    let mut islands: HashMap<u16, Vec<usize>> = HashMap::new();
    for (index, polygon) in mesh.polygons.iter().enumerate() {
        if !kept_slots.contains(&polygon.slot_index) {
            continue;
        }
        let Some(&first) = polygon.vertex_indices.first() else {
            continue;
        };
        let root = find(&mut parent, first);
        islands.entry(root).or_default().push(index);
    }

    // Largest first, then by earliest polygon: `HashMap::into_values` is
    // randomized, so equal-sized islands would otherwise swap between runs -
    // and callers select a hand by ordinal.
    let mut islands = islands.into_values().collect::<Vec<_>>();
    islands.sort_by_key(|polygons| {
        (
            std::cmp::Reverse(polygons.len()),
            polygons.iter().copied().min().unwrap_or(usize::MAX),
        )
    });

    islands
}

/// Where one hand sits on a first-person weapon, in model space.
///
/// Derived from the hand's *geometry* rather than its bone transform. A
/// sub-object's vertices are bone-local and are placed by the bone's global
/// transform at skinning time, so the bone's own translation is a joint pivot
/// (zero on `ar15_h`) and says nothing about where the hand ended up - only the
/// transformed geometry does.
#[derive(Debug, Clone)]
pub struct HandFrame {
    pub name: String,
    /// The wrist end of the hand, where a controller would be held.
    pub origin: Point3<f32>,
    /// Unit vector from wrist toward the fingertips.
    pub forward: Vector3<f32>,
    /// Wrist-to-fingertip distance, for sanity checks and scaling.
    pub length: f32,
}

/// One frame over all the geometry `keep` selects, for a mesh already known to
/// hold a single hand (an island out of [`split_connected`]).
pub fn hand_frame(mesh: &SystemShock2ObjectMesh, keep: impl Fn(&str) -> bool) -> Option<HandFrame> {
    let kept_slots = material_slots(mesh, keep);
    let transforms = sub_object_transforms(mesh);

    // Deduplicate by vertex index first. Walking polygon corners would push a
    // shared vertex once per incident face, so the endpoint scoring below would
    // measure triangulation density rather than geometry - a fan-triangulated
    // palm can then outweigh the fingertips and reverse the axis.
    let mut seen = HashSet::new();
    let mut points = Vec::new();
    for polygon in &mesh.polygons {
        if !kept_slots.contains(&polygon.slot_index) {
            continue;
        }
        for index in &polygon.vertex_indices {
            if !seen.insert(*index) {
                continue;
            }
            let sub_object = get_bone_index_for_point(mesh, *index) as usize;
            let Some((_, transform)) = transforms.get(sub_object) else {
                continue;
            };
            let local = mesh.vertices[*index as usize];
            points.push(transform.transform_point(point3(local.x, local.y, local.z)));
        }
    }

    let (wrist, tip) = wrist_and_fingertip(&points)?;
    let axis = tip - wrist;
    let length = axis.magnitude();
    if length <= f32::EPSILON {
        return None;
    }

    Some(HandFrame {
        name: String::new(),
        origin: wrist,
        forward: axis / length,
        length,
    })
}

/// The hand's long axis, as the farthest-apart pair of points, oriented so the
/// wrist comes first.
///
/// The *fingertip* end is the denser one: five separate digits pack far more
/// geometry into the end of the hand than the smooth tube of a forearm (or the
/// stump of a bare hand) does at the other. Verified against all six authored
/// hands - picking the denser end as the wrist instead points every one of them
/// backwards.
fn wrist_and_fingertip(points: &[Point3<f32>]) -> Option<(Point3<f32>, Point3<f32>)> {
    if points.len() < 2 {
        return None;
    }

    let mut best = (0usize, 1usize, 0.0_f32);
    for (i, a) in points.iter().enumerate() {
        for (j, b) in points.iter().enumerate().skip(i + 1) {
            let distance = (b - a).magnitude2();
            if distance > best.2 {
                best = (i, j, distance);
            }
        }
    }

    let (a, b) = (points[best.0], points[best.1]);
    let near_end = |end: Point3<f32>, other: Point3<f32>| {
        let cutoff = (other - end).magnitude() * 0.25;
        points
            .iter()
            .filter(|point| (*point - end).magnitude() < cutoff)
            .count()
    };

    if near_end(a, b) >= near_end(b, a) {
        Some((b, a))
    } else {
        Some((a, b))
    }
}

/// The model-space transform of every sub-object, paired with its name - the
/// placement the artist authored for that part.
///
/// For a 25AE first-person weapon this is what carries the *grip*: the hand
/// sub-objects (`@s01_han`, `@s02_han`) are posed onto the weapon by hand, so
/// their transforms say exactly where a hand belongs on that gun.
pub fn sub_object_transforms(mesh: &SystemShock2ObjectMesh) -> Vec<(String, Matrix4<f32>)> {
    let skeleton = obj_skeleton(mesh);

    mesh.sub_objects
        .iter()
        .enumerate()
        .map(|(index, sub_object)| {
            (
                sub_object.name.clone(),
                skeleton.global_transform(&(index as u32)),
            )
        })
        .collect()
}

/// The sub-object tree as the skeleton the renderer poses it with (joint index
/// = sub-object index).
pub fn obj_skeleton(mesh: &SystemShock2ObjectMesh) -> Skeleton {
    let mut bones = Vec::new();
    build_skeleton_for_obj_mesh(mesh, 0, None, &mut bones);
    Skeleton::create_from_bones(bones)
}

/// The model-space bounding box of every sub-object's own vertices, paired
/// with its name - where each authored part actually sits under `palette`,
/// the per-joint transforms the renderer skins with (index = sub-object
/// index; for the rest pose, `AnimationPlayer::empty().get_transforms`).
/// Empty parts (pure pivots) report `None`.
///
/// This is how a per-model anchor gets derived from the art (a magazine, a
/// grip, a sight) instead of eyeballed in a debug scene.
pub fn sub_object_bounds(
    mesh: &SystemShock2ObjectMesh,
    palette: &[Matrix4<f32>],
) -> Vec<(String, Option<Aabb3<f32>>)> {
    mesh.sub_objects
        .iter()
        .enumerate()
        .map(|(index, sub_object)| {
            let name = sub_object.name.clone();
            let transform = palette
                .get(index)
                .copied()
                .unwrap_or_else(Matrix4::identity);
            use collision::Aabb as _;
            let mut bounds: Option<Aabb3<f32>> = None;
            for index in sub_object.point_start..sub_object.point_stop {
                let Some(vertex) = mesh.vertices.get(index as usize) else {
                    continue;
                };
                let point = transform.transform_point(point3(vertex.x, vertex.y, vertex.z));
                bounds = Some(match bounds {
                    None => Aabb3::new(point, point),
                    Some(aabb) => aabb.grow(point),
                });
            }
            (name, bounds)
        })
        .collect()
}

pub fn to_vertices(
    mesh: &SystemShock2ObjectMesh,
) -> HashMap<u16, Vec<VertexPositionTextureSkinnedNormal>> {
    let polygons = &mesh.polygons;
    let uvs = &mesh.uvs;
    let vertices = &mesh.vertices;
    let normals = &mesh.normals;

    let mut hash_map = HashMap::new();

    for poly in polygons {
        let indices = &poly.vertex_indices;
        let uv_indices = &poly.uv_indices;
        let normal_indices = &poly.normal_indices;
        let slot = poly.slot_index;

        let vec = Vec::new();
        hash_map.entry(slot).or_insert(vec);

        let verts = hash_map.get_mut(&slot).unwrap();

        let len = indices.len();
        let uv_len = uv_indices.len();

        let normal_for_corner = |corner: usize| -> Vector3<f32> {
            normals
                .get(normal_indices[corner] as usize)
                .copied()
                .unwrap()
        };

        if len >= 3 && uv_len >= len {
            for idx in 1..(len - 1) {
                let bone_idx = get_bone_index_for_point(mesh, indices[idx]);

                verts.push(build_vertex(
                    vertices[indices[idx] as usize],
                    uvs[uv_indices[idx] as usize],
                    normal_for_corner(idx),
                    bone_idx,
                ));
                verts.push(build_vertex(
                    vertices[indices[idx + 1] as usize],
                    uvs[uv_indices[idx + 1_usize] as usize],
                    normal_for_corner(idx + 1),
                    bone_idx,
                ));
                verts.push(build_vertex(
                    vertices[indices[0] as usize],
                    uvs[uv_indices[0] as usize],
                    normal_for_corner(0),
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
    pub normal_indices: Vec<u16>,
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
    let normal_indices = read_array_u16(reader, num_verts as u32);

    // Read uv indices, maybe
    let mut uvs = vec![];
    if (poly_type & 3) == 3 {
        uvs = read_array_u16(reader, num_verts as u32);
    }

    // v4 and later carry one trailing byte per polygon. (The 25th Anniversary
    // Edition ships 13 LGMD v6 props; they use the same 1-byte tail as v4, so a
    // `== 4` test here silently misaligns the whole polygon stream.)
    if version >= 4 {
        let _unknown = read_u8(reader);
    }

    SystemShock2ObjectPolygon {
        vertex_indices,
        normal_indices,
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
    // Keep file order for consumers that explicitly need it. ID-based
    // attachments must look up `Vhot::id`, including sparse/high identifiers.
    vhots
}

pub fn read_lights<T: Read + Seek>(header: &ObjBinHeader, reader: &mut T) -> Vec<Vector3<f32>> {
    reader
        .seek(SeekFrom::Start((header.offset_lights) as u64))
        .unwrap();

    let mut normals = Vec::new();

    // Calculate number of lights like SystemShock2VR: (offset_normals - offset_lights) / 8
    let num_lights = (header.offset_normals - header.offset_lights) / 8;
    for _idx in 0..num_lights {
        // Read ObjLight structure (8 bytes total):
        let _material_idx = read_u16(reader); // Material reference (2 bytes)
        let _vertex_idx = read_u16(reader); // Point on object reference (2 bytes)
        let packed_normal = read_u32(reader); // Packed normal vector (4 bytes)

        let normal = read_packed_normal(packed_normal).normalize(); // Apply coordinate transform and normalize
        normals.push(normal);
    }

    normals
}

pub fn read_normals<T: Read + Seek>(header: &ObjBinHeader, reader: &mut T) -> Vec<Vector3<f32>> {
    reader
        .seek(SeekFrom::Start((header.offset_normals) as u64))
        .unwrap();

    let mut normals = Vec::new();

    let len = header.num_verts;
    for _idx in 0..len {
        // Read packed normal (32-bit) instead of raw vector3 (96-bit)
        let packed_normal = read_u32(reader);
        let normal = read_packed_normal(packed_normal);
        normals.push(normal);
    }

    normals
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
    // LGMD v3 has no extended-material chunk at all, so absence is legal rather
    // than a bug - the `if` below already handles it. (SCP ships a v3 SHOVEL.BIN.)
    if version > 3 && header.size_mat_extra >= 8 {
        reader
            .seek(SeekFrom::Start((header.offset_mat_extra) as u64))
            .unwrap();
        // `size_mat_extra` is the stride of ONE material's extra record, not the
        // size of the whole chunk: each material contributes transparency +
        // emissivity followed by `size_mat_extra - 8` bytes we don't model. Most
        // of Dark's own art uses a stride of exactly 8, but SHTUP and SCP ship
        // hundreds of models with a stride of 16 - reading those as one contiguous
        // block gives every material after the first a garbage transparency, which
        // renders the whole object invisible.
        let per_material_padding = (header.size_mat_extra - 8) as usize;

        for material in materials.iter_mut() {
            material.transparency = read_single(reader);
            material.emissivity = read_single(reader);
            if per_material_padding > 0 {
                let _unk = read_bytes(reader, per_material_padding);
            }
        }

        normalize_transparency_convention(materials);
    }
}

/// Dark stores this field as *transparency* (0.0 = opaque). Models re-exported by
/// the 25th Anniversary Edition mod layers (Nightdive, SHTUP, SCP) write 1.0 into
/// the opaque slot instead, which reads as "completely invisible" under Dark's
/// convention - the reason those props disappear from the world.
///
/// Only the opaque value was remapped: fractional transparencies survive the
/// re-export unchanged. Comparing every re-exported model against its vanilla
/// counterpart, clamping 1.0 to 0.0 reproduces the original values for 529 of
/// them and is never worse than inverting the whole model, which corrupts the 20
/// that mix 1.0 with a real transparency (`shutscrn.bin` is `[1.0, 0.9, 0.4]`
/// against vanilla's `0.9` and `0.4`; inverting would yield `0.1` and `0.6`).
///
/// Safe for the original game data, which never stores 1.0 in this field -
/// checked across all 1434 models that carry an extended-material chunk.
fn normalize_transparency_convention(materials: &mut [SystemShock2MeshMaterial]) {
    for material in materials.iter_mut() {
        if (material.transparency - 1.0).abs() < f32::EPSILON {
            material.transparency = 0.0;
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

#[derive(Debug, Clone)]
pub struct SubObjectHeader {
    #[allow(dead_code)]
    idx: u32,
    #[allow(dead_code)]
    parent_idx: i32,
    pub name: String,
    transform: Matrix4<f32>,
    #[allow(dead_code)]
    min_range: f32,
    #[allow(dead_code)]
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

#[derive(Clone)]
pub struct ObjBinHeader {
    bbox_min: Point3<f32>,
    bbox_max: Point3<f32>,
    pub obj_name: String,
    num_mats: u8,
    num_objs: u8,
    num_polygons: u16,
    pub num_verts: u16,
    num_vhots: u8,

    offset_mats: u32,
    offset_mat_extra: u32,
    offset_objs: u32,
    #[allow(dead_code)]
    mat_flags: u32,
    size_mat_extra: u32,
    offset_polygons: u32,
    offset_verts: u32,
    offset_vhots: u32,
    offset_uvs: u32,
    pub offset_lights: u32,
    pub offset_normals: u32,
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
    let offset_lights = ss2_common::read_u32(reader);
    let offset_normals = ss2_common::read_u32(reader);
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
        offset_lights,
        offset_polygons,
        offset_normals,
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
    vertices: &Vec<VertexPositionTextureSkinnedNormal>,
    skeleton: &Skeleton,
) -> Vec<VertexPositionTextureNormal> {
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
        let mut normal = bone_transform.transform_vector(vertex.normal);
        if normal.magnitude2() > f32::EPSILON {
            normal = normal.normalize();
        }
        v.push(VertexPositionTextureNormal {
            position,
            uv: vertex.uv,
            normal,
        });
    }

    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    /// S_HIVOLT.BIN contains two coplanar, opposite-wound faces with inverse U
    /// mappings. Only its authored clockwise face reads left-to-right from the
    /// affected command1 placement; rendering both lets the mirrored face win
    /// the depth test.
    #[test]
    fn dark_object_culling_selects_the_readable_sign_face() {
        let sign_faces = [
            (FrontFaceWinding::CounterClockwise, 1.0_f32, 0.0_f32),
            (FrontFaceWinding::Clockwise, 0.0_f32, 1.0_f32),
        ];

        let (_, left_u, right_u) = sign_faces
            .into_iter()
            .find(|(winding, _, _)| *winding == DARK_OBJECT_FRONT_FACE)
            .expect("the configured winding should match an authored sign face");

        assert!(
            left_u < right_u,
            "the selected sign face must not be mirrored"
        );
    }

    #[test]
    fn dark_object_scene_objects_enable_clockwise_culling() {
        let material = RefCell::new(engine::scene::color_material::create(vec3(1.0, 1.0, 1.0)));
        let geometry: Rc<Box<dyn engine::scene::Geometry>> =
            Rc::new(Box::new(engine::scene::geometry::EmptyMesh));

        let object = create_dark_object_scene_object(material, geometry);

        assert_eq!(object.backface_culling(), Some(FrontFaceWinding::Clockwise));
    }

    fn material_in_slot(name: &str, slot_num: u8) -> SystemShock2MeshMaterial {
        SystemShock2MeshMaterial {
            slot_num,
            ..material(name)
        }
    }

    fn polygon_in_slot(slot_index: u16) -> SystemShock2ObjectPolygon {
        SystemShock2ObjectPolygon {
            vertex_indices: vec![0, 1, 2],
            normal_indices: vec![0, 1, 2],
            uv_indices: vec![0, 1, 2],
            slot_index,
        }
    }

    /// A 25AE first-person model draws the hand with its own `ND-arm*` material,
    /// so selecting on the material name is what isolates the arm from the
    /// weapon - including on `sg_h`/`empgun_h`, where the hand shares a
    /// sub-object with moving gun parts and a sub-object filter would take the
    /// gun with it.
    fn mesh_with(
        materials: Vec<SystemShock2MeshMaterial>,
        polygons: Vec<SystemShock2ObjectPolygon>,
    ) -> SystemShock2ObjectMesh {
        SystemShock2ObjectMesh {
            header: header_with_mat_extra(0),
            version: 4,
            bounding_box: Aabb3::new(point3(0.0, 0.0, 0.0), point3(1.0, 1.0, 1.0)),
            materials,
            uvs: vec![Vector2::new(0.0, 0.0); 3],
            vertices: vec![vec3(0.0, 0.0, 0.0); 3],
            normals: vec![vec3(0.0, 1.0, 0.0); 3],
            vhots: Vec::new(),
            polygons,
            sub_objects: Vec::new(),
        }
    }

    fn sub_object(name: &str, local: Vector3<f32>, child: i16, next: i16) -> SubObjectHeader {
        SubObjectHeader {
            idx: 0,
            parent_idx: -1,
            name: name.to_owned(),
            transform: Matrix4::from_translation(local),
            min_range: 0.0,
            max_range: 0.0,
            child_sub_obj_idx: child,
            next_sub_obj_idx: next,
            point_start: 0,
            point_stop: 0,
        }
    }

    /// A sub-object's authored transform is relative to its parent, so a pivot
    /// is only right once the chain is composed - this is what
    /// `Model::sub_objects` (which needs GL, hence the test at this level)
    /// hands the articulation overlay.
    #[test]
    fn sub_object_transforms_compose_the_parent_chain() {
        let mut mesh = mesh_with(vec![], vec![]);
        mesh.sub_objects = vec![
            sub_object("parent", vec3(1.0, 0.0, 0.0), 1, -1),
            sub_object("child", vec3(0.0, 1.0, 0.0), -1, -1),
        ];

        let transforms = sub_object_transforms(&mesh);

        assert_eq!(transforms[0].0, "parent");
        assert_eq!(transforms[0].1.w.truncate(), vec3(1.0, 0.0, 0.0));
        assert_eq!(transforms[1].0, "child");
        assert_eq!(transforms[1].1.w.truncate(), vec3(1.0, 1.0, 0.0));
    }

    #[test]
    fn retain_materials_keeps_only_the_selected_slots() {
        let mesh = mesh_with(
            vec![
                material_in_slot("ND-ar15.psd", 0),
                material_in_slot("ND-arm.psd", 1),
            ],
            vec![
                polygon_in_slot(0),
                polygon_in_slot(1),
                polygon_in_slot(0),
                polygon_in_slot(1),
            ],
        );

        let arm = retain_materials(mesh, |name| name.starts_with("ND-arm"));

        assert_eq!(arm.polygons.len(), 2);
        assert!(arm.polygons.iter().all(|polygon| polygon.slot_index == 1));
        // The material table is left whole, so the kept slot still resolves to
        // its texture when the mesh is turned into scene objects.
        assert_eq!(arm.materials.len(), 2);
    }

    /// The weapon half is the complement, which is what lets the same call site
    /// draw the gun without the hand.
    #[test]
    fn retain_materials_can_select_the_complement() {
        let mesh = mesh_with(
            vec![
                material_in_slot("ND-ar15.psd", 0),
                material_in_slot("ND-arm.psd", 1),
            ],
            vec![polygon_in_slot(0), polygon_in_slot(1)],
        );

        let weapon = retain_materials(mesh, |name| !name.starts_with("ND-arm"));

        assert_eq!(weapon.polygons.len(), 1);
        assert_eq!(weapon.polygons[0].slot_index, 0);
    }

    fn material(name: &str) -> SystemShock2MeshMaterial {
        SystemShock2MeshMaterial {
            name: name.to_owned(),
            material_type: 0,
            slot_num: 0,
            ipal_index: 0,
            color: vec4(0.0, 0.0, 0.0, 0.0),
            handle: 0,
            uv_scale: 0.0,
            transparency: 0.0,
            emissivity: 0.0,
        }
    }

    #[test]
    fn vhots_preserve_sparse_ids_and_authored_file_order() {
        let mut header = header_with_mat_extra(8);
        header.num_vhots = 4;
        header.offset_vhots = 0;
        let ids = [10_u32, 1, 0, u32::MAX];
        let mut bytes = Vec::new();
        for id in ids {
            bytes.extend_from_slice(&id.to_le_bytes());
            for coordinate in [1.0_f32, 2.0, 3.0] {
                bytes.extend_from_slice(&(coordinate * SCALE_FACTOR).to_le_bytes());
            }
        }
        let vhots = read_vhots(&header, &mut Cursor::new(bytes));
        assert_eq!(vhots.iter().map(|vhot| vhot.id).collect::<Vec<_>>(), ids);
        assert!(
            vhots
                .iter()
                .all(|vhot| vhot.point == point3(-1.0, 3.0, 2.0))
        );
    }

    /// Header with just the fields `read_extended_materials` reads.
    fn header_with_mat_extra(size_mat_extra: u32) -> ObjBinHeader {
        ObjBinHeader {
            bbox_min: point3(0.0, 0.0, 0.0),
            bbox_max: point3(0.0, 0.0, 0.0),
            obj_name: String::new(),
            num_mats: 0,
            num_objs: 0,
            num_polygons: 0,
            num_verts: 0,
            num_vhots: 0,
            offset_mats: 0,
            offset_mat_extra: 0,
            offset_objs: 0,
            mat_flags: 0,
            size_mat_extra,
            offset_polygons: 0,
            offset_verts: 0,
            offset_vhots: 0,
            offset_uvs: 0,
            offset_lights: 0,
            offset_normals: 0,
        }
    }

    fn extra_chunk(records: &[(f32, f32)], stride: usize) -> Vec<u8> {
        let mut buf = Vec::new();
        for (trans, illum) in records {
            buf.extend_from_slice(&trans.to_le_bytes());
            buf.extend_from_slice(&illum.to_le_bytes());
            buf.extend(std::iter::repeat(0u8).take(stride - 8));
        }
        buf
    }

    #[test]
    fn extended_materials_stride_of_8_reads_every_material() {
        let mut materials = vec![material("a"), material("b")];
        let buf = extra_chunk(&[(0.25, 0.5), (0.75, 0.0)], 8);
        read_extended_materials(
            &header_with_mat_extra(8),
            &mut materials,
            &mut Cursor::new(buf),
            4,
        );
        assert_eq!(materials[0].transparency, 0.25);
        assert_eq!(materials[0].emissivity, 0.5);
        assert_eq!(materials[1].transparency, 0.75);
    }

    /// SHTUP and SCP ship models with a 16-byte extra record. Treating the chunk
    /// as one contiguous block of 8-byte records misaligns every material after
    /// the first, which is what made those props render invisible.
    #[test]
    fn extended_materials_honors_a_stride_larger_than_8() {
        let mut materials = vec![material("a"), material("b"), material("c")];
        let buf = extra_chunk(&[(0.1, 0.2), (0.3, 0.4), (0.5, 0.6)], 16);
        read_extended_materials(
            &header_with_mat_extra(16),
            &mut materials,
            &mut Cursor::new(buf),
            4,
        );
        assert_eq!(materials[0].transparency, 0.1);
        assert_eq!(materials[1].transparency, 0.3);
        assert_eq!(materials[2].transparency, 0.5);
        assert_eq!(materials[2].emissivity, 0.6);
    }

    #[test]
    fn transparency_left_alone_for_original_dark_models() {
        // Dark's own art never stores 1.0; 0.0 means opaque and must stay 0.0.
        let mut materials = vec![material("a"), material("b")];
        materials[0].transparency = 0.0;
        materials[1].transparency = 0.9;
        normalize_transparency_convention(&mut materials);
        assert_eq!(materials[0].transparency, 0.0);
        assert_eq!(materials[1].transparency, 0.9);
    }

    #[test]
    fn transparency_of_one_becomes_opaque() {
        // 1.0 is the re-exported models' "opaque"; without this they render
        // fully invisible.
        let mut materials = vec![material("frame"), material("glass")];
        materials[0].transparency = 1.0;
        materials[1].transparency = 0.5;
        normalize_transparency_convention(&mut materials);
        assert_eq!(materials[0].transparency, 0.0);
        assert_eq!(materials[1].transparency, 0.5);
    }

    /// Only the opaque value was remapped by the re-export, so a real
    /// transparency sitting alongside a 1.0 must survive untouched. Inverting the
    /// whole model instead would turn 0.9 into 0.1 (`shutscrn.bin` and 19 others).
    #[test]
    fn transparency_preserves_real_values_alongside_an_opaque_material() {
        let mut materials = vec![material("a"), material("b"), material("c")];
        materials[0].transparency = 1.0;
        materials[1].transparency = 0.9;
        materials[2].transparency = 0.4;
        normalize_transparency_convention(&mut materials);
        assert_eq!(materials[0].transparency, 0.0);
        assert_eq!(materials[1].transparency, 0.9);
        assert_eq!(materials[2].transparency, 0.4);
    }
}
