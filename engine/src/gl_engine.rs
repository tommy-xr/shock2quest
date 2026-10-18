extern crate gl;

use crate::util;
use std::sync::Arc;

pub struct OpenGLEngine {
    self_depth: std::cell::RefCell<crate::scene::self_depth::Target>,
    pub is_opengl_es: bool,
    pub storage: Arc<dyn crate::file_system::Storage>,
}

impl OpenGLEngine {
    /// Draws `objects` in explicit composition-layer order: opaque then
    /// transparent per layer. `self_depth_scene` enables self-occluding
    /// translucent groups; offscreen passes draw without them.
    unsafe fn draw_layers(
        &self,
        render_context: &EngineRenderContext,
        view: &cgmath::Matrix4<f32>,
        objects: &[crate::scene::SceneObject],
        lights: &crate::scene::light::LightArray,
        self_depth_scene: Option<&Scene>,
    ) {
        let in_layer = |layer: RenderLayer| {
            objects
                .iter()
                .filter(move |object| object.render_layer() == layer)
        };
        unsafe {
            // Composition is keyed explicitly: debug/desktop append per-eye
            // objects while Oculus prepends them, but both must render world,
            // scene effects, scene UI, then system UI. Each non-world layer
            // starts with one depth clear at its group boundary, never from an
            // individual object mid-pass.
            for layer in RenderLayer::ORDERED {
                if in_layer(layer).next().is_none() {
                    continue;
                }
                if layer.clears_depth() {
                    gl::Clear(gl::DEPTH_BUFFER_BIT);
                }

                // SINGLE-PASS LIGHTING: Opaque pass with all lighting calculated
                // in shaders.
                in_layer(layer).for_each(|s| {
                    // An object's own lights don't replace the scene's - the
                    // player's hand lights live there, and a torch pointed at a
                    // prop has to light it.
                    let merged = s.lights().map(|o| o.merged_with(lights));
                    s.draw_opaque(
                        self,
                        render_context,
                        view,
                        merged.as_ref().unwrap_or(lights),
                    )
                });

                // Transparent pass with all lighting calculated in shaders
                gl::DepthMask(gl::FALSE);
                // Preserve authored composition order. A mask belongs to one
                // actor only; rebuilding for another actor never touches scene depth.
                let mut cached_mask = None;
                for s in in_layer(layer) {
                    let mask = s
                        .self_depth_group
                        .zip(self_depth_scene)
                        .map(|(group, scene)| {
                            if cached_mask.is_none_or(|(id, _)| id != group) {
                                cached_mask = Some((
                                    group,
                                    self.self_depth.borrow_mut().render(
                                        group,
                                        layer,
                                        self,
                                        render_context,
                                        view,
                                        scene,
                                    ),
                                ));
                            }
                            cached_mask.unwrap().1
                        });
                    let context = EngineRenderContext {
                        self_depth: mask.map_or(
                            crate::scene::self_depth::Phase::None,
                            crate::scene::self_depth::Phase::Test,
                        ),
                        ..*render_context
                    };
                    let merged = s.lights().map(|o| o.merged_with(lights));
                    s.draw_transparent(self, &context, view, merged.as_ref().unwrap_or(lights));
                }
                gl::DepthMask(gl::TRUE);
            }
        }
    }
}

fn init(is_opengl_es: bool, storage: Arc<dyn crate::file_system::Storage>) -> OpenGLEngine {
    OpenGLEngine {
        self_depth: Default::default(),
        is_opengl_es,
        storage,
    }
}

use crate::engine::Engine;
use crate::engine::EngineRenderContext;
use crate::scene::{RenderLayer, scene::Scene};

/// Depth and blend state shared by every pass.
unsafe fn set_render_state() {
    unsafe {
        gl::Enable(gl::DEPTH_TEST);
        gl::Enable(gl::BLEND);
        // Shaders emit straight RGB; the framebuffer accumulates
        // premultiplied RGB. Preserve coverage alpha for the Quest
        // passthrough compositor instead of multiplying alpha by itself.
        gl::BlendFuncSeparate(
            gl::SRC_ALPHA,
            gl::ONE_MINUS_SRC_ALPHA,
            gl::ONE,
            gl::ONE_MINUS_SRC_ALPHA,
        );
    }
}

impl OpenGLEngine {
    /// Draws each of `scene`'s on-screen offscreen passes from
    /// `render_context`'s eye and records on its target how materials sample it.
    unsafe fn draw_offscreen_passes(&self, render_context: &EngineRenderContext, scene: &Scene) {
        let view = util::compute_view_matrix_from_render_context(render_context);
        for pass in &scene.offscreen_passes {
            let Some(camera) = crate::scene::pass_focus::pass_camera(
                view,
                render_context.projection_matrix,
                pass.camera_translation,
                pass.focus,
            ) else {
                continue;
            };
            let size = pass.target.size();
            let context = EngineRenderContext {
                camera_offset: render_context.camera_offset + pass.camera_translation,
                projection_matrix: camera.projection,
                screen_size: cgmath::vec2(size[0] as f32, size[1] as f32),
                ..*render_context
            };
            unsafe {
                pass.target.bind();
                gl::ClearColor(0.0, 0.0, 0.0, 1.0);
                gl::Clear(gl::COLOR_BUFFER_BIT | gl::DEPTH_BUFFER_BIT);
                // Scene lights carry the hand lights, which reach the far side too.
                self.draw_layers(&context, &camera.view, &pass.objects, scene.lights(), None);
                pass.target.unbind();
            }
            pass.target.set_sampling(camera.sampling);
        }
    }
}

impl Engine for OpenGLEngine {
    fn render_offscreen_passes(&self, render_context: &EngineRenderContext, scene: &Scene) {
        unsafe {
            set_render_state();
            self.draw_offscreen_passes(render_context, scene);
        }
    }

    fn get_storage(&self) -> Arc<dyn crate::file_system::Storage> {
        Arc::clone(&self.storage)
    }

    fn render(&self, render_context: &EngineRenderContext, scene: &Scene) {
        // let version =
        // let convertedVertex = shader::convert(vertexShaderSource, self.isES).unwrap();
        // let convertedFragment = shader::convert(fragmentShaderSource, self.isES).unwrap();

        unsafe {
            set_render_state();
            // Offscreen passes first, so materials in the main pass can sample
            // this frame's result.
            self.draw_offscreen_passes(render_context, scene);

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

            let view = util::compute_view_matrix_from_render_context(render_context);

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

            self.draw_layers(
                render_context,
                &view,
                &scene.objects,
                scene.lights(),
                Some(scene),
            );

            //cube.destroy();
        }
    }
}

pub fn init_gl() -> OpenGLEngine {
    let storage = create_desktop_storage();
    init(false, storage)
}

pub fn init_gles() -> OpenGLEngine {
    let storage = create_desktop_storage();

    init(true, storage)
}

fn create_desktop_storage() -> Arc<dyn crate::file_system::Storage> {
    let bundle_file_system = Box::new(crate::file_system::DefaultFileSystem {
        root_path: Box::new(std::path::Path::new("./assets/")),
    });

    crate::file_system::storage::init(bundle_file_system)
}

#[cfg(target_os = "android")]
pub fn init_android() -> OpenGLEngine {
    let bundle_file_system = Box::new(crate::file_system::android_file_system::init());
    let storage = crate::file_system::storage::init(bundle_file_system);

    init(true, storage)
}
