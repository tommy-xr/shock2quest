extern crate gl;

use crate::render_log;
use crate::Font;
// pub trait SceneObject {
//     fn init(&self) -> ();
//     fn draw(&self) -> ();
//     fn destroy(&self) -> ();
// }
use crate::engine::EngineRenderContext;
use crate::texture::TextureTrait;
use cgmath::prelude::*;
use cgmath::vec2;
use cgmath::vec3;
use cgmath::vec4;
use cgmath::Matrix4;
use cgmath::Vector2;

pub use crate::scene::Geometry;
pub use crate::scene::Material;

use crate::gl_engine::OpenGLEngine;
use std::cell::RefCell;
use std::rc::Rc;

use super::basic_material;
use super::mesh;
use super::quad;
use super::skinned_material::SkinnedMaterial;
use super::TextVertex;
use crate::materials;

#[derive(Clone)]
pub struct SceneObject {
    pub material: Rc<RefCell<Box<dyn Material>>>,
    pub geometry: Rc<Box<dyn Geometry>>,
    pub transform: Matrix4<f32>,
    pub local_transform: Matrix4<f32>, //hack...
    pub skinning_data: [Matrix4<f32>; 40],
    pub depth_write: bool,
}

impl SceneObject {
    pub fn screen_space_quad2(
        texture: Rc<dyn TextureTrait>,
        position: Vector2<f32>,
        size: Vector2<f32>,
        opacity: f32,
    ) -> SceneObject {
        let mesh = quad::create();
        let material =
            materials::ScreenSpaceMaterial::create(texture, vec4(1.0, 1.0, 1.0, opacity));

        let xform = Matrix4::from_translation(vec3(position.x, position.y, 0.0))
            * Matrix4::from_nonuniform_scale(size.x, size.y, 1.0)
            * Matrix4::from_translation(vec3(0.5, 0.5, 0.0));
        let mut ret = Self::new(material, Box::new(mesh));
        ret.set_local_transform(xform);
        ret
    }
    pub fn screen_space_quad(
        texture: Rc<dyn TextureTrait>,
        position: Vector2<f32>,
        size: Vector2<f32>,
    ) -> SceneObject {
        let mesh = quad::create();
        let material = materials::ScreenSpaceMaterial::create(texture, vec4(1.0, 1.0, 1.0, 1.0));

        let xform = Matrix4::from_translation(vec3(position.x, position.y, 0.0))
            * Matrix4::from_nonuniform_scale(size.x, size.y, 1.0)
            * Matrix4::from_translation(vec3(0.5, 0.5, 0.0));
        let mut ret = Self::new(material, Box::new(mesh));
        ret.set_local_transform(xform);
        ret
    }
    pub fn screen_space_text(
        str: &str,
        font: Rc<Box<dyn Font>>,
        font_size: f32,
        transparency: f32,
        in_x: f32,
        in_y: f32,
    ) -> SceneObject {
        render_log!(DEBUG, "screen-space-text: |{}|{}", str, str.len());
        let multiplier = font_size / font.base_height();
        let adj_height = font_size;

        let mut x = in_x;
        let y = in_y;

        let mut vertices = Vec::new();
        for c in str.chars() {
            let a_info = font.get_character_info(c).unwrap();
            let _half_pixel = font.get_half_pixel();
            let min_uv_x = a_info.min_uv_x;
            let max_uv_x = a_info.max_uv_x;

            // For screen space rendering - the y uvs need to be flipped:
            let min_uv_y = a_info.max_uv_y;
            let max_uv_y = a_info.min_uv_y;

            let adj_width = a_info.advance * multiplier;

            vertices.extend(vec![
                TextVertex {
                    position: vec2(x, y),
                    uv: vec2(min_uv_x, max_uv_y),
                },
                TextVertex {
                    position: vec2(x, y + adj_height),
                    uv: vec2(min_uv_x, min_uv_y),
                },
                TextVertex {
                    position: vec2(x + adj_width, y + adj_height),
                    uv: vec2(max_uv_x, min_uv_y),
                },
                TextVertex {
                    position: vec2(x, y),
                    uv: vec2(min_uv_x, max_uv_y),
                },
                TextVertex {
                    position: vec2(x + adj_width, y + adj_height),
                    uv: vec2(max_uv_x, min_uv_y),
                },
                TextVertex {
                    position: vec2(x + adj_width, y),
                    uv: vec2(max_uv_x, max_uv_y),
                },
            ]);

            x += adj_width + multiplier;
        }

        let mesh = mesh::create(vertices);
        let material = materials::ScreenSpaceMaterial::create(
            font.get_texture().clone(),
            vec4(1.0, 1.0, 1.0, transparency),
        );
        Self::new(material, Box::new(mesh))
    }
    pub fn world_space_text(str: &str, font: Rc<Box<dyn Font>>, transparency: f32) -> SceneObject {
        let mut x = 0.0;
        let y = 1.0;

        let font_size = 0.045;
        let multiplier = font_size / font.base_height();
        let adj_height = font_size;

        let mut vertices = Vec::new();
        for c in str.chars() {
            let a_info = font.get_character_info(c).unwrap();
            let _half_pixel = font.get_half_pixel();
            let min_uv_x = a_info.min_uv_x;
            let min_uv_y = a_info.min_uv_y;
            let max_uv_x = a_info.max_uv_x;
            // + a_info.uv_width
            // - (half_pixel * 2.0);
            let max_uv_y = a_info.max_uv_y;
            // + a_info.uv_height
            // - (half_pixel * 2.0);

            let adj_width = a_info.advance * multiplier;

            vertices.extend(vec![
                TextVertex {
                    position: vec2(x, y),
                    uv: vec2(min_uv_x, max_uv_y),
                },
                TextVertex {
                    position: vec2(x, y + adj_height),
                    uv: vec2(min_uv_x, min_uv_y),
                },
                TextVertex {
                    position: vec2(x + adj_width, y + adj_height),
                    uv: vec2(max_uv_x, min_uv_y),
                },
                TextVertex {
                    position: vec2(x, y),
                    uv: vec2(min_uv_x, max_uv_y),
                },
                TextVertex {
                    position: vec2(x + adj_width, y + adj_height),
                    uv: vec2(max_uv_x, min_uv_y),
                },
                TextVertex {
                    position: vec2(x + adj_width, y),
                    uv: vec2(max_uv_x, max_uv_y),
                },
            ]);

            x += adj_width + multiplier;
        }

        let mesh = mesh::create(vertices);
        let material = basic_material::create(font.get_texture(), 1.0, transparency);
        Self::new(material, Box::new(mesh))
    }

