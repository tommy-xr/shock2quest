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

use cgmath::{InnerSpace, Matrix4, One, Quaternion, Rad, Rotation3, vec2, vec3};
use dark::importers::MODELS_IMPORTER;
use dark::model::Model;
use dark_viewer::scenes::{BinAiViewerScene, BinObjViewerScene, SkeletonViewerScene, ToolScene};
use eframe::{egui, glow};
use engine::Engine;
use engine::assets::asset_cache::AssetCache;
use shock2vr::{
    GloveRenderer, Handedness,
    vr_grip::{GripKinematics, GripSurface, ResolvedGrip, surface_fingerprint},
};

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

/// What the preview renders for the current selection.
#[derive(Clone, PartialEq)]
pub enum PreviewScene {
    /// The `.bin` model alone.
    Model,
    /// Authored VR weapon reference, retaining its integrated hand.
    VrReference,
    Grip(
        Handedness,
        ResolvedGrip,
        Option<shock2vr::vr_support::SupportProfile>,
    ),
    /// The `.bin` model animated by a motion clip (`<name>_.mc`).
    Clip(String),
    /// Bone lines only: the `.bin`'s skeleton posed by a motion clip.
    Skeleton(String),
}

impl PreviewScene {
    fn clip(&self) -> Option<&str> {
        match self {
            PreviewScene::Model | PreviewScene::VrReference | PreviewScene::Grip(..) => None,
            PreviewScene::Clip(clip) | PreviewScene::Skeleton(clip) => Some(clip),
        }
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
    glove: Option<GloveRenderer>,
    grip_bounds: Option<(cgmath::Vector3<f32>, f32)>,
    asset_cache: AssetCache,
    /// What the current scene (or error) was built for: (key, scene, skeletons,
    /// hitboxes, articulation). Guards against rebuilding — or re-panicking —
    /// every frame.
    built_for: Option<(String, PreviewScene, bool, bool, bool)>,
    scene: Option<Box<dyn ToolScene>>,
    /// The scene plays an animation clip, so it re-renders every frame.
    animated: bool,
    error: Option<String>,
    pub debug_skeletons: bool,
    pub debug_hit_boxes: bool,
    pub debug_articulation: bool,
    /// (sub-object, vhot) counts of the loaded LGMD model, when it has any -
    /// what the articulation overlay would draw.
    articulation: Option<(usize, usize)>,
    /// Suspend the per-frame wall-clock tick of an animated scene, so
    /// `advance()` is the only time source - `--screenshot` runs set this to
    /// capture a deterministic pose.
    pub paused: bool,
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
            glove: None,
            grip_bounds: None,
            asset_cache,
            built_for: None,
            scene: None,
            animated: false,
            error: None,
            debug_skeletons: false,
            debug_hit_boxes: false,
            debug_articulation: false,
            articulation: None,
            paused: false,
            yaw: 65.0,
            pitch: 75.0,
            distance: 10.0,
            target: vec3(0.0, 0.0, 0.0),
            fbo: None,
            needs_render: false,
        }
    }

    /// Show the preview for `key` (a `.bin` model): toggles, then the rendered
    /// viewport filling the remaining space. `scene` picks static, animated, or
    /// skeleton-only rendering; a clip plays on loop.
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        frame: &mut eframe::Frame,
        key: &str,
        scene: &PreviewScene,
    ) {
        self.ensure_scene(key, scene);
        if let Some(error) = &self.error {
            ui.label(format!("Cannot render this model: {error}"));
            return;
        }
        if self.animated && !self.paused {
            // Tick the playing clip with real dt and keep frames coming.
            self.needs_render = true;
            ui.ctx().request_repaint();
        }

        // A toggle change rebuilds on the next frame's ensure_scene (the click
        // itself triggers that repaint). The overlays mean nothing in
        // skeleton-only mode, which draws bones and nothing else.
        ui.horizontal(|ui| {
            if !matches!(
                scene,
                PreviewScene::Skeleton(_) | PreviewScene::Grip(..) | PreviewScene::VrReference
            ) {
                ui.checkbox(&mut self.debug_skeletons, "Skeleton");
                ui.checkbox(&mut self.debug_hit_boxes, "Hitboxes");
            }
            // Only an object (LGMD) .bin has sub-objects/vhots to show.
            if let Some((sub_objects, vhots)) = self.articulation {
                ui.checkbox(&mut self.debug_articulation, "Articulation");
                ui.label(format!("({sub_objects} sub-objects, {vhots} vhots)"));
            }
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
            if !self.paused {
                let dt = ui.input(|i| i.stable_dt).min(0.1);
                if let Some(scene) = &mut self.scene {
                    scene.update(dt);
                }
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

    /// Both grip editing modes share model preparation and camera ordering.
    pub fn show_grip(
        &mut self,
        ui: &mut egui::Ui,
        frame: &mut eframe::Frame,
        key: &str,
        scene: &PreviewScene,
        camera: Option<(&str, Handedness)>,
    ) {
        self.prepare(key, scene);
        if let Some((view, hand)) = camera {
            self.grip_camera(view, hand);
        }
        self.show(ui, frame, key, scene);
    }

    pub fn prepare(&mut self, key: &str, scene: &PreviewScene) {
        self.ensure_scene(key, scene);
    }

    /// (Re)build the scene when the key, clip, or a debug toggle changed,
    /// framing the camera from the model's bounds where they are known.
    fn ensure_scene(&mut self, key: &str, scene: &PreviewScene) {
        let wanted = (
            key.to_string(),
            scene.clone(),
            self.debug_skeletons,
            self.debug_hit_boxes,
            self.debug_articulation,
        );
        if self.built_for.as_ref() == Some(&wanted) {
            return;
        }
        let reframe = self.built_for.as_ref().map(|(k, ..)| k.as_str()) != Some(key);
        self.built_for = Some(wanted);
        self.scene = None;
        self.animated = false;
        self.error = None;
        self.articulation = None;
        // Load the model eagerly under catch_unwind — the scene itself defers
        // loading to render, and Dark parsers panic on malformed input; a
        // failure becomes an error label instead of a crash. The key resolves
        // through the full game mount stack (obj outranks mesh; the two
        // families currently share no `.bin` basenames).
        let model = match quiet_catch(|| {
            if matches!(scene, PreviewScene::VrReference) {
                return std::rc::Rc::new(
                    self.asset_cache
                        .get(&dark::importers::VR_HELD_MODELS_IMPORTER, key)
                        .model
                        .clone(),
                );
            }
            if matches!(scene, PreviewScene::Grip(..))
                && shock2vr::vr_weapon_grip::supports_model(key)
            {
                let source = self
                    .asset_cache
                    .get(&dark::importers::GLOVE_WEAPON_IMPORTER, key);
                if let Some(source) = source.as_ref() {
                    return std::rc::Rc::new(source.model.clone());
                }
            }
            self.asset_cache.get(&MODELS_IMPORTER, key)
        }) {
            Ok(model) => model,
            Err(msg) => {
                self.error = Some(msg);
                return;
            }
        };
        // Skeleton scenes frame on their posed joints; an AI mesh has no
        // bounding box for `frame_camera` to use.
        let mut pose_bounds = None;
        let built: Result<Box<dyn ToolScene>, String> = match scene {
            PreviewScene::VrReference => Ok(Box::new(GripPreviewScene(
                engine::scene::Scene::from_objects(model.clone_scene_objects()),
            ))),
            PreviewScene::Grip(hand, grip, support) => quiet_catch(|| {
                let model_mirror = self
                    .grip_model_mirror(key)
                    .map_err(|_| "Weapon mirror unavailable")?;
                let mut model = model.as_ref().clone();
                if shock2vr::vr_weapon_grip::supports_model(key) {
                    let source = self
                        .asset_cache
                        .get(&dark::importers::GLOVE_WEAPON_IMPORTER, key);
                    let source = source.as_ref().as_ref().ok_or("Weapon model unavailable")?;
                    model.apply_local_transform(shock2vr::vr_weapon_grip::model_frame(
                        source, *hand,
                    ));
                }
                let mut objects = Model::transform(
                    &model,
                    Matrix4::from_translation(grip.offset)
                        * Matrix4::from(grip.rotation)
                        * Matrix4::from_scale(grip.item_scale),
                )
                .clone_scene_objects();
                if self.glove.is_none() {
                    self.glove = GloveRenderer::new(&mut self.asset_cache);
                }
                let glove = self.glove.as_mut().ok_or("Glove model unavailable")?;
                objects.extend(glove.render_hand(
                    vec3(0.0, 0.0, 0.0),
                    Quaternion::one(),
                    *hand,
                    0.0,
                    0.0,
                    true,
                    Some((grip.finger_amounts(), 1.0)),
                    shock2vr::HandLight::Off,
                ));
                let mut support_points = Vec::new();
                if let Some(support) = support {
                    if support.region.is_some() {
                        let [a, b] = support
                            .region_in_frame(*hand, model_mirror)
                            .map(|p| grip.offset + grip.rotation * (p * grip.item_scale));
                        objects.push(support_region_overlay(a, b, support.grab_radius));
                        for p in [a, b] {
                            support_points.push(p - vec3(1.0, 1.0, 1.0) * support.grab_radius);
                            support_points.push(p + vec3(1.0, 1.0, 1.0) * support.grab_radius);
                        }
                    }
                    let other = if *hand == Handedness::Left {
                        Handedness::Right
                    } else {
                        Handedness::Left
                    };
                    let rig = glove.grip_kinematics(other);
                    let pose = support.glove_pose(
                        *hand,
                        shock2vr::vr_support::GripPose {
                            position: grip.offset,
                            rotation: grip.rotation,
                        },
                        grip,
                        &rig,
                        support.anchor_in_frame(*hand, model_mirror) * grip.item_scale,
                    );
                    let mut support_grip = grip.clone();
                    support_grip.curls = support.curls;
                    support_grip.trigger_curls = None; // Preview curls are already blended.
                    objects.extend(glove.render_hand(
                        pose.position,
                        pose.rotation,
                        other,
                        0.0,
                        0.0,
                        false,
                        Some((support_grip.finger_amounts(), 1.0)),
                        shock2vr::HandLight::Off,
                    ));
                    support_points.extend(
                        rig.fingers
                            .iter()
                            .flatten()
                            .flatten()
                            .map(|p| pose.point(*p)),
                    );
                }
                let triangles = if shock2vr::vr_weapon_grip::supports_model(key) {
                    shock2vr::vr_weapon_grip::inputs(&mut self.asset_cache, key, *hand)
                        .ok_or("Weapon grip geometry unavailable")?
                        .0
                } else {
                    self.asset_cache
                        .get(&dark::importers::GRIP_SURFACE_IMPORTER, key)
                        .as_ref()
                        .clone()
                };
                let transform = Matrix4::from_translation(grip.offset)
                    * Matrix4::from(grip.rotation)
                    * Matrix4::from_scale(grip.item_scale);
                // Include the actual sampled glove arcs as well as the item, so
                // a small prop cannot crop the wrist or a large one its far end.
                let kinematics = glove.grip_kinematics(*hand);
                let points = triangles
                    .iter()
                    .flatten()
                    .map(|p| (transform * p.to_homogeneous()).truncate())
                    .chain(kinematics.fingers.iter().flatten().flatten().copied())
                    .chain(support_points);
                let mut min = vec3(f32::INFINITY, f32::INFINITY, f32::INFINITY);
                let mut max = -min;
                for p in points {
                    for i in 0..3 {
                        min[i] = min[i].min(p[i]);
                        max[i] = max[i].max(p[i]);
                    }
                }
                pose_bounds = Some(((min + max) * 0.5, (max - min).magnitude() * 0.5));
                self.grip_bounds = pose_bounds;
                Ok(
                    Box::new(GripPreviewScene(engine::scene::Scene::from_objects(
                        objects,
                    ))) as Box<dyn ToolScene>,
                )
            })
            .and_then(|r: Result<_, &str>| r.map_err(str::to_string)),
            PreviewScene::Model => BinObjViewerScene::from_model(
                key.to_string(),
                &self.asset_cache,
                self.debug_skeletons,
                self.debug_hit_boxes,
                self.debug_articulation,
            )
            .map(|scene| Box::new(scene) as Box<dyn ToolScene>)
            .map_err(|err| err.to_string()),
            // Clip parsing panics on malformed input too, so it also runs
            // under the guard.
            PreviewScene::Clip(clip) => quiet_catch(|| {
                BinAiViewerScene::from_clips(
                    key.to_string(),
                    vec![dark_viewer::normalize_clip_name(clip)?],
                    &mut self.asset_cache,
                    self.debug_skeletons,
                    self.debug_hit_boxes,
                )
                .map(|scene| Box::new(scene) as Box<dyn ToolScene>)
                .map_err(|err| err.to_string())
            })
            .and_then(|r| r),
            PreviewScene::Skeleton(clip) => quiet_catch(|| {
                SkeletonViewerScene::new(
                    key,
                    &dark_viewer::normalize_clip_name(clip)?,
                    &mut self.asset_cache,
                )
                .map(|scene| {
                    pose_bounds = Some(scene.pose_bounds());
                    Box::new(scene) as Box<dyn ToolScene>
                })
            })
            .and_then(|r| r),
        };
        // Offer the articulation toggle only where there is something to draw.
        if matches!(scene, PreviewScene::Model) {
            let counts = (model.sub_objects().len(), model.vhots().len());
            if counts != (0, 0) {
                self.articulation = Some(counts);
            }
        }
        // No toggle means no way to turn it back off, so don't carry a hidden
        // "on" over from the previous model. Keep `built_for` in step or the
        // next frame rebuilds the scene for nothing.
        if self.articulation.is_none() {
            self.debug_articulation = false;
            if let Some(built_for) = &mut self.built_for {
                built_for.4 = false;
            }
        }
        match built {
            Ok(built) => {
                self.scene = Some(built);
                self.animated = scene.clip().is_some();
                self.needs_render = true;
                if reframe {
                    match pose_bounds {
                        Some((center, radius)) => self.frame_bounds(
                            center,
                            radius,
                            if matches!(scene, PreviewScene::Grip(..)) {
                                0.25
                            } else {
                                1.0
                            },
                        ),
                        None => self.frame_camera(&model),
                    }
                }
            }
            Err(err) => self.error = Some(err),
        }
    }

    /// Reflection between the two rendered weapon frames. Ordinary pickups
    /// retain their mesh, so X reflection supplies an approximate opposite-side fit.
    pub fn grip_model_mirror(&mut self, key: &str) -> Result<Matrix4<f32>, String> {
        if !shock2vr::vr_weapon_grip::supports_model(key) {
            return Ok(Handedness::Left.mirror());
        }
        shock2vr::vr_weapon_grip::model_mirror(&mut self.asset_cache, key)
            .ok_or_else(|| "Weapon mirror unavailable".into())
    }

    /// Shared game geometry and rig samples for validation and explicit auto-fit.
    pub fn grip_inputs(
        &mut self,
        key: &str,
        hand: Handedness,
    ) -> Result<
        (
            GripSurface,
            GripKinematics,
            String,
            Option<(Vec<[cgmath::Point3<f32>; 3]>, Vec<cgmath::Point3<f32>>)>,
        ),
        String,
    > {
        quiet_catch(|| {
            let weapon = if shock2vr::vr_weapon_grip::supports_model(key) {
                Some(
                    shock2vr::vr_weapon_grip::inputs(&mut self.asset_cache, key, hand)
                        .ok_or("Weapon grip geometry unavailable")?,
                )
            } else {
                None
            };
            let (triangles, hash, guide) = if let Some((triangles, arms, hash)) = weapon {
                (triangles.clone(), hash, Some((triangles, arms)))
            } else {
                let triangles = self
                    .asset_cache
                    .get(&dark::importers::GRIP_SURFACE_IMPORTER, key);
                (
                    triangles.as_ref().clone(),
                    surface_fingerprint(&triangles),
                    None,
                )
            };
            let surface = GripSurface::new(&triangles).ok_or("No usable pickup surface")?;
            if self.glove.is_none() {
                self.glove = GloveRenderer::new(&mut self.asset_cache);
            }
            let rig = self
                .glove
                .as_mut()
                .ok_or("Glove model unavailable")?
                .grip_kinematics(hand);
            Ok((surface, rig, hash, guide))
        })
        .and_then(|r: Result<_, &str>| r.map_err(str::to_string))
    }

    /// Camera presets share the gallery's hand-local axes. Orbit remains free.
    pub fn grip_camera(&mut self, view: &str, hand: Handedness) {
        if let Some((center, radius)) = self.grip_bounds {
            self.frame_bounds(center, radius, 0.25);
        }
        (self.yaw, self.pitch) = match view {
            "front" => (90.0, 90.0),
            "back" => (-90.0, 90.0),
            "top" => (90.0, 0.1),
            "palm" => (
                if hand == Handedness::Right {
                    63.4
                } else {
                    -63.4
                },
                65.9,
            ),
            _ => (-45.0, 66.2),
        };
        if view == "palm" {
            self.target = vec3(0.0, 0.0, -0.12);
            self.distance = 0.5;
        }
        self.needs_render = true;
    }

    /// Step the playing scene forward by `seconds` of simulation time (in
    /// fixed 60 Hz increments), so `--screenshot` runs can capture a pose
    /// mid-clip.
    pub fn advance(&mut self, seconds: f32) {
        let Some(scene) = &mut self.scene else { return };
        // Cap at 10 minutes of sim time so a typo'd --advance can't hang.
        let steps = (seconds * 60.0).round().clamp(0.0, 60.0 * 600.0) as u32;
        for _ in 0..steps {
            scene.update(1.0 / 60.0);
        }
        self.needs_render = true;
    }

    /// Why the current selection could not be shown, if it could not.
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// Reset the orbit to frame the model: static models by their bounding
    /// box, animated (AI) meshes with a standing-creature default. The default
    /// angle is a three-quarter view so flat models are not seen edge-on.
    fn frame_camera(&mut self, model: &Model) {
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
                let extent = vec3(max.x - min.x, max.y - min.y, max.z - min.z);
                let radius =
                    (extent.x * extent.x + extent.y * extent.y + extent.z * extent.z).sqrt() / 2.0;
                self.frame_bounds(vec3(center.x, center.y, center.z), radius, 3.0);
            }
            None => {
                // Animated (AI) meshes expose no bounds; they T-pose around
                // the origin at the hips (legs below y=0), so aim there.
                self.yaw = 65.0;
                self.pitch = 75.0;
                self.target = vec3(0.0, 0.0, 0.0);
                self.distance = 11.0;
            }
        }
    }

    /// Orbit around a sphere of `radius` at `center`, at the default angle.
    /// r / sin(fovy/2) exactly fills the 45-degree frustum; 2.9r leaves margin.
    fn frame_bounds(&mut self, center: cgmath::Vector3<f32>, radius: f32, min_distance: f32) {
        self.yaw = 65.0;
        self.pitch = 75.0;
        self.target = center;
        self.distance = (radius * 2.9).clamp(min_distance, 150.0);
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
                let minimum = if self
                    .built_for
                    .as_ref()
                    .is_some_and(|(_, s, ..)| matches!(s, PreviewScene::Grip(..)))
                {
                    0.15
                } else {
                    0.5
                };
                self.distance = (self.distance * (-scroll * 0.003).exp()).clamp(minimum, 300.0);
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
                if self
                    .built_for
                    .as_ref()
                    .is_some_and(|(_, s, ..)| matches!(s, PreviewScene::Grip(..)))
                {
                    0.01
                } else {
                    0.1
                },
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

struct GripPreviewScene(engine::scene::Scene);
impl ToolScene for GripPreviewScene {
    fn update(&mut self, _delta_time: f32) {}
    fn render(&self, _asset_cache: &mut AssetCache) -> engine::scene::Scene {
        self.0.clone()
    }
}

/// Wire capsule in preview coordinates: the same world-unit radius used to grab.
fn support_region_overlay(
    a: cgmath::Vector3<f32>,
    b: cgmath::Vector3<f32>,
    radius: f32,
) -> engine::scene::SceneObject {
    use engine::scene::{SceneObject, VertexPosition, color_material, lines_mesh};
    let mut vertices = vec![
        VertexPosition { position: a },
        VertexPosition { position: b },
    ];
    dark::hit_box::append_capsule_lines(&mut vertices, &Matrix4::one(), a, b, radius);
    SceneObject::new(
        color_material::create(vec3(0.1, 0.9, 0.8)),
        Box::new(lines_mesh::create(vertices)),
    )
}
