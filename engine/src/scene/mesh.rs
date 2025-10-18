extern crate gl;
use self::gl::types::*;
pub use crate::scene::Geometry;
pub use crate::scene::VertexPositionTexture;
use std::mem;
use std::os::raw::c_void;

use super::Vertex;
use super::VertexAttributeType;

#[derive(Clone)]
pub struct Mesh {
    pub vbo: GLuint,
    pub vao: GLuint,
    pub ebo: GLuint,

    pub triangle_count: i32,
}

// use std::backtrace::Backtrace;

pub fn create<T: Vertex>(raw_vertices: Vec<T>) -> Mesh {
    let triangle_count = (raw_vertices.len()) as i32;

    // let vertices: Vec<f32> = raw_vertices
    //     .into_iter()
    //     .flat_map(|v| {
    //         let verts: [f32; 5] = [v.position.x, v.position.y, v.position.z, v.uv.x, v.uv.y];
    //         verts
    //     })
    //     .collect();

    // TODO
    let indices = [0, 1, 2];

    let (mut vbo, mut vao, mut ebo) = (0, 0, 0);
    unsafe {
        gl::GenVertexArrays(1, &mut vao);
        gl::GenBuffers(1, &mut vbo);
        gl::GenBuffers(1, &mut ebo);
        // bind the Vertex Array Object first, then bind and set vertex buffer(s), and then configure vertex attributes(s).
        gl::BindVertexArray(vao);

        gl::BindBuffer(gl::ARRAY_BUFFER, vbo);
        let size = <T>::get_total_size() as usize;
        let total_size = raw_vertices.len() * size;
        //let size = (raw_vertices.len() * size_of::<VertexPositionTexture>()) as isize;
        let data = &raw_vertices[0] as *const T as *const c_void;

        gl::BufferData(gl::ARRAY_BUFFER, total_size as isize, data, gl::STATIC_DRAW);

        gl::BindBuffer(gl::ELEMENT_ARRAY_BUFFER, ebo);
        gl::BufferData(
            gl::ELEMENT_ARRAY_BUFFER,
            (indices.len() * mem::size_of::<GLfloat>()) as GLsizeiptr,
            &indices[0] as *const i32 as *const c_void,
            gl::STATIC_DRAW,
        );
        let size = size as i32;

        let attributes = <T>::get_vertex_attributes();
        let attr_len = attributes.len();

        for i in 0..attr_len {
            let attr = &attributes[i];
            match attr.attribute_type {
                VertexAttributeType::Float => {
                    gl::VertexAttribPointer(
                        i as u32,
                        attr.size,
                        gl::FLOAT,
                        gl::FALSE,
                        size,
                        attr.offset as *const c_void,
                    );
                }
                VertexAttributeType::NormalizedFloat => {
                    gl::VertexAttribPointer(
                        i as u32,
                        attr.size,
                        gl::FLOAT,
                        gl::TRUE,
                        size,
                        attr.offset as *const c_void,
                    );
                    gl::EnableVertexAttribArray(i as u32);
                }
                VertexAttributeType::Int => {
                    gl::VertexAttribIPointer(
                        i as u32,
                        attr.size,
                        gl::INT,
                        size,
                        attr.offset as *const c_void,
                    );
                    gl::EnableVertexAttribArray(i as u32);
                }
            }
            gl::EnableVertexAttribArray(i as u32);
        }

        // note that this is allowed, the call to gl::VertexAttribPointer registered VBO as the vertex attribute's bound vertex buffer object so afterwards we can safely unbind
        gl::BindBuffer(gl::ARRAY_BUFFER, 0);
        gl::BindBuffer(gl::ELEMENT_ARRAY_BUFFER, 0);

        // You can unbind the VAO afterwards so other VAO calls won't accidentally modify this VAO, but this rarely happens. Modifying other
        // VAOs requires a call to glBindVertexArray anyways so we generally don't unbind VAOs (nor VBOs) when it's not directly necessary.
        gl::BindVertexArray(0);
    }

    // uncomment this call to draw in wireframe polygons.
    Mesh {
        triangle_count,
        vao,
        vbo,
        ebo,
    }
}

impl Geometry for Mesh {
    fn draw(&self) {
        unsafe {
            //gl::PolygonMode(gl::FRONT_AND_BACK, gl::LINE);
            gl::BindVertexArray(self.vao);
            //    gl::BindBuffer(gl::ELEMENT_ARRAY_BUFFER, self.ebo);
            gl::DrawArrays(gl::TRIANGLES, 0, self.triangle_count);
        }
    }
}

impl Drop for Mesh {
    fn drop(&mut self) {
        unsafe {
            gl::DeleteBuffers(1, &self.ebo);
            gl::DeleteBuffers(1, &self.vbo);
            gl::DeleteVertexArrays(1, &self.vao);
        }
    }
}
