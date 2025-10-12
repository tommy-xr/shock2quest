use std::mem::size_of;

use cgmath::{Vector2, Vector3, Vector4};
use gl::types;

pub struct TextVertex {
    pub position: Vector2<f32>,
    pub uv: Vector2<f32>,
}

impl Vertex for TextVertex {
    fn get_total_size() -> isize {
        size_of::<TextVertex>() as isize
    }

    fn get_vertex_attributes() -> Vec<VertexAttribute> {
        vec![
            VertexAttribute {
                attribute_type: VertexAttributeType::Float,
                offset: offset_of!(TextVertex, position),
                size: 2,
            },
            VertexAttribute {
                attribute_type: VertexAttributeType::Float,
                offset: offset_of!(TextVertex, uv),
                size: 2,
            },
        ]
    }
}

#[derive(Debug, Clone)]
#[repr(C)]
pub struct VertexPosition {
    pub position: Vector3<f32>,
}

impl Vertex for VertexPosition {
    fn get_total_size() -> isize {
        size_of::<VertexPosition>() as isize
    }

    fn get_vertex_attributes() -> Vec<VertexAttribute> {
        vec![VertexAttribute {
            attribute_type: VertexAttributeType::Float,
            offset: offset_of!(VertexPosition, position),
            size: 3,
        }]
    }
}

#[derive(Debug, Clone)]
#[repr(C)]
pub struct VertexPositionTexture {
    pub position: Vector3<f32>,
    pub uv: Vector2<f32>,
}

#[derive(Debug, Clone)]
#[repr(C)]
pub struct VertexPositionTextureLightmapAtlas {
    pub position: Vector3<f32>,
    pub uv: Vector2<f32>,
    pub lightmap_uv: Vector2<f32>,
    pub lightmap_atlas: Vector4<f32>,
}

impl Vertex for VertexPositionTextureLightmapAtlas {
    fn get_total_size() -> isize {
        size_of::<VertexPositionTextureLightmapAtlas>() as isize
    }

    fn get_vertex_attributes() -> Vec<VertexAttribute> {
        vec![
            VertexAttribute {
                attribute_type: VertexAttributeType::Float,
                offset: offset_of!(VertexPositionTextureLightmapAtlas, position),
                size: 3,
            },
            VertexAttribute {
                attribute_type: VertexAttributeType::Float,
                offset: offset_of!(VertexPositionTextureLightmapAtlas, uv),
                size: 2,
            },
            VertexAttribute {
                attribute_type: VertexAttributeType::Float,
                offset: offset_of!(VertexPositionTextureLightmapAtlas, lightmap_uv),
                size: 2,
            },
            VertexAttribute {
                attribute_type: VertexAttributeType::Float,
                offset: offset_of!(VertexPositionTextureLightmapAtlas, lightmap_atlas),
                size: 4,
            },
        ]
    }
}

#[derive(Debug, Clone)]
#[repr(C)]
pub struct VertexPositionTextureSkinned {
    pub position: Vector3<f32>,
    pub uv: Vector2<f32>,
    pub bone_indices: [u32; 4],
}

impl Vertex for VertexPositionTextureSkinned {
    fn get_total_size() -> isize {
        size_of::<VertexPositionTextureSkinned>() as isize
    }

    fn get_vertex_attributes() -> Vec<VertexAttribute> {
        vec![
            VertexAttribute {
                attribute_type: VertexAttributeType::Float,
                offset: offset_of!(VertexPositionTextureSkinned, position),
                size: 3,
            },
            VertexAttribute {
                attribute_type: VertexAttributeType::Float,
                offset: offset_of!(VertexPositionTextureSkinned, uv),
                size: 2,
            },
            VertexAttribute {
                attribute_type: VertexAttributeType::Int,
                offset: offset_of!(VertexPositionTextureSkinned, bone_indices),
                size: 4,
            },
        ]
    }
}

pub enum VertexAttributeType {
    Float,
    NormalizedFloat,
    Int,
}

pub struct VertexAttribute {
    pub attribute_type: VertexAttributeType,
    pub offset: usize,
    pub size: types::GLint,
}

pub trait Vertex {
    fn get_total_size() -> isize;
    fn get_vertex_attributes() -> Vec<VertexAttribute>;
}

impl Vertex for VertexPositionTexture {
    fn get_total_size() -> isize {
        size_of::<VertexPositionTexture>() as isize
    }

    fn get_vertex_attributes() -> Vec<VertexAttribute> {
        vec![
            VertexAttribute {
                attribute_type: VertexAttributeType::Float,
                offset: offset_of!(VertexPositionTexture, position),
                size: 3,
            },
            VertexAttribute {
                attribute_type: VertexAttributeType::Float,
                offset: offset_of!(VertexPositionTexture, uv),
                size: 2,
            },
        ]
    }
}
