use std::rc::Rc;

use cgmath::{Matrix4, Vector3};
use collision::Aabb3;
use engine::assets::{asset_cache::AssetCache, asset_importer::AssetImporter};
use once_cell::sync::Lazy;

use crate::{
    importers::glb_animation_importer::extract_skeleton_from_document, model::Model,
    ss2_skeleton::Skeleton,
};
use engine::scene::{
    SceneObject, SkinnedMaterial, VertexPositionTextureNormal, VertexPositionTextureSkinnedNormal,
};
use engine::texture::{self, TextureOptions};
use engine::texture_format::{PixelFormat, RawTextureData};

// GLB data structures
#[derive(Debug)]
pub enum GlbVertexData {
    Static(Vec<VertexPositionTextureNormal>),
    Skinned(Vec<VertexPositionTextureSkinnedNormal>),
}

pub struct GlbMesh {
    pub vertex_data: GlbVertexData,
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
    name: String,
    reader: &mut Box<dyn engine::assets::asset_paths::ReadableAndSeekable>,
    _assets: &mut AssetCache,
    _config: &(),
) -> GlbModel {
    // Read the entire GLB file into memory
    let mut buffer = Vec::new();
    let _ = std::io::copy(reader, &mut buffer);

    // Parse the GLTF document - use manual parsing to handle external references
    let gltf = gltf::Gltf::from_slice(&buffer).expect("Failed to parse GLB file");
    let document = gltf.document;
    let blob = gltf.blob;

    // Manually process buffers
    let mut buffers = Vec::new();
    for buffer_obj in document.buffers() {
        let data = match buffer_obj.source() {
            gltf::buffer::Source::Bin => blob.as_ref().expect("No binary blob in GLB file").clone(),
            gltf::buffer::Source::Uri(uri) => {
                eprintln!(
                    "Warning: GLB file '{}' contains external buffer reference: {}",
                    name, uri
                );
                eprintln!("Using empty buffer as fallback");
                vec![]
            }
        };
        buffers.push(gltf::buffer::Data(data));
    }

    // Manually process images with checkerboard fallback
    let mut images = Vec::new();
    for image in document.images() {
        let image_data = match image.source() {
            gltf::image::Source::View { view, .. } => {
                // Get data from buffer view
                let buffer = &buffers[view.buffer().index()];
                let start = view.offset();
                let end = start + view.length();
                let buf = buffer[start..end].to_vec();

                match image::load_from_memory(&buf) {
                    Ok(loaded_image) => {
                        // Convert to RGBA8 format
                        let rgba_image = loaded_image.to_rgba8();
                        let width = rgba_image.width();
                        let height = rgba_image.height();
                        gltf::image::Data {
                            pixels: rgba_image.into_raw(),
                            format: gltf::image::Format::R8G8B8A8,
                            width,
                            height,
                        }
                    }
                    Err(_) => {
                        eprintln!(
                            "Warning: Could not decode embedded image, using checkerboard fallback"
                        );
                        create_checkerboard_image_data()
                    }
                }
            }
            gltf::image::Source::Uri { uri, .. } => {
                eprintln!(
                    "Warning: GLB file '{}' contains external image reference: {}",
                    name, uri
                );
                eprintln!("Using checkerboard pattern as fallback");
                create_checkerboard_image_data()
            }
        };
        images.push(image_data);
    }

    let mut meshes = Vec::new();
    let mut min_bounds = Vector3::new(f32::MAX, f32::MAX, f32::MAX);
    let mut max_bounds = Vector3::new(f32::MIN, f32::MIN, f32::MIN);

    // Materials will be processed directly from primitives

    // Process each mesh in the GLTF scene
    for scene in document.scenes() {
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

    // Extract skeleton from GLB file (if present)
    let skeleton = extract_skeleton_from_document(&document, &buffers);

    GlbModel {
        meshes,
        bounding_box,
        skeleton,
        images,
    }
}

/// Create a checkerboard pattern as fallback for missing textures
fn create_checkerboard_image_data() -> gltf::image::Data {
    // Create a 4x4 magenta/black checkerboard pattern
    let width = 4;
    let height = 4;
    let mut pixels = Vec::with_capacity(width * height * 4); // RGBA

    for y in 0..height {
        for x in 0..width {
            if (x + y) % 2 == 0 {
                // Magenta
                pixels.extend_from_slice(&[255, 0, 255, 255]);
            } else {
                // Black
                pixels.extend_from_slice(&[0, 0, 0, 255]);
            }
        }
    }

    gltf::image::Data {
        pixels,
        format: gltf::image::Format::R8G8B8A8,
        width: width as u32,
        height: height as u32,
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
                // Update bounding box based on vertex type
                match &glb_mesh.vertex_data {
                    GlbVertexData::Static(vertices) => {
                        for vertex in vertices {
                            let pos = &vertex.position;
                            min_bounds.x = min_bounds.x.min(pos.x);
                            min_bounds.y = min_bounds.y.min(pos.y);
                            min_bounds.z = min_bounds.z.min(pos.z);
                            max_bounds.x = max_bounds.x.max(pos.x);
                            max_bounds.y = max_bounds.y.max(pos.y);
                            max_bounds.z = max_bounds.z.max(pos.z);
                        }
                    }
                    GlbVertexData::Skinned(vertices) => {
                        for vertex in vertices {
                            let pos = &vertex.position;
                            min_bounds.x = min_bounds.x.min(pos.x);
                            min_bounds.y = min_bounds.y.min(pos.y);
                            min_bounds.z = min_bounds.z.min(pos.z);
                            max_bounds.x = max_bounds.x.max(pos.x);
                            max_bounds.y = max_bounds.y.max(pos.y);
                            max_bounds.z = max_bounds.z.max(pos.z);
                        }
                    }
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

    // Check for skinning data
    let joints = primitive
        .get(&gltf::Semantic::Joints(0))
        .and_then(|accessor| extract_joints(accessor, buffers));

    let weights = primitive
        .get(&gltf::Semantic::Weights(0))
        .and_then(|accessor| extract_weights(accessor, buffers));

    // Determine if this is a skinned mesh
    let has_skinning = joints.is_some() && weights.is_some();

    let vertex_data = if has_skinning {
        let joints = joints.unwrap();
        let weights = weights.unwrap();

        println!("Processing skinned mesh with {} vertices", positions.len());

        // Create skinned vertices
        let mut skinned_vertices = Vec::new();
        for i in 0..positions.len() {
            let pos = positions[i];
            let norm = normals.get(i).copied().unwrap_or([0.0, 1.0, 0.0]);
            let tex = texcoords.get(i).copied().unwrap_or([0.0, 0.0]);

            // Get joint and weight data for this vertex
            let joint_indices = joints.get(i).copied().unwrap_or([0, 0, 0, 0]);
            let vertex_weights = weights.get(i).copied().unwrap_or([1.0, 0.0, 0.0, 0.0]);

            // Convert joint indices to u32 and normalize weights
            let bone_indices = [
                joint_indices[0] as u32,
                joint_indices[1] as u32,
                joint_indices[2] as u32,
                joint_indices[3] as u32,
            ];

            // Normalize weights to ensure they sum to 1.0
            let weight_sum =
                vertex_weights[0] + vertex_weights[1] + vertex_weights[2] + vertex_weights[3];
            let normalized_weights = if weight_sum > 0.0 {
                [
                    vertex_weights[0] / weight_sum,
                    vertex_weights[1] / weight_sum,
                    vertex_weights[2] / weight_sum,
                    vertex_weights[3] / weight_sum,
                ]
            } else {
                [1.0, 0.0, 0.0, 0.0] // Fallback to first bone if no weights
            };

            // Apply transform to position and normal
            let transformed_pos = transform * cgmath::Vector4::new(pos[0], pos[1], pos[2], 1.0);
            let transformed_norm = transform * cgmath::Vector4::new(norm[0], norm[1], norm[2], 0.0);

            skinned_vertices.push(VertexPositionTextureSkinnedNormal {
                position: cgmath::Vector3::new(
                    transformed_pos.x,
                    transformed_pos.y,
                    transformed_pos.z,
                ),
                uv: cgmath::Vector2::new(tex[0], tex[1]),
                bone_indices,                     // All 4 bone indices
                bone_weights: normalized_weights, // Normalized multi-bone weights
                normal: cgmath::Vector3::new(
                    transformed_norm.x,
                    transformed_norm.y,
                    transformed_norm.z,
                ),
            });
        }

        GlbVertexData::Skinned(skinned_vertices)
    } else {
        // Create static vertices (original behavior)
        let mut static_vertices = Vec::new();
        for i in 0..positions.len() {
            let pos = positions[i];
            let norm = normals.get(i).copied().unwrap_or([0.0, 1.0, 0.0]);
            let tex = texcoords.get(i).copied().unwrap_or([0.0, 0.0]);

            // Apply transform to position and normal
            let transformed_pos = transform * cgmath::Vector4::new(pos[0], pos[1], pos[2], 1.0);
            let transformed_norm = transform * cgmath::Vector4::new(norm[0], norm[1], norm[2], 0.0);

            static_vertices.push(VertexPositionTextureNormal {
                position: cgmath::Vector3::new(
                    transformed_pos.x,
                    transformed_pos.y,
                    transformed_pos.z,
                ),
                normal: cgmath::Vector3::new(
                    transformed_norm.x,
                    transformed_norm.y,
                    transformed_norm.z,
                ),
                uv: cgmath::Vector2::new(tex[0], tex[1]),
            });
        }

        GlbVertexData::Static(static_vertices)
    };

    // Extract material information
    let material = primitive.material();
    let (base_color, texture_index) = extract_base_color_and_texture(&material);

    Some(GlbMesh {
        vertex_data,
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

/// Extract joint indices from GLB accessor
fn extract_joints(
    accessor: gltf::Accessor,
    buffers: &[gltf::buffer::Data],
) -> Option<Vec<[u16; 4]>> {
    let view = accessor.view()?;
    let buffer = &buffers[view.buffer().index()];

    let start = view.offset() + accessor.offset();
    let component_size = match accessor.data_type() {
        gltf::accessor::DataType::U8 => 1,
        gltf::accessor::DataType::U16 => 2,
        _ => return None,
    };

    let mut joints = Vec::new();

    match component_size {
        1 => {
            // u8 joint indices
            for i in 0..accessor.count() {
                let offset = start + i * 4; // 4 joints per vertex
                if offset + 3 < buffer.len() {
                    joints.push([
                        buffer[offset] as u16,
                        buffer[offset + 1] as u16,
                        buffer[offset + 2] as u16,
                        buffer[offset + 3] as u16,
                    ]);
                }
            }
        }
        2 => {
            // u16 joint indices
            for i in 0..accessor.count() {
                let offset = start + i * 8; // 4 u16s per vertex
                if offset + 7 < buffer.len() {
                    let j0 = u16::from_le_bytes([buffer[offset], buffer[offset + 1]]);
                    let j1 = u16::from_le_bytes([buffer[offset + 2], buffer[offset + 3]]);
                    let j2 = u16::from_le_bytes([buffer[offset + 4], buffer[offset + 5]]);
                    let j3 = u16::from_le_bytes([buffer[offset + 6], buffer[offset + 7]]);
                    joints.push([j0, j1, j2, j3]);
                }
            }
        }
        _ => return None,
    }

    Some(joints)
}

/// Extract vertex weights from GLB accessor
fn extract_weights(
    accessor: gltf::Accessor,
    buffers: &[gltf::buffer::Data],
) -> Option<Vec<[f32; 4]>> {
    let view = accessor.view()?;
    let buffer = &buffers[view.buffer().index()];

    let start = view.offset() + accessor.offset();
    let end = start + accessor.count() * 16; // 4 f32s per vertex

    let data = &buffer[start..end.min(buffer.len())];
    let mut weights = Vec::new();

    for chunk in data.chunks_exact(16) {
        // 4 * 4 bytes per f32
        if chunk.len() >= 16 {
            let w0 = f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
            let w1 = f32::from_le_bytes([chunk[4], chunk[5], chunk[6], chunk[7]]);
            let w2 = f32::from_le_bytes([chunk[8], chunk[9], chunk[10], chunk[11]]);
            let w3 = f32::from_le_bytes([chunk[12], chunk[13], chunk[14], chunk[15]]);
            weights.push([w0, w1, w2, w3]);
        }
    }
    Some(weights)
}

/// Find the dominant bone (highest weight) for simplified skinning

fn process_glb_model(glb_model: GlbModel, _asset_cache: &mut AssetCache, _config: &()) -> Model {
    let mut scene_objects = Vec::new();

    // Convert GLB meshes to SceneObjects
    for glb_mesh in glb_model.meshes.into_iter() {
        let GlbMesh {
            vertex_data,
            indices,
            base_color,
            texture_index,
        } = glb_mesh;

        let (geometry, is_skinned) = match vertex_data {
            GlbVertexData::Static(vertices) => (
                engine::scene::indexed_mesh::create(vertices, indices),
                false,
            ),
            GlbVertexData::Skinned(vertices) => {
                (engine::scene::indexed_mesh::create(vertices, indices), true)
            }
        };

        let material = if is_skinned {
            create_skinned_material(&glb_model.images, texture_index, base_color)
        } else {
            create_static_material(&glb_model.images, texture_index, base_color)
        };

        let scene_object = SceneObject::create(material, Rc::new(Box::new(geometry)));
        scene_objects.push(scene_object);
    }

    // Create appropriate model type based on whether skeleton is present
    Model::from_glb(scene_objects, glb_model.bounding_box, glb_model.skeleton)
}

pub static GLB_MODELS_IMPORTER: Lazy<AssetImporter<GlbModel, Model, ()>> =
    Lazy::new(|| AssetImporter::define(load_glb, process_glb_model));

fn create_texture_from_image(
    images: &[gltf::image::Data],
    texture_index: usize,
) -> Option<Rc<dyn engine::texture::TextureTrait>> {
    if texture_index >= images.len() {
        return None;
    }

    let image_data = &images[texture_index];
    println!(
        "Loading texture {} ({}x{}, format: {:?})",
        texture_index, image_data.width, image_data.height, image_data.format
    );

    let raw_texture_data = RawTextureData {
        bytes: image_data.pixels.clone(),
        width: image_data.width,
        height: image_data.height,
        format: match image_data.format {
            gltf::image::Format::R8G8B8 => PixelFormat::RGB,
            gltf::image::Format::R8G8B8A8 => PixelFormat::RGBA,
            _ => PixelFormat::RGBA,
        },
    };

    let texture = texture::init_from_memory2(raw_texture_data, &TextureOptions::default());

    Some(Rc::new(texture) as Rc<dyn engine::texture::TextureTrait>)
}

fn create_solid_color_texture(base_color: [f32; 4]) -> Rc<dyn engine::texture::TextureTrait> {
    let clamp = |value: f32| -> u8 { (value.clamp(0.0, 1.0) * 255.0).round() as u8 };

    let r = clamp(base_color[0]);
    let g = clamp(base_color[1]);
    let b = clamp(base_color[2]);
    let a = clamp(base_color[3]);

    let raw_texture_data = RawTextureData {
        bytes: vec![r, g, b, a],
        width: 1,
        height: 1,
        format: PixelFormat::RGBA,
    };

    let texture = texture::init_from_memory2(raw_texture_data, &TextureOptions::default());

    Rc::new(texture) as Rc<dyn engine::texture::TextureTrait>
}

fn create_static_material(
    images: &[gltf::image::Data],
    texture_index: Option<usize>,
    base_color: [f32; 4],
) -> std::cell::RefCell<Box<dyn engine::scene::Material>> {
    match texture_index {
        Some(texture_index) => {
            if let Some(texture) = create_texture_from_image(images, texture_index) {
                return std::cell::RefCell::new(engine::scene::basic_material::create(
                    texture, 1.0, 0.0,
                ));
            }

            println!(
                "Texture index {} out of range (only {} images available), using base color: {:?}",
                texture_index,
                images.len(),
                base_color
            );

            std::cell::RefCell::new(engine::scene::color_material::create(cgmath::vec3(
                base_color[0],
                base_color[1],
                base_color[2],
            )))
        }
        None => {
            println!("No texture specified, using base color: {:?}", base_color);
            std::cell::RefCell::new(engine::scene::color_material::create(cgmath::vec3(
                base_color[0],
                base_color[1],
                base_color[2],
            )))
        }
    }
}

fn create_skinned_material(
    images: &[gltf::image::Data],
    texture_index: Option<usize>,
    base_color: [f32; 4],
) -> std::cell::RefCell<Box<dyn engine::scene::Material>> {
    let texture: Rc<dyn engine::texture::TextureTrait> = if let Some(texture_index) = texture_index
    {
        match create_texture_from_image(images, texture_index) {
            Some(tex) => tex,
            None => {
                println!(
                    "Texture index {} out of range (only {} images available) for skinned mesh, using base color: {:?}",
                    texture_index,
                    images.len(),
                    base_color
                );
                create_solid_color_texture(base_color)
            }
        }
    } else {
        println!(
            "No texture specified for skinned mesh, using base color: {:?}",
            base_color
        );
        create_solid_color_texture(base_color)
    };

    std::cell::RefCell::new(SkinnedMaterial::create(texture, 1.0, 0.0))
}

fn extract_base_color_and_texture(material: &gltf::Material) -> ([f32; 4], Option<usize>) {
    if let Some(spec_gloss) = material.pbr_specular_glossiness() {
        let diffuse_factor = spec_gloss.diffuse_factor();
        let texture_index = spec_gloss
            .diffuse_texture()
            .map(|texture_info| texture_info.texture().source().index());
        return (diffuse_factor, texture_index);
    }

    let pbr = material.pbr_metallic_roughness();
    let base_color = pbr.base_color_factor();
    let texture_index = pbr
        .base_color_texture()
        .map(|texture_info| texture_info.texture().source().index());
    (base_color, texture_index)
}
