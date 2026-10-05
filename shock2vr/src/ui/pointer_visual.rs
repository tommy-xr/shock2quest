//! The visible half of the VR frontend pointer: a gloved hand at each tracked
//! controller, a beam along its aim, and a dot where the menu is being pointed.
//!
//! Without this a VR menu gives no feedback until an entry happens to light up,
//! so aiming is guesswork (issue #1001). Everything here is derived from the
//! [`FrontendPointerPass`] the menu itself hit-tested - the *canvas* hit point,
//! mapped back through [`canvas_to_panel_world`], and the pass's own choice of
//! which controller is driving the menu - so the dot marks the pixel the menu
//! hit-tested, on the controller the menu is listening to, and the two cannot
//! drift apart.

use cgmath::{InnerSpace, Matrix4, Quaternion, Vector2, Vector3, vec3};
use engine::{
    assets::asset_cache::AssetCache,
    scene::{FrontFaceWinding, SceneObject, color_material, cube, cylinder, laser_material, quad},
};

use crate::{
    hand_glove::{GloveRenderer, StaticHandPose},
    ui::{
        FrontendPointerPass, FrontendRay, VR_COMPONENT_Z_STEP, WorldPanel, canvas_to_panel_world,
    },
};

/// How far a beam that misses the panel reaches into space. Long enough to read
/// as "pointing somewhere", short enough not to spear the whole room.
pub const POINTER_BEAM_MISS_LENGTH: f32 = 2.0;
/// Radii of the beam's soft halo and bright core, in metres.
const BEAM_HALO_RADIUS: f32 = 0.012;
const BEAM_CORE_RADIUS: f32 = 0.0025;
/// Edge length of the hit dot's quad, in metres. The glow fades out well
/// inside it.
const POINTER_DOT_SIZE: f32 = 0.12;
/// Clear air between the panel's frontmost canvas layer and the dot.
const POINTER_DOT_CLEARANCE: f32 = VR_COMPONENT_Z_STEP * 2.0;
/// Floor on how squarely a ray may meet the panel before the pull-back below
/// stops scaling. Without it a ray grazing the panel edge would put its dot
/// arbitrarily far back down the beam.
const MIN_APPROACH: f32 = 0.25;
/// Size of the fallback controller proxy: a stubby box at the hand, aimed down
/// the ray. Only drawn when the glove model is unavailable.
const CONTROLLER_PROXY_SIZE: Vector3<f32> = Vector3 {
    x: 0.035,
    y: 0.035,
    z: 0.09,
};

/// Clear space left at the hand end, so the beam emerges from the glove's
/// fingertips instead of starting inside the palm. Roughly a hand's length.
const BEAM_HAND_CLEARANCE: f32 = 0.25;

/// UI green, so a menu pointer never reads as a weapon's red laser sight.
const BEAM_HALO_COLOR: Vector3<f32> = Vector3 {
    x: 0.02,
    y: 1.0,
    z: 0.15,
};
const BEAM_CORE_COLOR: Vector3<f32> = Vector3 {
    x: 0.25,
    y: 1.0,
    z: 0.35,
};
const DOT_COLOR: Vector3<f32> = BEAM_CORE_COLOR;
const CONTROLLER_COLOR: Vector3<f32> = Vector3 {
    x: 0.6,
    y: 0.6,
    z: 0.7,
};

/// The pose the hand driving `rays[index]` is shown in.
///
/// Read off the same pass that decides the hover highlight, the click and the
/// hit dot, so the hand points exactly when the menu is listening to it - a
/// hand posed as pointing can never promise a hover the menu will not give.
pub fn pointer_hand_pose(pass: &FrontendPointerPass, index: usize) -> StaticHandPose {
    if pass.is_active(index) {
        StaticHandPose::Pointing
    } else {
        StaticHandPose::Relaxed
    }
}

