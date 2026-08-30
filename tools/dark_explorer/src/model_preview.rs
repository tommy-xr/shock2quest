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
use dark::model::Model;
use dark_viewer::scenes::{BinAiViewerScene, BinObjViewerScene, ToolScene};
use eframe::{egui, glow};
use engine::Engine;
use engine::assets::asset_cache::AssetCache;

use crate::ui::quiet_catch;

/// Load the raw `gl` crate's function pointers from eframe's GL context.
/// Call once, in the `AppCreator` closure, before any engine rendering.
pub fn init_raw_gl(cc: &eframe::CreationContext<'_>) {
    match cc.get_proc_address.clone() {
        Some(get_proc) => gl::load_with(move |symbol| match CString::new(symbol) {
            Ok(symbol) => get_proc(&symbol),
            Err(_) => std::ptr::null(),
        }),
        // Selecting a model would then die inside the first raw GL call, so
        // leave a trail (eframe only omits the loader on non-glow backends).
        None => eprintln!("no GL proc loader from eframe; model preview will not work"),
    }
}

/// Offscreen render target whose color texture egui displays. Created once;
/// a resize re-specifies the texture/renderbuffer storage in place, so the
/// registered egui texture id stays valid for the preview's lifetime.
struct OffscreenTarget {
    fbo: u32,
    color: u32,
    depth: u32,
    size: [i32; 2],
    egui_texture: egui::TextureId,
}

pub struct ModelPreview {
    engine: Box<dyn Engine>,
    asset_cache: AssetCache,
    /// What the current scene (or error) was built for: (key, clip, skeletons,
    /// hitboxes). Guards against rebuilding — or re-panicking — every frame.
    built_for: Option<(String, Option<String>, bool, bool)>,
    scene: Option<Box<dyn ToolScene>>,
    /// The scene plays an animation clip, so it re-renders every frame.
    animated: bool,
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
    /// A static scene's FBO is only re-rendered when something changed —
    /// scene, camera, or viewport size; an animated one renders every frame.
    needs_render: bool,
}

impl ModelPreview {
    /// Build the render host: the engine plus one asset cache over the exact
    /// mount stack the game resolves models and textures through (which also
    /// carries the engine bundle assets, e.g. the grid ground-plane texture).
    pub fn new() -> ModelPreview {
        let engine = engine::opengl();
        let mounts = shock2vr::game_asset_mounts(engine.get_storage());
        let base_path = shock2vr::paths::data_root().to_string_lossy().into_owned();
        let asset_cache = AssetCache::new(base_path, mounts);
        ModelPreview {
            engine,
            asset_cache,
            built_for: None,
            scene: None,
            animated: false,
            error: None,
            debug_skeletons: false,
            debug_hit_boxes: false,
            yaw: 65.0,
            pitch: 75.0,
            distance: 10.0,
            target: vec3(0.0, 0.0, 0.0),
            fbo: None,
            needs_render: false,
        }
    }

