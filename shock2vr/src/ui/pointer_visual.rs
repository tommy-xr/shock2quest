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
    scene::{FrontFaceWinding, SceneObject, color_material, cube},
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
/// Beam cross-section, in metres.
const POINTER_BEAM_THICKNESS: f32 = 0.006;
/// Edge length of the hit dot, in metres.
const POINTER_DOT_SIZE: f32 = 0.03;
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

/// How many pieces the beam is cut into to fade along its length. Enough for
/// the falloff to read as smoke rather than as stripes, few enough that a menu
/// frame stays cheap on a Quest (each piece is a draw, per hand, per eye).
pub const BEAM_SEGMENTS: usize = 6;
/// Transparency (0 = opaque) of the beam where it leaves the hand...
const BEAM_NEAR_TRANSPARENCY: f32 = 0.35;
/// ...and at its far end, just short of invisible.
const BEAM_FAR_TRANSPARENCY: f32 = 0.95;
/// Clear space left at the hand end, so the beam emerges from the glove's
/// fingertips instead of starting inside the palm. Roughly a hand's length.
const BEAM_HAND_CLEARANCE: f32 = 0.25;

const BEAM_COLOR: Vector3<f32> = Vector3 {
    x: 0.3,
    y: 0.8,
    z: 1.0,
};
const DOT_COLOR: Vector3<f32> = Vector3 {
    x: 1.0,
    y: 1.0,
    z: 1.0,
};
const CONTROLLER_COLOR: Vector3<f32> = Vector3 {
    x: 0.6,
    y: 0.6,
    z: 0.7,
};