/// How far in front of the panel face the dot floats, given a canvas that
/// stacked `panel_layers` layers on it (see [`canvas_layers`]).
fn dot_lift(panel_layers: usize) -> f32 {
    VR_COMPONENT_Z_STEP * panel_layers as f32 + POINTER_DOT_CLEARANCE
}

/// How many `VR_COMPONENT_Z_STEP` layers a world-space canvas stacks on
/// `panel`, read off its frontmost object. The canvas steps only overlapping
/// art forward, so its object count overstates the stack many times over -
/// enough to float the dot centimetres off a near panel like the tricorder's.
pub fn canvas_layers(canvas: &[SceneObject], panel: &WorldPanel) -> usize {
    let front = canvas
        .iter()
        .map(|object| (object.get_transform().w.truncate() - panel.center).dot(panel.normal()))
        .fold(0.0, f32::max);
    (front / VR_COMPONENT_Z_STEP).round() as usize + 1
}

/// Where one controller's pointer draws, in world space.
///
/// Pure geometry, so the placement rule is testable without a renderer.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PointerRayGeometry {
    /// The controller end of the beam.
    pub start: Vector3<f32>,
    /// The far end: just clear of the panel hit when there is one, otherwise
    /// [`POINTER_BEAM_MISS_LENGTH`] out along the aim.
    pub end: Vector3<f32>,
    /// The hit dot, drawn only for the controller the menu is listening to.
    pub dot: Option<Vector3<f32>>,
}

/// The beam and dot placement for one ray. `marks_hit` is whether this is the
/// ray the menu is actually pointing with.
pub fn pointer_ray_geometry(
    ray: &FrontendRay,
    canvas_size: Vector2<f32>,
    panel: &WorldPanel,
    panel_layers: usize,
    marks_hit: bool,
) -> PointerRayGeometry {
    // The hit comes back from the canvas point the menu hit-tested, not from a
    // second intersection of the same ray.
    let Some(point) = ray.canvas_hit else {
        return PointerRayGeometry {
            start: ray.origin,
            end: ray.origin + ray.direction * POINTER_BEAM_MISS_LENGTH,
            dot: None,
        };
    };
    let hit = canvas_to_panel_world(canvas_size, panel, point);

    // Stop short *along the beam* rather than pushing the marker out along the
    // panel normal: back down the ray it still reads as the point being aimed
    // at from any viewpoint, and the beam can end exactly there - so there is
    // no gap between beam and dot, and no beam tip buried in the canvas layers.
    let approach = -ray.direction.dot(panel.normal());
    let lift = dot_lift(panel_layers);
    let pull = lift / approach.max(MIN_APPROACH);
    // A ray grazing past MIN_APPROACH stops short of clearing the stack; top
    // the rest up along the normal so the dot never sinks under a label.
    let marker = hit - ray.direction * pull + panel.normal() * (lift - pull * approach).max(0.0);

    PointerRayGeometry {
        start: ray.origin,
        end: marker,
        dot: marks_hit.then_some(marker),
    }
}

/// A unit cube scaled to `scale` and placed at `center`, oriented by `rotation`.
fn box_object(
    center: Vector3<f32>,
    scale: Vector3<f32>,
    rotation: Quaternion<f32>,
    color: Vector3<f32>,
) -> SceneObject {
    let mut object = SceneObject::new(color_material::create(color), Box::new(cube::create()));
    object.set_transform(
        Matrix4::from_translation(center)
            * Matrix4::from(rotation)
            * Matrix4::from_nonuniform_scale(scale.x, scale.y, scale.z),
    );
    object
}

