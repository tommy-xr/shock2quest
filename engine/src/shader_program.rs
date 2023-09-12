use gl::types::*;
use std::ptr;
use tracing::warn;

pub struct ShaderProgram {
    pub gl_id: u32,
}

impl Drop for ShaderProgram {
    fn drop(&mut self) {
        //println!("Deleting shader program: {}", self.gl_id);
        unsafe { gl::DeleteProgram(self.gl_id) }
    }
}

use crate::shader::*;

pub fn link(vertex_shader: &Shader, fragment_shader: &Shader) -> ShaderProgram {
    let mut success = 0;
    let mut info_log = Vec::with_capacity(512);
    let shader_program;
    unsafe {
        info_log.set_len(512 - 1); // subtract 1 to skip the trailing null character
        shader_program = gl::CreateProgram();
        gl::AttachShader(shader_program, vertex_shader.gl_id);
        gl::AttachShader(shader_program, fragment_shader.gl_id);
        gl::LinkProgram(shader_program);
        // check for linking errors
        gl::GetProgramiv(shader_program, gl::LINK_STATUS, &mut success);
        if success != gl::TRUE as GLint {
            gl::GetProgramInfoLog(
                shader_program,
                512,
                ptr::null_mut(),
                info_log.as_mut_ptr() as *mut GLchar,
            );
            warn!(
                "ERROR::SHADER::PROGRAM::COMPILATION_FAILED\n{}",
                String::from_utf8_lossy(&info_log)
            );
        }
    }

    ShaderProgram {
        gl_id: shader_program,
    }
}
