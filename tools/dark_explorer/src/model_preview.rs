//! 3D model preview: renders a `dark_viewer` scene with the game engine into
//! an offscreen FBO, shown in the preview pane as an egui texture, with an
//! orbit camera (drag to rotate, scroll to zoom) and the viewer's debug
//! skeleton/hitbox overlays.
//!
//! GL interop: eframe's glow backend owns the GL context; `init_raw_gl` points
//! the engine's raw `gl` function pointers at the same context via
//! `cc.get_proc_address`, so the engine can render while eframe's context is
//! current (it is, throughout `App::ui`). The engine draws into our own FBO
//! (its render clears whatever framebuffer is bound), and the FBO's color
//! texture is registered with egui as a native texture.

use std::ffi::CString;

use cgmath::{Quaternion, Rad, Rotation3, vec2, vec3};
use dark::importers::MODELS_IMPORTER;
use dark_viewer::scenes::{BinObjViewerScene, ToolScene};
use eframe::{egui, glow};
use engine::Engine;
use engine::assets::asset_cache::AssetCache;
use engine::assets::asset_paths::{AbstractAssetPath, AssetPath};
use engine::assets::bundle_asset_path::BundleAssetPath;

use crate::ui::quiet_catch;

/// Load the raw `gl` crate's function pointers from eframe's GL context.
/// Call once, in the `AppCreator` closure, before any engine rendering.
pub fn init_raw_gl(cc: &eframe::CreationContext<'_>) {
    if let Some(get_proc) = cc.get_proc_address.clone() {
        gl::load_with(move |symbol| match CString::new(symbol) {
            Ok(symbol) => get_proc(&symbol),
            Err(_) => std::ptr::null(),
        });
    }
}

/// Offscreen render target whose color texture egui displays.
struct OffscreenTarget {
    fbo: u32,
    depth: u32,
    size: [i32; 2],
    egui_texture: egui::TextureId,
}

pub struct ModelPreview {
    engine: Box<dyn Engine>,
    asset_cache: AssetCache,
    /// What the current scene (or error) was built for: (key, skeletons,
    /// hitboxes). Guards against rebuilding — or re-panicking — every frame.
    built_for: Option<(String, bool, bool)>,
    scene: Option<Box<dyn ToolScene>>,
    error: Option<String>,
    pub debug_skeletons: bool,
    pub debug_hit_boxes: bool,
    // Orbit camera around `target` (dark_viewer's parameterization: pitch 90
    // is horizontal, distance along the orbit radius).
    yaw: f32,
    pitch: f32,
    distance: f32,
    target: cgmath::Vector3<f32>,
    fbo: Option<OffscreenTarget>,
}

impl ModelPreview {
    /// Build the render host: the engine plus one asset cache over the same
    /// family mounts the game resolves models and textures through.
    pub fn new() -> ModelPreview {
        let engine = engine::opengl();
        let mut mounts: Vec<Box<dyn AbstractAssetPath>> = shock2vr::resource_families()
            .iter()
            .map(|family| shock2vr::resource_family_paths(family))
            .collect();
        // Engine bundle assets (the grid ground-plane texture).
        mounts.push(BundleAssetPath::new("".to_owned(), engine.get_storage()));
        let base_path = shock2vr::paths::data_root().to_string_lossy().into_owned();
        let asset_cache = AssetCache::new(base_path, AssetPath::combine(mounts));
        ModelPreview {
            engine,
            asset_cache,
            built_for: None,
            scene: None,
            error: None,
            debug_skeletons: false,
            debug_hit_boxes: false,
            yaw: 90.0,
            pitch: 90.0,
            distance: 10.0,
            target: vec3(0.0, 0.0, 0.0),
            fbo: None,
        }
    }

    /// Show the preview for `key` (a `.bin` model): toggles, then the rendered
    /// viewport filling the remaining space.
    pub fn show(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame, key: &str) {
        self.ensure_scene(key);
        if let Some(error) = &self.error {
            ui.label(format!("Cannot render this model: {error}"));
            return;
        }

        // A toggle change rebuilds on the next frame's ensure_scene (the pane
        // repaints continuously, so that is immediate in practice).
        ui.horizontal(|ui| {
            ui.checkbox(&mut self.debug_skeletons, "Skeleton");
            ui.checkbox(&mut self.debug_hit_boxes, "Hitboxes");
            ui.label("(drag to orbit, scroll to zoom)");
        });

        let available = ui.available_size();
        let size = egui::vec2(available.x.max(1.0), available.y.max(1.0));
        let response = ui.allocate_response(size, egui::Sense::drag());
        self.apply_camera_input(ui, &response);

        // Tick the scene with real time and keep repainting while shown, so an
        // animated scene advances even without input.
        let dt = ui.input(|i| i.stable_dt).min(0.1);
        if let Some(scene) = &mut self.scene {
            scene.update(dt);
        }
        ui.ctx().request_repaint();

        let pixels_per_point = ui.ctx().pixels_per_point();
        let px = [
            (size.x * pixels_per_point).round().max(1.0) as i32,
            (size.y * pixels_per_point).round().max(1.0) as i32,
        ];
        self.ensure_target(frame, px);
        self.render_scene(px);

        if let Some(target) = &self.fbo {
            // GL renders bottom-up; flip V so egui shows it upright.
            let uv = egui::Rect::from_min_max(egui::pos2(0.0, 1.0), egui::pos2(1.0, 0.0));
            ui.painter()
                .image(target.egui_texture, response.rect, uv, egui::Color32::WHITE);
        }
    }