/// The smoky beam for one ray: the laser sight's halo and core cylinders in UI
/// green. Both blend without writing depth, so the panel shows through.
fn beam_objects(
    start: Vector3<f32>,
    along: Vector3<f32>,
    length: f32,
    seconds: f32,
) -> Vec<SceneObject> {
    // The hand fills the first stretch of the ray. A beam no longer than that
    // would be entirely inside the glove, so there is nothing to draw - the
    // hand is already touching what it points at.
    if length <= BEAM_HAND_CLEARANCE {
        return Vec::new();
    }
    let direction = along / length;
    let origin = start + direction * BEAM_HAND_CLEARANCE;
    let reach = direction * (length - BEAM_HAND_CLEARANCE);
    let tangent = if direction.y.abs() < 0.9 {
        vec3(0.0, 1.0, 0.0)
    } else {
        vec3(1.0, 0.0, 0.0)
    };
    let right = tangent.cross(direction).normalize();
    let up = direction.cross(right);

    [
        (false, BEAM_HALO_RADIUS, BEAM_HALO_COLOR),
        (true, BEAM_CORE_RADIUS, BEAM_CORE_COLOR),
    ]
    .into_iter()
    .map(|(core, radius, color)| {
        let mut object = SceneObject::new(
            laser_material::create_beam(core, color, seconds),
            Box::new(cylinder::Cylinder),
        );
        object.set_transform(Matrix4::from_cols(
            (right * radius).extend(0.0),
            (up * radius).extend(0.0),
            reach.extend(0.0),
            origin.extend(1.0),
        ));
        object.set_depth_write(false);
        object.set_backface_culling(Some(FrontFaceWinding::CounterClockwise));
        object
    })
    .collect()
}

/// The glowing hit dot: the laser sight's spot, flat against the panel so it
/// reads as a mark on the canvas.
fn dot_object(center: Vector3<f32>, panel: &WorldPanel) -> SceneObject {
    let mut object = SceneObject::new(laser_material::create(DOT_COLOR), Box::new(quad::create()));
    object.set_transform(
        Matrix4::from_translation(center)
            * Matrix4::from(panel.rotation)
            * Matrix4::from_nonuniform_scale(POINTER_DOT_SIZE, POINTER_DOT_SIZE, 1.0),
    );
    object.set_depth_write(false);
    object.set_backface_culling(None);
    object
}

/// The visible half of the frontend pointer, including the glove model it draws
/// the hands with.
///
/// The glove is loaded lazily on first render (it needs the asset cache) and
/// the outcome is kept either way, so a missing model costs one cache miss
/// rather than one per frame - the same shape `VrInteraction` uses in game.
#[derive(Default)]
pub struct PointerVisuals {
    /// `None` = not tried yet; `Some(None)` = tried and the model is missing.
    glove: Option<Option<GloveRenderer>>,
    pub(crate) glove_fit: Option<crate::glove_fit::GloveFit>,
}

impl PointerVisuals {
    pub fn new() -> Self {
        Self::default()
    }

    /// The scene objects for a frame of frontend pointing: a hand and beam for
    /// every tracked controller, plus a dot on the one the menu is listening
    /// to.
    ///
    /// `panel_layers` is the panel canvas's [`canvas_layers`]; `seconds`
    /// animates the beam's smoke. Shared by every frontend screen that uses the VR
    /// pointer, so a screen cannot end up with a pointer that hit-tests but
    /// does not show.
    pub fn render(
        &mut self,
        asset_cache: &mut AssetCache,
        pass: &FrontendPointerPass,
        canvas_size: Vector2<f32>,
        panel: &WorldPanel,
        panel_layers: usize,
        seconds: f32,
    ) -> Vec<SceneObject> {
        let glove = self
            .glove
            .get_or_insert_with(|| GloveRenderer::new(asset_cache))
            .as_mut();
        let mut objects = render_pointer_rays(
            glove,
            true,
            pass,
            canvas_size,
            panel,
            panel_layers,
            self.glove_fit,
            seconds,
        );
        // The one pair of hands a frontend screen shows. Labelled so a check
        // can assert *both* halves of issue #1018's fix: the scene's hands are
        // gone, and these are still there.
        crate::util::tag_render_source(&mut objects, crate::util::render_source::FRONTEND_POINTER);
        objects
    }
}

