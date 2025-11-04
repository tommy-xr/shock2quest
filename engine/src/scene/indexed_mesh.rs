extern crate gl;
use self::gl::types::*;
pub use crate::scene::Geometry;
use std::mem;
use std::os::raw::c_void;

use super::Vertex;
use super::VertexAttributeType;

#[derive(Clone)]
pub struct IndexedMesh {
    pub vbo: GLuint,
    pub vao: GLuint,
    pub ebo: GLuint,
    pub index_count: i32,
}

pub fn create<T: Vertex>(raw_vertices: Vec<T>, indices: Vec<u32>) -> IndexedMesh {
    let index_count = indices.len() as i32;

    let (mut vbo, mut vao, mut ebo) = (0, 0, 0);
    unsafe {
        gl::GenVertexArrays(1, &mut vao);
        gl::GenBuffers(1, &mut vbo);
        gl::GenBuffers(1, &mut ebo);

        // Bind the Vertex Array Object first
        gl::BindVertexArray(vao);

        // Bind and set vertex buffer
        gl::BindBuffer(gl::ARRAY_BUFFER, vbo);
        let size = <T>::get_total_size() as usize;
        let total_size = raw_vertices.len() * size;
        let data = &raw_vertices[0] as *const T as *const c_void;
        gl::BufferData(gl::ARRAY_BUFFER, total_size as isize, data, gl::STATIC_DRAW);

        // Bind and set index buffer
        gl::BindBuffer(gl::ELEMENT_ARRAY_BUFFER, ebo);
        gl::BufferData(
            gl::ELEMENT_ARRAY_BUFFER,
            (indices.len() * mem::size_of::<u32>()) as GLsizeiptr,
            &indices[0] as *const u32 as *const c_void,
            gl::STATIC_DRAW,
        );

        // Set up vertex attributes
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

        // Unbind buffers
        gl::BindBuffer(gl::ARRAY_BUFFER, 0);
        gl::BindVertexArray(0);
        // Note: Do NOT unbind EBO while VAO is bound
    }

    IndexedMesh {
        index_count,
        vao,
        vbo,
        ebo,
    }
}

impl Geometry for IndexedMesh {
    fn draw(&self) {
        unsafe {
            gl::BindVertexArray(self.vao);
            // Use DrawElements instead of DrawArrays to use the index buffer
            gl::DrawElements(
                gl::TRIANGLES,
                self.index_count,
                gl::UNSIGNED_INT,
                std::ptr::null(),
            );
        }
    }
}

impl Drop for IndexedMesh {
    fn drop(&mut self) {
        unsafe {
            gl::DeleteBuffers(1, &self.ebo);
            gl::DeleteBuffers(1, &self.vbo);
            gl::DeleteVertexArrays(1, &self.vao);
        }
    }
}