    /// (Re)build the scene when the key or a debug toggle changed, framing the
    /// camera from the model's bounds where they are known.
    fn ensure_scene(&mut self, key: &str) {
        let wanted = (key.to_string(), self.debug_skeletons, self.debug_hit_boxes);
        if self.built_for.as_ref() == Some(&wanted) {
            return;
        }
        let reframe = self.built_for.as_ref().map(|(k, ..)| k.as_str()) != Some(key);
        self.built_for = Some(wanted);
        self.scene = None;
        self.error = None;
        // Dark parsers panic on malformed input; fall back to an error label.
        let built = quiet_catch(|| {
            BinObjViewerScene::from_model(
                key.to_string(),
                &self.asset_cache,
                self.debug_skeletons,
                self.debug_hit_boxes,
            )
            .map_err(|e| e.to_string())
        });
        match built {
            Ok(Ok(scene)) => {
                self.scene = Some(Box::new(scene));
                if reframe {
                    self.frame_camera(key);
                }
            }
            Ok(Err(msg)) | Err(msg) => self.error = Some(msg),
        }
    }

    /// Reset the orbit to frame the model: static models by their bounding
    /// box, animated (AI) meshes with a standing-creature default.
    fn frame_camera(&mut self, key: &str) {
        self.yaw = 90.0;
        self.pitch = 80.0;
        let bounds = quiet_catch(|| {
            let model = self.asset_cache.get(&MODELS_IMPORTER, key);
            model
                .bounding_box()
                .map(|bb| (bb.min, bb.max, model.get_transform()))
        })
        .ok()
        .flatten();
        match bounds {
            Some((min, max, transform)) => {
                // The scene objects render with the model transform applied, so
                // frame the transformed box center.
                let center = transform
                    * cgmath::vec4(
                        (min.x + max.x) / 2.0,
                        (min.y + max.y) / 2.0,
                        (min.z + max.z) / 2.0,
                        1.0,
                    );
                self.target = vec3(center.x, center.y, center.z);
                let extent = vec3(max.x - min.x, max.y - min.y, max.z - min.z);
                let radius =
                    (extent.x * extent.x + extent.y * extent.y + extent.z * extent.z).sqrt() / 2.0;
                self.distance = (radius * 4.0).clamp(3.0, 150.0);
            }
            None => {
                // Animated (AI) meshes expose no bounds; frame a creature
                // T-posed around the origin (hips at 0, limbs a few feet out).
                self.target = vec3(0.0, 0.0, 0.0);
                self.distance = 12.0;
            }
        }
    }

    fn apply_camera_input(&mut self, ui: &egui::Ui, response: &egui::Response) {
        if response.dragged() {
            let delta = response.drag_delta();
            self.yaw += delta.x * 0.4;
            self.pitch = (self.pitch + delta.y * 0.4).clamp(5.0, 175.0);
        }
        if response.hovered() {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            if scroll != 0.0 {
                self.distance = (self.distance * (-scroll * 0.003).exp()).clamp(0.5, 300.0);
            }
        }
    }

    /// Create (or resize) the offscreen FBO and register its color texture
    /// with egui. A resize registers a fresh egui texture id.
    fn ensure_target(&mut self, frame: &mut eframe::Frame, size: [i32; 2]) {
        if self.fbo.as_ref().is_some_and(|t| t.size == size) {
            return;
        }
        if let Some(old) = self.fbo.take() {
            unsafe {
                gl::DeleteFramebuffers(1, &old.fbo);
                gl::DeleteRenderbuffers(1, &old.depth);
                // The color texture's ownership passed to egui at registration;
                // leave deleting it to the painter.
            }
        }
        unsafe {
            let mut color = 0;
            gl::GenTextures(1, &mut color);
            gl::BindTexture(gl::TEXTURE_2D, color);
            gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MIN_FILTER, gl::LINEAR as i32);
            gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MAG_FILTER, gl::LINEAR as i32);
            gl::TexImage2D(
                gl::TEXTURE_2D,
                0,
                gl::RGBA8 as i32,
                size[0],
                size[1],
                0,
                gl::RGBA,
                gl::UNSIGNED_BYTE,
                std::ptr::null(),
            );
            gl::BindTexture(gl::TEXTURE_2D, 0);