/// The aim beams and hit dot alone, with no hands at the controllers.
///
/// For a panel shown *during play* - the VR cyber interface - where the
/// player's own hands are already rendered by the interaction controller.
/// Drawing [`PointerVisuals`]' static glove there would stack a second,
/// empty-handed glove on top of the live one (and over a held weapon), so this
/// path deliberately shows only where each controller is aiming. It needs no
/// glove model, and so no asset cache.
pub fn pointer_beams(
    pass: &FrontendPointerPass,
    canvas_size: Vector2<f32>,
    panel: &WorldPanel,
    panel_layers: usize,
    seconds: f32,
) -> Vec<SceneObject> {
    let mut objects = render_pointer_rays(
        None,
        false,
        pass,
        canvas_size,
        panel,
        panel_layers,
        None,
        seconds,
    );
    crate::util::tag_render_source(&mut objects, crate::util::render_source::USE_MODE_POINTER);
    objects
}

/// The pointer's objects for one frame. Split out from [`PointerVisuals`] so
/// the geometry can be exercised without an asset cache (`glove: None` with
/// `draw_hand` draws the fallback proxy, exactly as a missing model does at
/// runtime; `draw_hand: false` draws no hand at all - see [`pointer_beams`]).
fn render_pointer_rays(
    mut glove: Option<&mut GloveRenderer>,
    draw_hand: bool,
    pass: &FrontendPointerPass,
    canvas_size: Vector2<f32>,
    panel: &WorldPanel,
    panel_layers: usize,
    glove_fit: Option<crate::glove_fit::GloveFit>,
    seconds: f32,
) -> Vec<SceneObject> {
    let mut objects = Vec::new();
    for (index, ray) in pass.rays.iter().enumerate() {
        let geometry =
            pointer_ray_geometry(ray, canvas_size, panel, panel_layers, pass.is_active(index));

        let along = geometry.end - geometry.start;
        let length = along.magnitude();
        // A degenerate beam (the hand pushed right up to its own hit point) has
        // no orientation to speak of, so skip the beam rather than build a NaN
        // transform - but still draw the hand and mark the hit, neither of
        // which needs one.
        if length >= 1e-4 {
            objects.extend(beam_objects(geometry.start, along, length, seconds));
        }

        let hand_start = objects.len();
        match glove.as_deref_mut().filter(|_| draw_hand) {
            Some(glove) => objects.extend(glove.render_static_hand(
                geometry.start,
                ray.rotation,
                ray.handedness,
                pointer_hand_pose(pass, index),
            )),
            // No glove model: keep a proxy so the player can still see where
            // the controller is, rather than a beam growing out of nothing.
            None if draw_hand && length >= 1e-4 => objects.push(box_object(
                geometry.start,
                CONTROLLER_PROXY_SIZE,
                Quaternion::from_arc(vec3(0.0, 0.0, 1.0), along / length, None),
                CONTROLLER_COLOR,
            )),
            None => {}
        }

        if let Some(fit) = glove_fit {
            // Calibrate only the mesh. Hover, clicks, beam and dot retain the
            // single tracked aim pass, so tuning cannot move the menu target.
            let pose = crate::vr_support::GripPose {
                position: geometry.start,
                rotation: ray.rotation,
            };
            let original = Matrix4::from_translation(pose.position) * Matrix4::from(pose.rotation);
            let adjustment = fit.transform(pose, ray.handedness)
                * cgmath::SquareMatrix::invert(&original).expect("tracked hand pose is invertible");
            if fit.visible {
                for object in &mut objects[hand_start..] {
                    object.set_transform(adjustment * object.get_transform());
                }
            } else {
                objects.truncate(hand_start);
            }
        }

        if let Some(dot) = geometry.dot {
            objects.push(dot_object(dot, panel));
        }
    }
    objects
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input_context::{Hand, InputContext};
    use crate::ui::test_support::{hand_aimed_at, hand_aimed_away, test_panel};
    use crate::ui::{canvas_to_panel_world, vr_frontend_pointer_pass};
    use cgmath::{Rotation, Zero, vec2};

    const CANVAS: Vector2<f32> = Vector2 { x: 640.0, y: 480.0 };
    /// A stand-in for what a frontend canvas emits; the exact count only shifts
    /// the dot's clearance.
    const LAYERS: usize = 18;

    fn pass(right: Hand, left: Hand) -> FrontendPointerPass {
        let input = InputContext {
            right_hand: right,
            left_hand: left,
            ..InputContext::default()
        };
        vr_frontend_pointer_pass(&input, CANVAS, &test_panel())
    }

    /// The fallback-proxy pointer for `pass`, at simulation time zero.
    fn rays(
        draw_hand: bool,
        pass: &FrontendPointerPass,
        panel: &WorldPanel,
        fit: Option<crate::glove_fit::GloveFit>,
    ) -> Vec<SceneObject> {
        render_pointer_rays(None, draw_hand, pass, CANVAS, panel, LAYERS, fit, 0.0)
    }

    fn geometry(pass: &FrontendPointerPass, index: usize) -> PointerRayGeometry {
        pointer_ray_geometry(
            &pass.rays[index],
            CANVAS,
            &test_panel(),
            LAYERS,
            pass.is_active(index),
        )
    }

    #[test]
    fn a_beam_ends_just_in_front_of_the_point_the_menu_hit_tested() {
        let target = vec2(500.0, 120.0);
        let pass = pass(hand_aimed_at(CANVAS, target, 0.0), hand_aimed_away(0.0));
        let panel = test_panel();
        let geometry = geometry(&pass, 0);

        let hit = canvas_to_panel_world(CANVAS, &panel, target);
        let dot = geometry.dot.expect("the active ray must show a hit dot");
        assert_eq!(
            geometry.end, dot,
            "the beam must end at the dot, leaving no gap between them"
        );
        // The dot stays *on the ray* - it marks the aimed-at point from any
        // viewpoint, rather than being shoved sideways off the beam.
        let off_ray = (dot - hit) - ray_component(dot - hit, pass.rays[0].direction);
        assert!(off_ray.magnitude() < 1e-4, "the dot must sit on the ray");
        // ...and it clears the panel's whole canvas layer stack, rather than
        // z-fighting the very label it is pointing at.
        assert!(
            (dot - hit).dot(panel.normal()) >= dot_lift(LAYERS) - 1e-4,
            "the dot must clear the panel's canvas layers"
        );
    }

    #[test]
    fn canvas_layers_counts_the_stack_not_the_objects() {
        // Ten labels side by side on the face, one badge two layers up: the
        // dot clears three layers, not ten objects.
        let panel = test_panel();
        let at_layer = |layer: f32| {
            let mut object = box_object(
                vec3(0.0, 0.0, 0.0),
                vec3(1.0, 1.0, 1.0),
                Quaternion::new(1.0, 0.0, 0.0, 0.0),
                CONTROLLER_COLOR,
            );
            object.set_transform(
                panel.transform()
                    * Matrix4::from_translation(vec3(0.0, 0.0, VR_COMPONENT_Z_STEP * layer)),
            );
            object
        };
        let mut canvas: Vec<_> = (0..9).map(|_| at_layer(0.0)).collect();
        canvas.push(at_layer(2.0));
        assert_eq!(canvas_layers(&canvas, &panel), 3);
        assert_eq!(canvas_layers(&[], &panel), 1);
    }

    #[test]
    fn a_grazing_ray_still_lifts_its_dot_clear_of_the_canvas() {
        let panel = test_panel();
        let hit = canvas_to_panel_world(CANVAS, &panel, vec2(500.0, 120.0));
        // Approach 0.1, well under MIN_APPROACH.
        let direction =
            (panel.normal() * -0.1 + panel.rotation.rotate_vector(vec3(1.0, 0.0, 0.0))).normalize();
        let mut ray = pass(
            hand_aimed_at(CANVAS, vec2(500.0, 120.0), 0.0),
            hand_aimed_away(0.0),
        )
        .rays[0];
        ray.origin = hit - direction;
        ray.direction = direction;
        let dot = pointer_ray_geometry(&ray, CANVAS, &panel, LAYERS, true)
            .dot
            .unwrap();
        assert!((dot - hit).dot(panel.normal()) >= dot_lift(LAYERS) - 1e-4);
    }

    fn ray_component(v: Vector3<f32>, direction: Vector3<f32>) -> Vector3<f32> {
        direction * v.dot(direction)
    }

    #[test]
    fn a_beam_that_misses_the_panel_floats_free() {
        let pass = pass(hand_aimed_away(0.0), hand_aimed_away(0.0));
        let geometry = geometry(&pass, 0);
        assert_eq!(geometry.dot, None, "an off-panel ray must show no hit dot");
        assert!(
            ((geometry.end - geometry.start).magnitude() - POINTER_BEAM_MISS_LENGTH).abs() < 1e-3
        );
    }

    #[test]
    fn an_untracked_controller_draws_nothing() {
        // The same zero-quaternion guard that keeps an untracked controller
        // from driving the highlight must keep it from drawing a beam, or the
        // menu grows a ray from the world origin nobody is holding.
        let untracked = Hand {
            rotation: Quaternion::zero(),
            ..Hand::default()
        };
        let pass = pass(untracked.clone(), untracked);
        assert!(pass.rays.is_empty());
        assert!(rays(true, &pass, &test_panel(), None).is_empty());
    }

    #[test]
    fn only_the_controller_the_menu_is_listening_to_shows_a_dot() {
        // Both hands on the panel: the menu highlights one entry, so exactly
        // one dot may appear - two would leave the player unable to tell which
        // one the menu is actually reading.
        let pass = pass(
            hand_aimed_at(CANVAS, vec2(320.0, 240.0), 0.0),
            hand_aimed_at(CANVAS, vec2(100.0, 100.0), 0.0),
        );
        assert_eq!(pass.rays.len(), 2);
        let dots = pass
            .rays
            .iter()
            .enumerate()
            .filter(|(index, _)| geometry(&pass, *index).dot.is_some())
            .count();
        assert_eq!(dots, 1);
        // Each hand: a proxy plus its halo and core; the right hand also the
        // one dot.
        assert_eq!(rays(true, &pass, &test_panel(), None).len(), 2 * 3 + 1);
    }

    #[test]
    fn fit_moves_only_menu_hands_and_visibility_preserves_pointer_feedback() {
        let pass = pass(
            hand_aimed_at(CANVAS, vec2(320.0, 240.0), 0.0),
            hand_aimed_away(0.0),
        );
        let panel = test_panel();
        let fit = crate::glove_fit::GloveFit {
            side_cm: 2.0,
            up_cm: 3.0,
            size: 1.2,
            visible: true,
        };
        let base = rays(true, &pass, &panel, None);
        let adjusted = rays(true, &pass, &panel, Some(fit));
        assert_eq!(base.len(), adjusted.len());
        // All beam segments and the hit dot keep their exact transforms;
        // exactly the two glove proxies change.
        assert_eq!(
            base.iter()
                .zip(&adjusted)
                .filter(|(a, b)| a.get_transform() != b.get_transform())
                .count(),
            2
        );
        let hidden = rays(
            true,
            &pass,
            &panel,
            Some(crate::glove_fit::GloveFit {
                visible: false,
                ..fit
            }),
        );
        let beams = rays(false, &pass, &panel, None);
        assert_eq!(hidden.len(), beams.len());
        for (actual, expected) in hidden.iter().zip(beams) {
            assert_eq!(actual.get_transform(), expected.get_transform());
        }
    }

    #[test]
    fn the_beam_is_a_halo_and_core_from_the_fingertips_to_the_dot() {
        let pass = pass(
            hand_aimed_at(CANVAS, vec2(320.0, 240.0), 0.0),
            hand_aimed_away(0.0),
        );
        let geometry = geometry(&pass, 0);
        let objects = rays(false, &pass, &test_panel(), None);
        // Right hand: halo, core, dot. Left hand (off panel): halo, core.
        assert_eq!(objects.len(), 5);
        let direction = (geometry.end - geometry.start).normalize();
        for beam in &objects[..2] {
            let transform = beam.get_transform();
            // The unit cylinder runs from z = 0 to z = 1.
            let start = transform.w.truncate();
            let end = start + transform.z.truncate();
            let expected_start = geometry.start + direction * BEAM_HAND_CLEARANCE;
            assert!((start - expected_start).magnitude() < 1e-4);
            assert!((end - geometry.end).magnitude() < 1e-4);
            // Blends over the panel instead of punching a hole in its depth.
            assert!(!beam.depth_write);
            assert!(beam.backface_culling().is_some());
        }
    }

    #[test]
    fn a_hand_right_up_against_the_panel_draws_no_beam_but_still_marks_its_hit() {
        // The whole beam would be inside the glove, so drawing it just buries a
        // bright box in the hand. The hit still has to be marked - that is what
        // the player is reading.
        let panel = test_panel();
        let target = vec2(320.0, 240.0);
        let hit = canvas_to_panel_world(CANVAS, &panel, target);
        let close = Hand {
            position: hit + panel.normal() * (BEAM_HAND_CLEARANCE * 0.5),
            rotation: Quaternion::from_arc(vec3(0.0, 0.0, -1.0), -panel.normal(), None),
            ..Hand::default()
        };
        // The other controller is untracked, so only the close hand's objects
        // are in play.
        let pass = pass(
            close,
            Hand {
                rotation: Quaternion::zero(),
                ..Hand::default()
            },
        );
        // Just the proxy and the dot - no beam buried in the hand.
        assert_eq!(rays(true, &pass, &panel, None).len(), 2);
        assert!(pass.rays[0].canvas_hit.is_some());
        assert!(
            geometry(&pass, 0).dot.is_some(),
            "the hit must still be marked"
        );
    }

    #[test]
    fn only_the_hand_the_menu_is_listening_to_points() {
        // Pose is arbitrated by the same pass as the dot: the left hand is on
        // the panel too, but the menu is not reading it, so posing it as
        // pointing would promise a hover it will not get.
        let pass = pass(
            hand_aimed_at(CANVAS, vec2(320.0, 240.0), 0.0),
            hand_aimed_at(CANVAS, vec2(100.0, 100.0), 0.0),
        );
        assert_eq!(pointer_hand_pose(&pass, 0), StaticHandPose::Pointing);
        assert_eq!(pointer_hand_pose(&pass, 1), StaticHandPose::Relaxed);
    }

    #[test]
    fn a_hand_pointing_off_the_panel_stays_relaxed() {
        let pass = pass(hand_aimed_away(0.0), hand_aimed_away(0.0));
        assert!(!pass.rays.is_empty());
        assert!(
            (0..pass.rays.len())
                .all(|index| pointer_hand_pose(&pass, index) == StaticHandPose::Relaxed)
        );
    }

    #[test]
    fn an_idle_hand_on_the_panel_shows_no_dot_while_the_other_holds_its_trigger() {
        // The menu ignores the idle hand here (`vr_frontend_pointer_pass`'s
        // held-hand gate), so drawing its dot would promise a hover the menu
        // will not give and a click it will not take.
        let pass = pass(
            hand_aimed_at(CANVAS, vec2(320.0, 240.0), 0.0),
            hand_aimed_away(1.0),
        );
        assert_eq!(pass.point(), None);
        assert!(pass.pressed);
        assert!(
            pass.rays
                .iter()
                .enumerate()
                .all(|(index, _)| geometry(&pass, index).dot.is_none())
        );
    }
}
