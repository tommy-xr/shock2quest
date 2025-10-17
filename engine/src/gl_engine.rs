extern crate gl;

use crate::texture_descriptor::TextureDescriptor;
use crate::util;
use std::cell::RefCell;
use std::rc::Rc;

pub struct OpenGLEngine {
    pub is_opengl_es: bool,
    pub storage: Box<dyn crate::file_system::Storage>,
    #[allow(dead_code)]
    texture_descriptor: Rc<RefCell<Box<dyn TextureDescriptor>>>,
}

impl OpenGLEngine {}

fn init(is_opengl_es: bool, storage: Box<dyn crate::file_system::Storage>) -> OpenGLEngine {
    let texture = Rc::new(RefCell::new(
        crate::texture_descriptor::FilePathTextureDescriptor::new(
            "texture_07.png".to_owned(),
            Box::new(crate::texture_format::PNG),
        ),
    ));
    OpenGLEngine {
        is_opengl_es,
        storage,
        texture_descriptor: texture,
    }
}

use crate::engine::Engine;
use crate::engine::EngineRenderContext;
use crate::scene::scene::Scene;

impl Engine for OpenGLEngine {
    fn get_storage(&self) -> &Box<dyn crate::file_system::Storage> {
        &self.storage
    }

    fn render(&self, render_context: &EngineRenderContext, scene: &Scene) {
        // let version =
        // let convertedVertex = shader::convert(vertexShaderSource, self.isES).unwrap();
        // let convertedFragment = shader::convert(fragmentShaderSource, self.isES).unwrap();

        unsafe {
            gl::Enable(gl::DEPTH_TEST);
            gl::Enable(gl::BLEND);
            // gl::Enable(gl::CULL_FACE);
            // gl::FrontFace(gl::CW);
            gl::BlendFunc(gl::SRC_ALPHA, gl::ONE_MINUS_SRC_ALPHA);
            gl::ClearColor(0.0, 0.0, 0.0, 0.0);
            gl::Clear(gl::COLOR_BUFFER_BIT | gl::DEPTH_BUFFER_BIT);

            // WIREFRAME: Uncomment this to see wireframe in desktop
            // gl::PolygonMode(gl::FRONT_AND_BACK, gl::LINE);

            // let time = render_context.time;
            // create transformations
            // let mut transform: Matrix4<f32> = Matrix4::identity();
            // //transform = transform * Matrix4::<f32>::from_translation(vec3(0.0, 0.5, 0.0));
            // transform = transform * Matrix4::<f32>::from_angle_z(Rad(time));
            // transform = transform * Matrix4::<f32>::from_angle_y(Rad(time * 2.0));

            // let camera_rotation: Matrix4<f32> = Matrix4::from(render_context.camera_rotation);
            // let camera_translation = Matrix4::<f32>::from_translation(render_context.camera_offset);
            // let camera = (camera_translation * camera_rotation).invert().unwrap();

            // let head_rotation = Matrix4::from(render_context.head_rotation);
            // let head_offset = Matrix4::<f32>::from_translation(render_context.head_offset);
            // let head = (head_offset * head_rotation).invert().unwrap();

            // let view = head * camera;

            let view = util::compute_view_matrix_from_render_context(&render_context);

            //let view = camera_translation * camera_rotation * head_rotation;

            // let cube: Rc<Box<dyn crate::scene::Geometry>> =
            //     Rc::new(Box::new(crate::scene::cube::create()));
            // let cube2: Rc<Box<dyn crate::scene::Geometry>> =
            //     Rc::new(Box::new(crate::scene::cube_indexed::create()));
            // let plane: Rc<Box<dyn crate::scene::Geometry>> =
            //     Rc::new(Box::new(crate::scene::plane::create()));
            // let mut cm = crate::scene::color_material::create();
            // cm.initialize(self.is_opengl_es, &self.storage);
            // let color_material = RefCell::new(cm);

            // let tm1 = RefCell::new(crate::scene::basic_material::create(
            //     self.texture_descriptor.clone(),
            // ));
            // let tm2 = RefCell::new(crate::scene::basic_material::create(
            //     self.texture_descriptor.clone(),
            // ));
            // let tm3 = RefCell::new(crate::scene::basic_material::create(
            //     self.texture_descriptor.clone(),
            // ));
            // let mut scene_object1 = crate::scene::scene_object::create(tm1, cube.clone());
            // let mut scene_object2 = crate::scene::scene_object::create(tm2, cube2);
            // let mut scene_object3 =
            //     crate::scene::scene_object::create(color_material, cube.clone());

            // //cube.init();
            // scene_object1.set_transform(transform);
            // //scene_object1.draw(&self, render_context, &view);

            // transform = transform * Matrix4::<f32>::from_translation(vec3(0.0, 2.0, 0.0));
            // scene_object2.set_transform(transform);
            // scene_object2.draw(&self, render_context, &view);

            // transform = transform * Matrix4::<f32>::from_translation(vec3(0.0, 2.0, 0.0));
            // scene_object3.set_transform(Matrix4::from_translation(
            //     render_context.camera_offset - vec3(0.0, 2.0, 0.0),
            // ));
            // scene_object3.draw(&self, render_context, &view);

            // let mut floor = crate::scene::scene_object::create(tm3, plane.clone());
            // floor.set_transform(
            //     Matrix4::from_translation(vec3(0.0, -10., 0.0)) * Matrix4::<f32>::from_scale(100.),
            // );
            // // floor.set_transform(
            // //     Matrix4::<f32>::from_scale(100.0) * Matrix4::from_translation(vec3(0.0, -1., 0.0)),
            // // );
            // floor.draw(&self, render_context, &view);

            // STEP 1: Base opaque pass (no lighting, just base material properties)
            scene
                .iter()
                .for_each(|s| s.draw_opaque(self, render_context, &view));

            // STEP 2: Multi-pass lighting for opaque objects
            if scene.light_count() > 0 {
                // Setup additive blending for light accumulation
                gl::Enable(gl::BLEND);
                gl::BlendFunc(gl::ONE, gl::ONE); // Additive blending
                gl::DepthMask(gl::FALSE); // Read-only depth testing
                gl::DepthFunc(gl::EQUAL); // Only render pixels with exact depth match

                // Render one pass per light
                for light in scene.lights().lights() {
                    scene.iter().for_each(|scene_object| {
                        // Check if this light affects this object's position
                        // For now, use object's transform position as a simple check
                        let world_pos = scene_object.get_world_position();
                        if light.affects_position(world_pos) {
                            scene_object.draw_light_pass(
                                self,
                                render_context,
                                &view,
                                light.as_ref(),
                                None, // No shadow map yet
                            );
                        }
                    });
                }

                // Restore normal blend state
                gl::BlendFunc(gl::SRC_ALPHA, gl::ONE_MINUS_SRC_ALPHA);
                gl::DepthFunc(gl::LESS); // Restore normal depth testing
                gl::DepthMask(gl::TRUE); // Re-enable depth writes
            }

            gl::DepthMask(gl::FALSE);

            // STEP 3: Transparent pass
            // TODO: Properly order back-to-front..
            // TODO: Add lighting support for transparent materials
            scene
                .iter()
                .for_each(|s| s.draw_transparent(self, render_context, &view));
            gl::DepthMask(gl::TRUE);

            //cube.destroy();
        }
    }
}