    pub fn create(
        material: RefCell<Box<dyn Material>>,
        geometry: Rc<Box<dyn Geometry>>,
    ) -> SceneObject {
        let transform: Matrix4<f32> = Matrix4::identity();
        SceneObject {
            material: Rc::new(material),
            geometry,
            transform,
            local_transform: Matrix4::identity(),
            skinning_data: [Matrix4::identity(); 40],
            depth_write: true,
        }
    }

    pub fn draw_opaque(
        &self,
        engine_context: &OpenGLEngine,
        render_context: &EngineRenderContext,
        view: &Matrix4<f32>,
        lights: &crate::scene::light::LightArray,
    ) {
        if !self.material.borrow().has_initialized() {
            self.material
                .borrow_mut()
                .initialize(engine_context.is_opengl_es);
        }

        let xform = self.transform * self.local_transform;
        if !self.depth_write {
            unsafe { gl::DepthMask(gl::FALSE) };
        }

        if self.material.borrow().draw_opaque(
            render_context,
            view,
            &xform,
            &self.skinning_data,
            lights,
        ) {
            self.geometry.draw();
        }

        if !self.depth_write {
            unsafe { gl::DepthMask(gl::TRUE) };
        }
    }
    pub fn draw_transparent(
        &self,
        _engine_context: &OpenGLEngine,
        render_context: &EngineRenderContext,
        view: &Matrix4<f32>,
        lights: &crate::scene::light::LightArray,
    ) {
        let xform = self.transform * self.local_transform;
        if self.material.borrow().draw_transparent(
            render_context,
            view,
            &xform,
            &self.skinning_data,
            lights,
        ) {
            self.geometry.draw();
        }
    }

    /// Get the world position of this scene object from its transform matrix
    pub fn get_world_position(&self) -> cgmath::Vector3<f32> {
        let final_transform = self.transform * self.local_transform;
        cgmath::Vector3::new(
            final_transform[3][0],
            final_transform[3][1],
            final_transform[3][2],
        )
    }
    pub fn set_transform(&mut self, transform: Matrix4<f32>) {
        self.transform = transform;
    }

    pub fn set_local_transform(&mut self, transform: Matrix4<f32>) {
        self.local_transform = transform;
    }

    pub fn set_skinning_data(&mut self, skinning_data: [Matrix4<f32>; 40]) {
        self.skinning_data = skinning_data;
    }

    pub fn get_transform(&self) -> Matrix4<f32> {
        self.transform
    }

    pub fn new(material: Box<dyn Material>, geometry: Box<dyn Geometry>) -> SceneObject {
        SceneObject {
            material: Rc::new(RefCell::new(material)),
            geometry: Rc::new(geometry),
            transform: Matrix4::identity(),
            local_transform: Matrix4::identity(),
            skinning_data: [Matrix4::identity(); 40],
            depth_write: true,
        }
    }

    pub fn duplicate(&self) -> SceneObject {
        SceneObject {
            material: self.material.clone(),
            geometry: self.geometry.clone(),
            transform: self.transform,
            local_transform: self.local_transform,
            skinning_data: self.skinning_data,
            depth_write: self.depth_write,
        }
    }

    pub fn set_depth_write(&mut self, enabled: bool) {
        self.depth_write = enabled;
    }

    pub fn set_skinned_transparency(&mut self, transparency: Option<f32>) {
        if let Some(material) = self
            .material
            .borrow_mut()
            .as_any_mut()
            .downcast_mut::<SkinnedMaterial>()
        {
            match transparency {
                Some(value) => material.set_transparency_override(value),
                None => material.reset_transparency(),
            }
        }
    }
}