    /// Show the preview for `key` (a `.bin` model): toggles, then the rendered
    /// viewport filling the remaining space. With `clip` (a motion name, no
    /// extension), the model animates with that clip on loop.
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        frame: &mut eframe::Frame,
        key: &str,
        clip: Option<&str>,
    ) {
        self.ensure_scene(key, clip);
        if let Some(error) = &self.error {
            ui.label(format!("Cannot render this model: {error}"));
            return;
        }
        if self.animated {
            // Tick the playing clip with real dt and keep frames coming.
            self.needs_render = true;
            ui.ctx().request_repaint();
        }

        // A toggle change rebuilds on the next frame's ensure_scene (the click
        // itself triggers that repaint).
        ui.horizontal(|ui| {
            ui.checkbox(&mut self.debug_skeletons, "Skeleton");
            ui.checkbox(&mut self.debug_hit_boxes, "Hitboxes");
            ui.label("(drag to orbit, scroll to zoom)");
        });

        let available = ui.available_size();
        let size = egui::vec2(available.x.max(1.0), available.y.max(1.0));
        let response = ui.allocate_response(size, egui::Sense::drag());
        if self.apply_camera_input(ui, &response) {
            self.needs_render = true;
        }

        let pixels_per_point = ui.ctx().pixels_per_point();
        let px = [
            (size.x * pixels_per_point).round().max(1.0) as i32,
            (size.y * pixels_per_point).round().max(1.0) as i32,
        ];
        if self.ensure_target(frame, px) {
            self.needs_render = true;
        }

        if self.needs_render && self.scene.is_some() && self.fbo.is_some() {
            let dt = ui.input(|i| i.stable_dt).min(0.1);
            if let Some(scene) = &mut self.scene {
                scene.update(dt);
            }
            self.render_scene(px);
            self.needs_render = false;
        }

        if let Some(target) = &self.fbo {
            // Opaque backing: the engine clears to alpha 0 and blends straight
            // alpha, while egui composites premultiplied — translucent overlay
            // pixels would tint against the panel otherwise.
            ui.painter()
                .rect_filled(response.rect, 0.0, egui::Color32::BLACK);
            // GL renders bottom-up; flip V so egui shows it upright.
            let uv = egui::Rect::from_min_max(egui::pos2(0.0, 1.0), egui::pos2(1.0, 0.0));
            ui.painter()
                .image(target.egui_texture, response.rect, uv, egui::Color32::WHITE);
        }
    }

    /// (Re)build the scene when the key, clip, or a debug toggle changed,
    /// framing the camera from the model's bounds where they are known.
    fn ensure_scene(&mut self, key: &str, clip: Option<&str>) {
        let wanted = (
            key.to_string(),
            clip.map(|c| c.to_string()),
            self.debug_skeletons,
            self.debug_hit_boxes,
        );
        if self.built_for.as_ref() == Some(&wanted) {
            return;
        }
        let reframe = self.built_for.as_ref().map(|(k, ..)| k.as_str()) != Some(key);
        self.built_for = Some(wanted);
        self.scene = None;
        self.animated = false;
        self.error = None;
        // Load the model eagerly under catch_unwind — the scene itself defers
        // loading to render, and Dark parsers panic on malformed input; a
        // failure becomes an error label instead of a crash. The key resolves
        // through the full game mount stack (obj outranks mesh; the two
        // families currently share no `.bin` basenames).
        let model = match quiet_catch(|| self.asset_cache.get(&MODELS_IMPORTER, key)) {
            Ok(model) => model,
            Err(msg) => {
                self.error = Some(msg);
                return;
            }
        };
        let scene: Result<Box<dyn ToolScene>, String> = match clip {
            None => BinObjViewerScene::from_model(
                key.to_string(),
                &self.asset_cache,
                self.debug_skeletons,
                self.debug_hit_boxes,
            )
            .map(|scene| Box::new(scene) as Box<dyn ToolScene>)
            .map_err(|err| err.to_string()),
            // Clip parsing panics on malformed input too, so it also runs
            // under the guard. `<name>_.mc` is the clip importer's naming.
            Some(clip) => quiet_catch(|| {
                BinAiViewerScene::from_clips(
                    key.to_string(),
                    vec![format!("{clip}_.mc")],
                    &mut self.asset_cache,
                    self.debug_skeletons,
                    self.debug_hit_boxes,
                )
                .map(|scene| Box::new(scene) as Box<dyn ToolScene>)
                .map_err(|err| err.to_string())
            })
            .and_then(|r| r),
        };
        match scene {
            Ok(scene) => {
                self.scene = Some(scene);
                self.animated = clip.is_some();
                self.needs_render = true;
                if reframe {
                    self.frame_camera(&model);
                }
            }
            Err(err) => self.error = Some(err),
        }
    }

    /// Step the playing scene forward by `seconds` of simulation time (in
    /// fixed 60 Hz increments), so `--screenshot` runs can capture a pose
    /// mid-clip.
    pub fn advance(&mut self, seconds: f32) {
        let Some(scene) = &mut self.scene else { return };
        let steps = (seconds * 60.0).round().max(0.0) as u32;
        for _ in 0..steps {
            scene.update(1.0 / 60.0);
        }
        self.needs_render = true;
    }

    /// Reset the orbit to frame the model: static models by their bounding
    /// box, animated (AI) meshes with a standing-creature default. The default
    /// angle is a three-quarter view so flat models are not seen edge-on.
    fn frame_camera(&mut self, model: &Model) {
        self.yaw = 65.0;
        self.pitch = 75.0;
        match model.bounding_box() {
            Some(bb) => {
                // The scene objects render with the model transform applied, so
                // frame the transformed box center.
                let (min, max) = (bb.min, bb.max);
                let center = model.get_transform()
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
                // r / sin(fovy/2) exactly fills the 45-degree frustum; 2.9r
                // leaves some margin around it.
                self.distance = (radius * 2.9).clamp(3.0, 150.0);
            }
            None => {
                // Animated (AI) meshes expose no bounds; they T-pose around
                // the origin at the hips (legs below y=0), so aim there.
                self.target = vec3(0.0, 0.0, 0.0);
                self.distance = 11.0;
            }
        }
    }

    /// Returns whether the camera moved.
    fn apply_camera_input(&mut self, ui: &egui::Ui, response: &egui::Response) -> bool {
        let mut moved = false;
        if response.dragged() {
            let delta = response.drag_delta();
            if delta != egui::Vec2::ZERO {
                self.yaw += delta.x * 0.4;
                self.pitch = (self.pitch + delta.y * 0.4).clamp(5.0, 175.0);
                moved = true;
            }
        }
        if response.hovered() {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            if scroll != 0.0 {
                self.distance = (self.distance * (-scroll * 0.003).exp()).clamp(0.5, 300.0);
                moved = true;
            }
        }
        moved
    }

    /// Create the offscreen FBO on first use, or re-specify its texture and
    /// depth storage on resize (keeping the GL names, and therefore the
    /// registered egui texture id, stable). Returns whether anything changed.
    fn ensure_target(&mut self, frame: &mut eframe::Frame, size: [i32; 2]) -> bool {
        if let Some(target) = &mut self.fbo {
            if target.size == size {
                return false;
            }
            unsafe {
                gl::BindTexture(gl::TEXTURE_2D, target.color);
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
                gl::BindRenderbuffer(gl::RENDERBUFFER, target.depth);
                gl::RenderbufferStorage(gl::RENDERBUFFER, gl::DEPTH_COMPONENT24, size[0], size[1]);
                gl::BindRenderbuffer(gl::RENDERBUFFER, 0);
            }
            target.size = size;
            return true;
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
                return false;
            }

            let native = glow::NativeTexture(std::num::NonZeroU32::new(color).unwrap());
            let egui_texture = frame.register_native_glow_texture(native);
            self.fbo = Some(OffscreenTarget {
                fbo,
                color,
                depth,
                size,
                egui_texture,
            });
        }
        true
    }

    /// Render the scene into the FBO with the engine, restoring the
    /// framebuffer and viewport egui had bound.
    fn render_scene(&mut self, size: [i32; 2]) {
        let Some(target) = &self.fbo else { return };
        let Some(scene) = &self.scene else { return };
        // Belt-and-braces: the scene loads lazily through the asset cache at
        // render time too (its own model lookup, the grid texture).
        let rendered = match quiet_catch(|| scene.render(&mut self.asset_cache)) {
            Ok(rendered) => rendered,
            Err(msg) => {
                self.scene = None;
                self.error = Some(msg);
                return;
            }
        };

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