pub fn init_gl() -> OpenGLEngine {
    let storage = create_desktop_storage();
    init(false, storage)
}

pub fn init_gles() -> OpenGLEngine {
    let _file_system = Box::new(crate::file_system::DefaultFileSystem {
        root_path: Box::new(std::path::Path::new("../assets")),
    });
    let storage = create_desktop_storage();

    init(true, storage)
}

fn create_desktop_storage() -> Box<dyn crate::file_system::Storage> {
    let bundle_file_system = Box::new(crate::file_system::DefaultFileSystem {
        root_path: Box::new(std::path::Path::new("../assets/")),
    });
    let external_file_system = Box::new(crate::file_system::DefaultFileSystem {
        root_path: Box::new(std::path::Path::new("../../Data/")),
    });

    crate::file_system::storage::init(external_file_system, bundle_file_system)
}

#[cfg(target_os = "android")]
pub fn init_android() -> OpenGLEngine {
    let bundle_file_system = Box::new(crate::file_system::android_file_system::init());
    let external_file_system = Box::new(crate::file_system::DefaultFileSystem {
        root_path: Box::new(std::path::Path::new("/mnt/sdcard/shock2quest/")),
    });
    let storage = crate::file_system::storage::init(external_file_system, bundle_file_system);

    init(true, storage)
}
