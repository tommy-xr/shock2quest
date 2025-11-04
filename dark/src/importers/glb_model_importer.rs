use std::rc::Rc;

use cgmath::{Matrix4, Vector3};
use collision::Aabb3;
use engine::assets::{asset_cache::AssetCache, asset_importer::AssetImporter};
use once_cell::sync::Lazy;

use crate::{model::Model, ss2_skeleton::Skeleton};
use engine::scene::{SceneObject, VertexPositionTextureNormal};
use engine::texture::{self, TextureOptions};
use engine::texture_format::{PixelFormat, RawTextureData};

// GLB data structures
pub struct GlbMesh {
    pub vertices: Vec<VertexPositionTextureNormal>,
    pub indices: Vec<u32>,
    pub base_color: [f32; 4],         // RGBA base color from material
    pub texture_index: Option<usize>, // Index into images array
}

pub struct GlbModel {
    pub meshes: Vec<GlbMesh>,
    pub bounding_box: Aabb3<f32>,
    pub skeleton: Option<Skeleton>,
    pub images: Vec<gltf::image::Data>,
}

fn load_glb(
    _name: String,
    reader: &mut Box<dyn engine::assets::asset_paths::ReadableAndSeekable>,
    _assets: &mut AssetCache,
    _config: &(),
) -> GlbModel {
    // Read the entire GLB file into memory
    let mut buffer = Vec::new();
    let _ = std::io::copy(reader, &mut buffer);

    // Parse the GLTF document
    let (gltf, buffers, images) = gltf::import_slice(&buffer).expect("Failed to parse GLB file");

    let mut meshes = Vec::new();
    let mut min_bounds = Vector3::new(f32::MAX, f32::MAX, f32::MAX);
    let mut max_bounds = Vector3::new(f32::MIN, f32::MIN, f32::MIN);

    // Materials will be processed directly from primitives

    // Process each mesh in the GLTF scene
    for scene in gltf.scenes() {
        for node in scene.nodes() {
            process_node(
                &node,
                &buffers,
                &mut meshes,
                &mut min_bounds,
                &mut max_bounds,
            );
        }
    }

    let bounding_box = Aabb3::new(
        cgmath::Point3::new(min_bounds.x, min_bounds.y, min_bounds.z),
        cgmath::Point3::new(max_bounds.x, max_bounds.y, max_bounds.z),
    );

    GlbModel {
        meshes,
        bounding_box,
        skeleton: None, // TODO: Extract skeleton data if present
        images,
    }
}

fn process_node(
    node: &gltf::Node,
    buffers: &[gltf::buffer::Data],
    meshes: &mut Vec<GlbMesh>,
    min_bounds: &mut Vector3<f32>,
    max_bounds: &mut Vector3<f32>,
) {
    // Apply node transform
    let transform = Matrix4::from(node.transform().matrix());

    // Process mesh if present
    if let Some(mesh) = node.mesh() {
        for primitive in mesh.primitives() {
            if let Some(glb_mesh) = process_primitive(&primitive, buffers, &transform) {
                // Update bounding box
                for vertex in &glb_mesh.vertices {
                    let pos = &vertex.position;
                    min_bounds.x = min_bounds.x.min(pos.x);
                    min_bounds.y = min_bounds.y.min(pos.y);
                    min_bounds.z = min_bounds.z.min(pos.z);
                    max_bounds.x = max_bounds.x.max(pos.x);
                    max_bounds.y = max_bounds.y.max(pos.y);
                    max_bounds.z = max_bounds.z.max(pos.z);
                }
                meshes.push(glb_mesh);
            }
        }
    }

    // Process child nodes recursively
    for child in node.children() {
        process_node(&child, buffers, meshes, min_bounds, max_bounds);
    }
}

