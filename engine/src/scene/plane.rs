extern crate gl;
use self::gl::types::*;
pub use crate::scene::Geometry;
use std::mem;
use std::os::raw::c_void;
use std::ptr;

pub struct Plane {
    #[allow(dead_code)]
    vbo: GLuint,
    vao: GLuint,
    #[allow(dead_code)]
    ebo: GLuint,
}

pub fn create() -> Plane {
    let uv_scale = 100.0;
    let vertices: [f32; 30] = [
        -0.5, 0.0, -0.5, 0.0, 0.0, 0.5, 0.0, -0.5, uv_scale, 0.0, 0.5, 0.0, 0.5, uv_scale,
        uv_scale, -0.5, 0.0, -0.5, 0.0, 0.0, -0.5, 0.0, 0.5, 0.0, uv_scale, 0.5, 0.0, 0.5,
        uv_scale, uv_scale,
    ];
    let indices = [
        0, 1, 3, // first Triangle
        1, 2, 3, // second Triangle
    ];

    let (mut vbo, mut vao, mut ebo) = (0, 0, 0);
    unsafe {
        gl::GenVertexArrays(1, &mut vao);
        gl::GenBuffers(1, &mut vbo);
        gl::GenBuffers(1, &mut ebo);
        // bind the Vertex Array Object first, then bind and set vertex buffer(s), and then configure vertex attributes(s).
        gl::BindVertexArray(vao);

        gl::BindBuffer(gl::ARRAY_BUFFER, vbo);
        gl::BufferData(
            gl::ARRAY_BUFFER,
            (vertices.len() * mem::size_of::<GLfloat>()) as GLsizeiptr,
            &vertices[0] as *const f32 as *const c_void,
            gl::STATIC_DRAW,
        );

        gl::BindBuffer(gl::ELEMENT_ARRAY_BUFFER, ebo);
        gl::BufferData(
            gl::ELEMENT_ARRAY_BUFFER,
            (indices.len() * mem::size_of::<GLfloat>()) as GLsizeiptr,
            &indices[0] as *const i32 as *const c_void,
            gl::STATIC_DRAW,
        );

        gl::VertexAttribPointer(
            0,
            3,
            gl::FLOAT,
            gl::FALSE,
            5 * mem::size_of::<GLfloat>() as GLsizei,
            ptr::null(),
        );
        gl::EnableVertexAttribArray(0);
        gl::VertexAttribPointer(
            1,
            2,
            gl::FLOAT,
            gl::FALSE,
            5 * mem::size_of::<GLfloat>() as GLsizei,
            (3 * mem::size_of::<GLfloat>() as GLsizei) as *const c_void,
        );
        gl::EnableVertexAttribArray(1);

        // note that this is allowed, the call to gl::VertexAttribPointer registered VBO as the vertex attribute's bound vertex buffer object so afterwards we can safely unbind
        gl::BindBuffer(gl::ARRAY_BUFFER, 0);
        gl::BindBuffer(gl::ELEMENT_ARRAY_BUFFER, 0);

        // You can unbind the VAO afterwards so other VAO calls won't accidentally modify this VAO, but this rarely happens. Modifying other
        // VAOs requires a call to glBindVertexArray anyways so we generally don't unbind VAOs (nor VBOs) when it's not directly necessary.
        gl::BindVertexArray(0);
    }

    // uncomment this call to draw in wireframe polygons.
    // gl::PolygonMode(gl::FRONT_AND_BACK, gl::LINE);
    Plane {
        vao: vao,
        vbo: vbo,
        ebo: ebo,
    }
}

impl Geometry for Plane {
    fn draw(&self) {
        unsafe {
            gl::BindVertexArray(self.vao);
            //    gl::BindBuffer(gl::ELEMENT_ARRAY_BUFFER, self.ebo);
            gl::DrawArrays(gl::TRIANGLES, 0, 6);
        }
    }
}