/// How see-through beam segment `index` of `count` is, hand end first.
///
/// Strictly increasing, so the beam is brightest where it leaves the hand and
/// thins out toward the panel - the hit dot stays the one crisp thing at the
/// far end.
pub fn beam_segment_transparency(index: usize, count: usize) -> f32 {
    debug_assert!(index < count);
    // Sample each segment at its midpoint, so the first segment is not fully at
    // the near value nor the last fully at the far one.
    let t = (index as f32 + 0.5) / count as f32;
    BEAM_NEAR_TRANSPARENCY + (BEAM_FAR_TRANSPARENCY - BEAM_NEAR_TRANSPARENCY) * t
}

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
/// stacked `panel_layers` components on it.
///
/// [`crate::ui::UiCanvas::render_world_space`] steps component *i* forward by
/// `VR_COMPONENT_Z_STEP * i`, so the count of objects it emitted bounds the
/// stack - taking it from the caller keeps this honest as screens gain widgets,
/// where a guessed layer count would silently sink the dot behind a label.
fn dot_lift(panel_layers: usize) -> f32 {
    VR_COMPONENT_Z_STEP * panel_layers as f32 + POINTER_DOT_CLEARANCE
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
    let marker = hit - ray.direction * (dot_lift(panel_layers) / approach.max(MIN_APPROACH));

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

/// The faded beam for one ray, hand end first: [`BEAM_SEGMENTS`] pieces whose
/// alpha falls off along the length.
///
/// Each piece draws in the engine's transparent pass (which masks depth writes
/// for the whole pass), so the segments blend with each other and with the
/// panel behind rather than occluding one another with their own boxes. They
/// also cull backfaces: a double-sided box blends *twice* per pixel, which
/// turns the authored alpha `a` into `1 - (1 - a)^2` and would make the far end
/// read as a solid rod instead of fading out.
fn beam_objects(start: Vector3<f32>, along: Vector3<f32>, length: f32) -> Vec<SceneObject> {
    // The hand fills the first stretch of the ray. A beam no longer than that
    // would be entirely inside the glove, so there is nothing to draw - the
    // hand is already touching what it points at.
    if length <= BEAM_HAND_CLEARANCE {
        return Vec::new();
    }
    let direction = along / length;
    let drawn = length - BEAM_HAND_CLEARANCE;
    let step = drawn / BEAM_SEGMENTS as f32;
    // `from_arc` is fine at the antiparallel extreme: cgmath falls back to an
    // arbitrary perpendicular axis, and the beam is square in cross-section, so
    // the free roll is invisible.
    let aim = Quaternion::from_arc(vec3(0.0, 0.0, 1.0), direction, None);

    (0..BEAM_SEGMENTS)
        .map(|index| {
            let center = start + direction * (BEAM_HAND_CLEARANCE + step * (index as f32 + 0.5));
            let mut object = box_object(
                center,
                vec3(POINTER_BEAM_THICKNESS, POINTER_BEAM_THICKNESS, step),
                aim,
                BEAM_COLOR,
            );
            object.set_transparency(Some(beam_segment_transparency(index, BEAM_SEGMENTS)));
            // The cube's outward faces are clockwise as seen from outside.
            object.set_backface_culling(Some(FrontFaceWinding::Clockwise));
            object
        })
        .collect()
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
    /// `panel_layers` is how many objects the panel's canvas already emitted
    /// (see [`dot_lift`]). Shared by every frontend screen that uses the VR
    /// pointer, so a screen cannot end up with a pointer that hit-tests but
    /// does not show.
    pub fn render(
        &mut self,
        asset_cache: &mut AssetCache,
        pass: &FrontendPointerPass,
        canvas_size: Vector2<f32>,
        panel: &WorldPanel,
        panel_layers: usize,
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
) -> Vec<SceneObject> {
    let mut objects =
        render_pointer_rays(None, false, pass, canvas_size, panel, panel_layers, None);
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
            objects.extend(beam_objects(geometry.start, along, length));
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
            objects.push(box_object(
                dot,
                // Flat against the panel, so the dot reads as a mark on the
                // canvas rather than a cube floating off it.
                vec3(POINTER_DOT_SIZE, POINTER_DOT_SIZE, VR_COMPONENT_Z_STEP),
                panel.rotation,
                DOT_COLOR,
            ));
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
    use cgmath::{Zero, vec2};

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
        assert!(
            render_pointer_rays(None, true, &pass, CANVAS, &test_panel(), LAYERS, None).is_empty()
        );
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
        // Each hand: a proxy plus its beam segments; the right hand also the
        // one dot.
        assert_eq!(
            render_pointer_rays(None, true, &pass, CANVAS, &test_panel(), LAYERS, None).len(),
            2 * (BEAM_SEGMENTS + 1) + 1
        );
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
        let base = render_pointer_rays(None, true, &pass, CANVAS, &panel, LAYERS, None);
        let adjusted = render_pointer_rays(None, true, &pass, CANVAS, &panel, LAYERS, Some(fit));
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
        let hidden = render_pointer_rays(
            None,
            true,
            &pass,
            CANVAS,
            &panel,
            LAYERS,
            Some(crate::glove_fit::GloveFit {
                visible: false,
                ..fit
            }),
        );
        let beams = render_pointer_rays(None, false, &pass, CANVAS, &panel, LAYERS, None);
        assert_eq!(hidden.len(), beams.len());
        for (actual, expected) in hidden.iter().zip(beams) {
            assert_eq!(actual.get_transform(), expected.get_transform());
        }
    }

    #[test]
    fn the_beam_fades_along_its_length() {
        // "Smoky": brightest at the hand, thinning toward the panel, so the
        // crisp dot at the end is the thing the eye lands on. A beam of one
        // flat alpha (or one that brightened toward the panel) would read as a
        // solid rod and compete with the dot.
        let alphas: Vec<f32> = (0..BEAM_SEGMENTS)
            .map(|index| beam_segment_transparency(index, BEAM_SEGMENTS))
            .collect();
        assert!(
            alphas.windows(2).all(|pair| pair[1] > pair[0]),
            "beam transparency must increase away from the hand: {alphas:?}"
        );
        assert!(
            alphas.iter().all(|a| (0.0..=1.0).contains(a)),
            "transparency must stay in range: {alphas:?}"
        );
        // Translucent at the hand too, not just at the tip.
        assert!(alphas[0] > 0.0);
    }

    #[test]
    fn every_beam_segment_is_translucent_and_single_sided() {
        // A double-sided box blends twice per pixel, turning alpha `a` into
        // `1 - (1 - a)^2` - the far end would read as a solid rod rather than
        // fading out, so the authored ramp only means what it says with the
        // inner faces culled.
        let pass = pass(
            hand_aimed_at(CANVAS, vec2(320.0, 240.0), 0.0),
            hand_aimed_away(0.0),
        );
        let objects = render_pointer_rays(None, true, &pass, CANVAS, &test_panel(), LAYERS, None);
        let translucent: Vec<_> = objects
            .iter()
            .filter(|object| object.effective_transparency().is_some_and(|t| t > 0.0))
            .collect();
        assert_eq!(translucent.len(), 2 * BEAM_SEGMENTS);
        assert!(
            translucent
                .iter()
                .all(|object| object.backface_culling().is_some())
        );
        // The dot stays opaque - it is the focus point.
        let dot = objects.last().expect("a dot must be drawn");
        assert_eq!(dot.effective_transparency(), None);
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
        let objects = render_pointer_rays(None, true, &pass, CANVAS, &panel, LAYERS, None);
        assert!(
            objects
                .iter()
                .all(|object| object.effective_transparency().is_none()),
            "no translucent beam segment may be drawn inside the hand"
        );
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