fn process_primitive(
    primitive: &gltf::Primitive,
    buffers: &[gltf::buffer::Data],
    transform: &Matrix4<f32>,
) -> Option<GlbMesh> {
    // Get position data
    let position_accessor = primitive.get(&gltf::Semantic::Positions)?;
    let positions = extract_positions(position_accessor, buffers)?;

    // Get normal data (optional)
    let normals = primitive
        .get(&gltf::Semantic::Normals)
        .and_then(|accessor| extract_normals(accessor, buffers))
        .unwrap_or_else(|| vec![[0.0, 1.0, 0.0]; positions.len()]);

    // Get texture coordinate data (optional)
    let texcoords = primitive
        .get(&gltf::Semantic::TexCoords(0))
        .and_then(|accessor| extract_texcoords(accessor, buffers))
        .unwrap_or_else(|| vec![[0.0, 0.0]; positions.len()]);

    // Get indices
    let indices = primitive
        .indices()
        .and_then(|accessor| extract_indices(accessor, buffers))
        .unwrap_or_else(|| (0..positions.len() as u32).collect());

    // Create vertices
    let mut vertices = Vec::new();
    for i in 0..positions.len() {
        let pos = positions[i];
        let norm = normals.get(i).copied().unwrap_or([0.0, 1.0, 0.0]);
        let tex = texcoords.get(i).copied().unwrap_or([0.0, 0.0]);

        // Apply transform to position and normal
        let transformed_pos = transform * cgmath::Vector4::new(pos[0], pos[1], pos[2], 1.0);
        let transformed_norm = transform * cgmath::Vector4::new(norm[0], norm[1], norm[2], 0.0);

        vertices.push(VertexPositionTextureNormal {
            position: cgmath::Vector3::new(transformed_pos.x, transformed_pos.y, transformed_pos.z),
            normal: cgmath::Vector3::new(
                transformed_norm.x,
                transformed_norm.y,
                transformed_norm.z,
            ),
            uv: cgmath::Vector2::new(tex[0], tex[1]),
        });
    }

    // Extract material information
    let material = primitive.material();
    let pbr = material.pbr_metallic_roughness();
    let base_color = pbr.base_color_factor();

    let texture_index = pbr
        .base_color_texture()
        .map(|texture_info| texture_info.texture().source().index());

    Some(GlbMesh {
        vertices,
        indices,
        base_color,
        texture_index,
    })
}

fn extract_positions(
    accessor: gltf::Accessor,
    buffers: &[gltf::buffer::Data],
) -> Option<Vec<[f32; 3]>> {
    let view = accessor.view()?;
    let buffer = &buffers[view.buffer().index()];

    let start = view.offset() + accessor.offset();
    let end = start + accessor.count() * 12; // 3 f32s per position

    let data = &buffer[start..end.min(buffer.len())];
    let mut positions = Vec::new();

    for chunk in data.chunks_exact(12) {
        // 3 * 4 bytes per f32
        if chunk.len() >= 12 {
            let x = f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
            let y = f32::from_le_bytes([chunk[4], chunk[5], chunk[6], chunk[7]]);
            let z = f32::from_le_bytes([chunk[8], chunk[9], chunk[10], chunk[11]]);
            positions.push([x, y, z]);
        }
    }
    Some(positions)
}

fn extract_normals(
    accessor: gltf::Accessor,
    buffers: &[gltf::buffer::Data],
) -> Option<Vec<[f32; 3]>> {
    extract_positions(accessor, buffers) // Same format as positions
}

fn extract_texcoords(
    accessor: gltf::Accessor,
    buffers: &[gltf::buffer::Data],
) -> Option<Vec<[f32; 2]>> {
    let view = accessor.view()?;
    let buffer = &buffers[view.buffer().index()];

    let start = view.offset() + accessor.offset();
    let end = start + accessor.count() * 8; // 2 f32s per texcoord

    let data = &buffer[start..end.min(buffer.len())];
    let mut texcoords = Vec::new();

    for chunk in data.chunks_exact(8) {
        // 2 * 4 bytes per f32
        if chunk.len() >= 8 {
            let u = f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
            let v = f32::from_le_bytes([chunk[4], chunk[5], chunk[6], chunk[7]]);
            texcoords.push([u, v]);
        }
    }
    Some(texcoords)
}

