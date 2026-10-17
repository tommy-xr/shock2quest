use crate::render_log;
use gl::types;
use gl::types::*;
use std::ffi::CString;
use std::ptr;

pub struct Shader {
    pub gl_id: types::GLuint,
}

impl Drop for Shader {
    fn drop(&mut self) {
        render_log!(DEBUG, "Deleting shader: {}", self.gl_id);
        unsafe { gl::DeleteShader(self.gl_id) }
    }
}

pub enum ShaderType {
    Fragment,
    Vertex,
}

pub fn build(shader_contents: &str, shader_type: ShaderType, is_es: bool) -> Shader {
    let (gl_shader_type, gl_shader_description) = match shader_type {
        ShaderType::Fragment => (gl::FRAGMENT_SHADER, "FRAGMENT"),
        ShaderType::Vertex => (gl::VERTEX_SHADER, "VERTEX"),
    };
    let multiview = is_es && crate::multiview::is_enabled();
    let vertex = matches!(shader_type, ShaderType::Vertex);

    let shader;
    unsafe {
        let mut success = 0;
        let converted_fragment =
            convert(shader_contents, is_es, multiview, vertex).expect("Error compiling shader.");
        shader = gl::CreateShader(gl_shader_type);
        let c_str_frag = CString::new(converted_fragment.as_bytes()).unwrap();
        let mut info_log = vec![0u8; 512 - 1]; // Initialize with zeros
        gl::ShaderSource(shader, 1, &c_str_frag.as_ptr(), ptr::null());
        gl::CompileShader(shader);
        // check for shader compile errors
        gl::GetShaderiv(shader, gl::COMPILE_STATUS, &mut success);
        if success != gl::TRUE as GLint {
            gl::GetShaderInfoLog(
                shader,
                512,
                ptr::null_mut(),
                info_log.as_mut_ptr() as *mut GLchar,
            );
            render_log!(
                ERROR,
                "Shader compilation failed for {}: {}",
                gl_shader_description,
                String::from_utf8_lossy(&info_log)
            );
        }
    }

    Shader { gl_id: shader }
}

/**
 * convert converts an agnostic shader to either 320 es or 410; `multiview`
 * (GLES only) adds the OVR_multiview2 header and per-eye view/projection.
 */
fn convert(
    shader: &str,
    is_opengl_es: bool,
    multiview: bool,
    vertex: bool,
) -> std::result::Result<String, Box<dyn std::error::Error>> {
    let version = if is_opengl_es {
        "#version 320 es"
    } else {
        "#version 410"
    };

    // Compatibility context for shader
    let preamble: &str = r#"
            #ifndef GL_ES
            #define highp
            #else
            precision mediump float;
            #endif
    "#;

    if multiview {
        let shader = crate::multiview::rewrite_view_uniforms(shader);
        let header = crate::multiview::shader_header(vertex);
        return Ok([version, header, preamble, &shader].join("\n"));
    }

    let out = [version, preamble, shader].join("\n");

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::convert;

    const VERTEX: &str = "uniform highp mat4 view;\nuniform mat4 projection;\nvoid main() {}";

    #[test]
    fn single_view_output_is_unchanged() {
        let preamble = r#"
            #ifndef GL_ES
            #define highp
            #else
            precision mediump float;
            #endif
    "#;
        for (es, version) in [(true, "#version 320 es"), (false, "#version 410")] {
            let out = convert(VERTEX, es, false, true).unwrap();
            assert_eq!(out, [version, preamble, VERTEX].join("\n"));
        }
    }

    #[test]
    fn multiview_header_precedes_every_declaration() {
        let out = convert(VERTEX, true, true, true).unwrap();
        let mut lines = out.lines();
        assert_eq!(lines.next(), Some("#version 320 es"));
        assert_eq!(lines.next(), Some("#extension GL_OVR_multiview2 : require"));
        assert_eq!(lines.next(), Some("#define MULTIVIEW 1"));
        assert_eq!(lines.next(), Some("layout(num_views = 2) in;"));
        assert!(out.find("precision").unwrap() > out.find("num_views").unwrap());
        assert!(out.contains("uniform highp mat4 view[2];"));

        let fragment = convert("void main() {}", true, true, false).unwrap();
        assert!(fragment.contains("#extension GL_OVR_multiview2 : require"));
        assert!(!fragment.contains("num_views"));
    }
}
