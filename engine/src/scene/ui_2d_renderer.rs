use cgmath::{Matrix4, Vector3, vec3};

use super::{Material, Renderable, SceneObject, TransformSceneObject};
use crate::scene::quad_unit;

/// 2D UI coordinate system renderer that handles world positioning and coordinate flipping
pub struct UI2DRenderer {
    group: TransformSceneObject,
}

impl UI2DRenderer {
    /// Create a new 2D UI renderer
    ///
    /// # Arguments
    /// * `world_pos` - Position in 3D world space
    /// * `world_size` - Size of the 2D coordinate system (width, height)
    /// * `scale` - Scale factor from 2D pixels to world units
    /// * `flip_y` - Whether to flip Y-axis (useful for textures with top-left origin)
    pub fn new(world_pos: Vector3<f32>, world_size: (f32, f32), scale: f32, flip_y: bool) -> Self {
        Self::new_with_rotation(
            world_pos,
            world_size,
            scale,
            flip_y,
            cgmath::Quaternion::new(1.0, 0.0, 0.0, 0.0),
        )
    }

    /// Create a new 2D UI renderer with rotation
    ///
    /// # Arguments
    /// * `world_pos` - Position in 3D world space
    /// * `world_size` - Size of the 2D coordinate system (width, height)
    /// * `scale` - Scale factor from 2D pixels to world units
    /// * `flip_y` - Whether to flip Y-axis (useful for textures with top-left origin)
    /// * `rotation` - Rotation to apply to the entire 2D system
    pub fn new_with_rotation(
        world_pos: Vector3<f32>,
        world_size: (f32, f32),
        scale: f32,
        flip_y: bool,
        rotation: cgmath::Quaternion<f32>,
    ) -> Self {
        Self::new_with_flips(world_pos, world_size, scale, false, flip_y, rotation)
    }

    /// Create a new 2D UI renderer with full control over axis flipping
    ///
    /// # Arguments
    /// * `world_pos` - Position in 3D world space
    /// * `world_size` - Size of the 2D coordinate system (width, height)
    /// * `scale` - Scale factor from 2D pixels to world units
    /// * `flip_x` - Whether to flip X-axis (mirror horizontally)
    /// * `flip_y` - Whether to flip Y-axis (useful for textures with top-left origin)
    /// * `rotation` - Rotation to apply to the entire 2D system
    pub fn new_with_flips(
        world_pos: Vector3<f32>,
        world_size: (f32, f32),
        scale: f32,
        flip_x: bool,
        flip_y: bool,
        rotation: cgmath::Quaternion<f32>,
    ) -> Self {
        let mut group = TransformSceneObject::new();
        let x_scale = if flip_x { -scale } else { scale };
        let y_scale = if flip_y { -scale } else { scale };
        let (width, height) = world_size;

        let transform = Matrix4::from_translation(world_pos)
            * Matrix4::from(rotation)
            * Matrix4::from_nonuniform_scale(x_scale, y_scale, 1.0)
            * Matrix4::from_translation(vec3(-width / 2.0, -height / 2.0, 0.0)); // Center the coordinate system

        group.set_transform(transform);
        Self { group }
    }

    /// Add a rectangular object to the 2D UI
    ///
    /// # Arguments
    /// * `material` - Material to render with
    /// * `x, y` - Position in 2D coordinate system
    /// * `w, h` - Size in 2D coordinate system
    /// * `z` - Depth offset (negative values are closer to camera)
    pub fn add_rect(
        &mut self,
        material: Box<dyn Material>,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        z: f32,
    ) {
        let mut obj = SceneObject::new(material, Box::new(quad_unit::create()));
        let transform =
            Matrix4::from_translation(vec3(x, y, z)) * Matrix4::from_nonuniform_scale(w, h, 1.0);
        obj.set_transform(transform);
        self.group.add_scene_object(obj);
    }

    /// Render all objects in the 2D UI system
    pub fn render_objects(self) -> Vec<SceneObject> {
        self.group.render_objects()
    }
}
