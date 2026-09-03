pub mod scene;
pub use scene::{LegacyScene, Scene};

/// Number of base joint slots in the skinning palette.
pub const MAX_SKINNED_JOINTS: usize = 40;
/// Full skinning-palette size: base joints in `0..MAX_SKINNED_JOINTS`, one
/// auxiliary "parent frame" per joint in the upper half (blended by stretchy
/// vertices). The shader-side `bone_matrices` array literals mirror this.
pub const SKINNING_PALETTE_SIZE: usize = 2 * MAX_SKINNED_JOINTS;

pub mod light;
pub use light::{Light, LightType, SpotLight};

pub mod light_system;
pub use light_system::LightSystem;

mod skinned_material;
pub use skinned_material::*;

mod particles;
pub use particles::*;

mod billboard_material;
pub use billboard_material::*;

pub mod scene_object;
pub use scene_object::{FrontFaceWinding, RenderLayer, SceneObject, SceneObjectDebugTag};

pub mod renderable;
pub use renderable::{
    Renderable, TransformSceneObject, create_transform_group, flatten_renderables,
    scene_objects_to_renderables,
};

pub mod geometry;
pub use geometry::Geometry;

pub mod cube;
pub use cube::Cube;

pub mod quad;
pub use quad::{Quad, QuadUv, create as create_quad, create_with_uv as create_quad_with_uv};

pub mod quad_unit;
pub use quad_unit::{QuadUnit, create as create_quad_unit};

pub mod plane;
pub use plane::{Plane, create_with_uv_scale as create_plane_with_uv_scale};

pub mod cube_indexed;
pub use cube_indexed::CubeIndexed;

pub mod mesh;
pub use mesh::Mesh;

pub mod indexed_mesh;
pub use indexed_mesh::IndexedMesh;

pub mod lines_mesh;
pub use lines_mesh::LinesMesh;

pub mod material;
pub use material::Material;

pub mod vertex;
pub use vertex::*;

pub mod basic_material;
pub use basic_material::BasicMaterial;

pub mod color_material;
pub use color_material::ColorMaterial;

pub mod vignette_material;
pub use vignette_material::VignetteMaterial;

pub mod debug_normal_material;
pub use debug_normal_material::{
    DebugNormalMaterial, DebugNormalSkinnedMaterial, create as create_debug_normal_material,
    create_skinned as create_skinned_debug_normal_material,
};

pub mod clipped_screen_material;
pub use clipped_screen_material::ClippedScreenMaterial;