fn extract_indices(accessor: gltf::Accessor, buffers: &[gltf::buffer::Data]) -> Option<Vec<u32>> {
    let view = accessor.view()?;
    let buffer = &buffers[view.buffer().index()];

    let start = view.offset() + accessor.offset();
    let component_size = match accessor.data_type() {
        gltf::accessor::DataType::U16 => 2,
        gltf::accessor::DataType::U32 => 4,
        _ => return None,
    };

    let mut indices = Vec::new();

    match component_size {
        2 => {
            // u16 indices
            for i in 0..accessor.count() {
                let offset = start + i * 2;
                if offset + 1 < buffer.len() {
                    let index = u16::from_le_bytes([buffer[offset], buffer[offset + 1]]) as u32;
                    indices.push(index);
                }
            }
        }
        4 => {
            // u32 indices
            for i in 0..accessor.count() {
                let offset = start + i * 4;
                if offset + 3 < buffer.len() {
                    let index = u32::from_le_bytes([
                        buffer[offset],
                        buffer[offset + 1],
                        buffer[offset + 2],
                        buffer[offset + 3],
                    ]);
                    indices.push(index);
                }
            }
        }
        _ => return None,
    }

    Some(indices)
}

fn process_glb_model(glb_model: GlbModel, _asset_cache: &mut AssetCache, _config: &()) -> Model {
    let mut scene_objects = Vec::new();

    // Convert GLB meshes to SceneObjects
    for glb_mesh in glb_model.meshes.into_iter() {
        // Create indexed geometry from vertices and indices
        let geometry = engine::scene::indexed_mesh::create(glb_mesh.vertices, glb_mesh.indices);

        // Create material based on GLB material data
        let material = if let Some(texture_index) = glb_mesh.texture_index {
            if texture_index < glb_model.images.len() {
                let image_data = &glb_model.images[texture_index];

                // Create texture from memory
                let raw_texture_data = RawTextureData {
                    bytes: image_data.pixels.clone(),
                    width: image_data.width,
                    height: image_data.height,
                    format: match image_data.format {
                        gltf::image::Format::R8G8B8 => PixelFormat::RGB,
                        gltf::image::Format::R8G8B8A8 => PixelFormat::RGBA,
                        _ => PixelFormat::RGBA, // Default fallback
                    },
                };

                let texture =
                    texture::init_from_memory2(raw_texture_data, &TextureOptions::default());

                // Create BasicMaterial with texture
                std::cell::RefCell::new(engine::scene::basic_material::create(
                    Rc::new(texture) as Rc<dyn engine::texture::TextureTrait>,
                    1.0, // emissivity
                    0.0, // transparency
                ))
            } else {
                // Fallback to color material
                std::cell::RefCell::new(engine::scene::color_material::create(cgmath::vec3(
                    glb_mesh.base_color[0],
                    glb_mesh.base_color[1],
                    glb_mesh.base_color[2],
                )))
            }
        } else {
            // No texture, use base color
            std::cell::RefCell::new(engine::scene::color_material::create(cgmath::vec3(
                glb_mesh.base_color[0],
                glb_mesh.base_color[1],
                glb_mesh.base_color[2],
            )))
        };

        let scene_object = SceneObject::create(material, Rc::new(Box::new(geometry)));
        scene_objects.push(scene_object);
    }

    // Create appropriate model type based on whether skeleton is present
    Model::from_glb(scene_objects, glb_model.bounding_box, glb_model.skeleton)
}

pub static GLB_MODELS_IMPORTER: Lazy<AssetImporter<GlbModel, Model, ()>> =
    Lazy::new(|| AssetImporter::define(load_glb, process_glb_model));
