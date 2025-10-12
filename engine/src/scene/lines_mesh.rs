extern crate gl;
use self::gl::types::*;
pub use crate::scene::Geometry;
pub use crate::scene::VertexPosition;
use std::mem::size_of;
use std::os::raw::c_void;

pub struct LinesMesh {
    pub vbo: GLuint,
    pub vao: GLuint,
    pub ebo: GLuint,

    pub index_count: i32,
}

pub fn create(raw_vertices: Vec<VertexPosition>) -> LinesMesh {
    let index_count = (raw_vertices.len()) as i32;

    let (mut VBO, mut VAO, mut EBO) = (0, 0, 0);
    unsafe {
        gl::GenVertexArrays(1, &mut VAO);
        gl::GenBuffers(1, &mut VBO);
        gl::GenBuffers(1, &mut EBO);
        // bind the Vertex Array Object first, then bind and set vertex buffer(s), and then configure vertex attributes(s).
        gl::BindVertexArray(VAO);
        let size = size_of::<VertexPosition>() as isize;
        let total_size = (raw_vertices.len() * size_of::<VertexPosition>()) as isize;
        let data = &raw_vertices[0] as *const VertexPosition as *const c_void;

        gl::BindBuffer(gl::ARRAY_BUFFER, VBO);
        gl::BufferData(gl::ARRAY_BUFFER, total_size, data, gl::STATIC_DRAW);

        gl::VertexAttribPointer(
            0,
            3,
            gl::FLOAT,
            gl::FALSE,
            size as i32,
            offset_of!(VertexPosition, position) as *const c_void,
        );
        gl::EnableVertexAttribArray(0);

        // note that this is allowed, the call to gl::VertexAttribPointer registered VBO as the vertex attribute's bound vertex buffer object so afterwards we can safely unbind
        gl::BindBuffer(gl::ARRAY_BUFFER, 0);

        // You can unbind the VAO afterwards so other VAO calls won't accidentally modify this VAO, but this rarely happens. Modifying other
        // VAOs requires a call to glBindVertexArray anyways so we generally don't unbind VAOs (nor VBOs) when it's not directly necessary.
        gl::BindVertexArray(0);
    }

    // uncomment this call to draw in wireframe polygons.
    LinesMesh {
        index_count,
        vao: VAO,
        vbo: VBO,
        ebo: EBO,
    }
}

impl Geometry for LinesMesh {
    fn draw(&self) {
        unsafe {
            //gl::PolygonMode(gl::FRONT_AND_BACK, gl::LINE);
            gl::BindVertexArray(self.vao);
            //    gl::BindBuffer(gl::ELEMENT_ARRAY_BUFFER, self.ebo);
            gl::DrawArrays(gl::LINES, 0, self.index_count);
        }
    }
}

impl Drop for LinesMesh {
    fn drop(&mut self) {
        unsafe {
            gl::DeleteBuffers(1, &self.ebo);
            gl::DeleteBuffers(1, &self.vbo);
            gl::DeleteVertexArrays(1, &self.vao);
        }
    }
}