            let mut depth = 0;
            gl::GenRenderbuffers(1, &mut depth);
            gl::BindRenderbuffer(gl::RENDERBUFFER, depth);
            gl::RenderbufferStorage(gl::RENDERBUFFER, gl::DEPTH_COMPONENT24, size[0], size[1]);
            gl::BindRenderbuffer(gl::RENDERBUFFER, 0);

            let mut fbo = 0;
            gl::GenFramebuffers(1, &mut fbo);
            gl::BindFramebuffer(gl::FRAMEBUFFER, fbo);
            gl::FramebufferTexture2D(
                gl::FRAMEBUFFER,
                gl::COLOR_ATTACHMENT0,
                gl::TEXTURE_2D,
                color,
                0,
            );
            gl::FramebufferRenderbuffer(
                gl::FRAMEBUFFER,
                gl::DEPTH_ATTACHMENT,
                gl::RENDERBUFFER,
                depth,
            );
            let complete = gl::CheckFramebufferStatus(gl::FRAMEBUFFER) == gl::FRAMEBUFFER_COMPLETE;
            gl::BindFramebuffer(gl::FRAMEBUFFER, 0);
            if !complete {
                self.error = Some("offscreen framebuffer incomplete".to_string());
                gl::DeleteFramebuffers(1, &fbo);
                gl::DeleteRenderbuffers(1, &depth);
                gl::DeleteTextures(1, &color);
                return;
            }

            let native = glow::NativeTexture(std::num::NonZeroU32::new(color).unwrap());
            let egui_texture = frame.register_native_glow_texture(native);
            self.fbo = Some(OffscreenTarget {
                fbo,
                depth,
                size,
                egui_texture,
            });
        }
    }

    /// Render the scene into the FBO with the engine, restoring the
    /// framebuffer and viewport egui had bound.
    fn render_scene(&mut self, size: [i32; 2]) {
        let Some(target) = &self.fbo else { return };
        let Some(scene) = &self.scene else { return };
        let rendered = scene.render(&mut self.asset_cache);

        // dark_viewer's orbit: position on a sphere around `target`, oriented
        // to look back at it.
        let yaw_rad = self.yaw.to_radians();
        let pitch_rad = self.pitch.to_radians();
        let offset = vec3(
            self.distance * pitch_rad.sin() * yaw_rad.cos(),
            self.distance * pitch_rad.cos(),
            self.distance * pitch_rad.sin() * yaw_rad.sin(),
        );
        let pitch_quat = Quaternion::from_angle_x(Rad(pitch_rad - 90.0f32.to_radians()));
        let yaw_quat = Quaternion::from_angle_y(Rad(-yaw_rad + 90.0f32.to_radians()));
        let render_context = engine::EngineRenderContext {
            time: 0.0,
            camera_offset: self.target + offset,
            camera_rotation: Quaternion {
                v: vec3(0.0, 0.0, 0.0),
                s: 1.0,
            },
            head_offset: vec3(0.0, 0.0, 0.0),
            head_rotation: yaw_quat * pitch_quat,
            projection_matrix: cgmath::perspective(
                cgmath::Deg(45.0),
                size[0] as f32 / size[1] as f32,
                0.1,
                1000.0,
            ),
            screen_size: vec2(size[0] as f32, size[1] as f32),
        };

        unsafe {
            let mut prev_fbo = 0;
            gl::GetIntegerv(gl::DRAW_FRAMEBUFFER_BINDING, &mut prev_fbo);
            let mut prev_viewport = [0i32; 4];
            gl::GetIntegerv(gl::VIEWPORT, prev_viewport.as_mut_ptr());
            // egui's painter leaves scissoring on; the engine never touches it.
            gl::Disable(gl::SCISSOR_TEST);
            gl::BindFramebuffer(gl::FRAMEBUFFER, target.fbo);
            gl::Viewport(0, 0, size[0], size[1]);

            self.engine.render(&render_context, &rendered);

            gl::BindFramebuffer(gl::FRAMEBUFFER, prev_fbo as u32);
            gl::Viewport(
                prev_viewport[0],
                prev_viewport[1],
                prev_viewport[2],
                prev_viewport[3],
            );
        }
    }
}
