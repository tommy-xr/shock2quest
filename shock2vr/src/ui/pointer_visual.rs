//! The visible half of the VR frontend pointer: a gloved hand at each tracked
//! controller, plus a beam and a dot from the one the menu is listening to.
//!
//! Without this a VR menu gives no feedback until an entry happens to light up,
//! so aiming is guesswork (issue #1001). Everything here is derived from the
//! [`FrontendPointerPass`] the menu itself hit-tested - the beam ends at the
//! *canvas* hit point, mapped back through [`canvas_to_panel_world`] - so it
//! ends on the pixel the menu hit-tested and the two cannot drift apart. A
//! controller aimed off the panel, or one the menu is ignoring, draws no beam.

use cgmath::{InnerSpace, Matrix4, Quaternion, Vector2, Vector3, vec3};
use engine::{
    assets::asset_cache::AssetCache,
    scene::{SceneObject, color_material, cube, laser_material, quad},
};

use crate::{
    hand_glove::{GloveRenderer, StaticHandPose},
    ui::{
        FrontendPointerPass, FrontendRay, VR_COMPONENT_Z_STEP, WorldPanel, canvas_to_panel_world,
    },
};

/// Radii of the beam's soft halo and bright core, in metres.
const BEAM_HALO_RADIUS: f32 = 0.012;
const BEAM_CORE_RADIUS: f32 = 0.0025;
/// Clear air between the panel's frontmost canvas layer and the beam's tip.
const BEAM_TIP_CLEARANCE: f32 = VR_COMPONENT_Z_STEP * 2.0;
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

/// A beam's `[halo, core]` colours.
type BeamColors = [Vector3<f32>; 2];
/// Frontend and pause menus: blue.
const MENU_BEAM: BeamColors = [vec3(0.05, 0.55, 1.0), vec3(0.4, 0.85, 1.0)];
/// In-play UI devices (cyber interface, tricorder): green.
const DEVICE_BEAM: BeamColors = [vec3(0.02, 1.0, 0.15), vec3(0.25, 1.0, 0.35)];

/// How one caller's pointer looks: beam colours and hit-dot size (0 = none).
#[derive(Clone, Copy)]
struct PointerStyle {
    colors: BeamColors,
    dot_size: f32,
    both_hands: bool,
}

impl PointerStyle {
    fn with_dev_dot(colors: BeamColors) -> Self {
        Self {
            colors,
            both_hands: false,
            dot_size: crate::dev_params::get(crate::dev_params::VR_POINTER_DOT_SIZE),
        }
    }
}
const CONTROLLER_COLOR: Vector3<f32> = Vector3 {
    x: 0.6,
    y: 0.6,
    z: 0.7,
};

/// The pose the hand driving `rays[index]` is shown in.
///
/// Read off the same pass that decides the hover highlight and the click, so
/// the hand points exactly when the menu is listening to it - a
/// hand posed as pointing can never promise a hover the menu will not give.
pub fn pointer_hand_pose(pass: &FrontendPointerPass, index: usize) -> StaticHandPose {
    if pass.is_active(index) {
        StaticHandPose::Pointing
    } else {
        StaticHandPose::Relaxed
    }
}

/// How far in front of the panel face the beam's tip stops, given a canvas
/// that stacked `panel_layers` layers on it (see [`canvas_layers`]).
fn tip_lift(panel_layers: usize) -> f32 {
    VR_COMPONENT_Z_STEP * panel_layers as f32 + BEAM_TIP_CLEARANCE
}

/// How many `VR_COMPONENT_Z_STEP` layers a world-space canvas stacks on
/// `panel`, read off its frontmost object. The canvas steps only overlapping
/// art forward, so its object count overstates the stack many times over -
/// enough to end the beam centimetres off a near panel like the tricorder's.
pub fn canvas_layers(canvas: &[SceneObject], panel: &WorldPanel) -> usize {
    let front = canvas
        .iter()
        .map(|object| (object.get_transform().w.truncate() - panel.center).dot(panel.normal()))
        .fold(0.0, f32::max);
    (front / VR_COMPONENT_Z_STEP).round() as usize + 1
}

/// Where one ray's beam ends, in world space: just clear of the panel point
/// the menu hit-tested, or `None` when the ray misses the panel.
///
/// Pure geometry, so the placement rule is testable without a renderer.
pub fn pointer_beam_end(
    ray: &FrontendRay,
    canvas_size: Vector2<f32>,
    panel: &WorldPanel,
    panel_layers: usize,
) -> Option<Vector3<f32>> {
    // The hit comes back from the canvas point the menu hit-tested, not from a
    // second intersection of the same ray.
    let hit = canvas_to_panel_world(canvas_size, panel, ray.canvas_hit?);

    // Lift perpendicular to the canvas so the marker retains the clicked
    // pixel's X/Y. Pulling back along an oblique controller ray displaces the
    // dot across small MFD buttons when viewed from the player's eyes.
    Some(hit + panel.normal() * tip_lift(panel_layers))
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

/// The smoky beam for one ray: the laser sight's halo and core.
fn beam_objects(
    start: Vector3<f32>,
    along: Vector3<f32>,
    length: f32,
    [halo, core]: BeamColors,
    seconds: f32,
) -> Vec<SceneObject> {
    // The hand fills the first stretch of the ray. A beam no longer than that
    // would be entirely inside the glove, so there is nothing to draw - the
    // hand is already touching what it points at.
    if length <= BEAM_HAND_CLEARANCE {
        return Vec::new();
    }
    let direction = along / length;
    laser_material::beam(
        start + direction * BEAM_HAND_CLEARANCE,
        direction * (length - BEAM_HAND_CLEARANCE),
        (BEAM_HALO_RADIUS, halo),
        (BEAM_CORE_RADIUS, core),
        seconds,
    )
    .into()
}

/// The glowing hit dot: the laser sight's spot in the beam's core colour, flat
/// against the panel and `size` across. It writes no depth, so it shows over
/// the canvas only because callers emit the pointer after it (the transparent
/// pass draws in emit order).
fn dot_object(
    center: Vector3<f32>,
    panel: &WorldPanel,
    color: Vector3<f32>,
    size: f32,
) -> SceneObject {
    let mut object = SceneObject::new(laser_material::create(color), Box::new(quad::create()));
    object.set_transform(
        Matrix4::from_translation(center)
            * Matrix4::from(panel.rotation)
            * Matrix4::from_nonuniform_scale(size, size, 1.0),
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

    /// The scene objects for a frame of frontend pointing: a hand for every
    /// tracked controller, plus a beam and dot from the one the menu is
    /// listening to.
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
            PointerStyle::with_dev_dot(MENU_BEAM),
            seconds,
        );
        // The one pair of hands a frontend screen shows. Labelled so a check
        // can assert *both* halves of issue #1018's fix: the scene's hands are
        // gone, and these are still there.
        crate::util::tag_render_source(&mut objects, crate::util::render_source::FRONTEND_POINTER);
        objects
    }
}

/// The aim beam and hit dot alone, with no hands at the controllers.
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
    both_hands: bool,
) -> Vec<SceneObject> {
    let mut objects = render_pointer_rays(
        None,
        false,
        pass,
        canvas_size,
        panel,
        panel_layers,
        None,
        PointerStyle {
            both_hands,
            ..PointerStyle::with_dev_dot(DEVICE_BEAM)
        },
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
    style: PointerStyle,
    seconds: f32,
) -> Vec<SceneObject> {
    let mut objects = Vec::new();
    for (index, ray) in pass.rays.iter().enumerate() {
        // Inventory presents both hands identically. Input arbitration still
        // chooses the widget that receives input; it does not style the rays.
        let end = (pass.is_active(index) || style.both_hands)
            .then(|| pointer_beam_end(ray, canvas_size, panel, panel_layers))
            .flatten();
        if let Some(end) = end {
            let along = end - ray.origin;
            objects.extend(beam_objects(
                ray.origin,
                along,
                along.magnitude(),
                style.colors,
                seconds,
            ));
        }

        let hand_start = objects.len();
        match glove.as_deref_mut().filter(|_| draw_hand) {
            Some(glove) => objects.extend(glove.render_static_hand(
                ray.origin,
                ray.rotation,
                ray.handedness,
                pointer_hand_pose(pass, index),
            )),
            // No glove model: keep a proxy so the player can still see where
            // the controller is, rather than a beam growing out of nothing.
            None if draw_hand => objects.push(box_object(
                ray.origin,
                CONTROLLER_PROXY_SIZE,
                Quaternion::from_arc(vec3(0.0, 0.0, 1.0), ray.direction, None),
                CONTROLLER_COLOR,
            )),
            None => {}
        }

        if let Some(fit) = glove_fit {
            // Calibrate only the mesh. Hover, clicks and the beam retain the
            // single tracked aim pass, so tuning cannot move the menu target.
            let pose = crate::vr_support::GripPose {
                position: ray.origin,
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

        if let Some(end) = end.filter(|_| style.dot_size > 0.0) {
            // On a handheld screen a metre-sized preference can obscure an
            // entire node. Limit the dot to sixteen canvas pixels across.
            let size = style.dot_size.min(16.0 * panel.size.x / canvas_size.x);
            objects.push(dot_object(end, panel, style.colors[1], size));
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
    /// the beam tip's clearance.
    const LAYERS: usize = 18;
    const STYLE: PointerStyle = PointerStyle {
        colors: MENU_BEAM,
        dot_size: 0.05,
        both_hands: false,
    };

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
        render_pointer_rays(
            None, draw_hand, pass, CANVAS, panel, LAYERS, fit, STYLE, 0.0,
        )
    }

    fn end(pass: &FrontendPointerPass, index: usize) -> Option<Vector3<f32>> {
        pointer_beam_end(&pass.rays[index], CANVAS, &test_panel(), LAYERS)
    }

    #[test]
    fn a_beam_ends_just_in_front_of_the_point_the_menu_hit_tested() {
        let target = vec2(500.0, 120.0);
        let pass = pass(hand_aimed_at(CANVAS, target, 0.0), hand_aimed_away(0.0));
        let panel = test_panel();
        let hit = canvas_to_panel_world(CANVAS, &panel, target);
        let tip = end(&pass, 0).expect("a ray on the panel must draw a beam");
        let displacement = tip - hit;
        let lateral = displacement - panel.normal() * displacement.dot(panel.normal());
        assert!(
            lateral.magnitude() < 1e-4,
            "the tip must mark the clicked canvas pixel"
        );
        // ...and it clears the panel's whole canvas layer stack, rather than
        // burying itself in the very label it is pointing at.
        assert!(
            (tip - hit).dot(panel.normal()) >= tip_lift(LAYERS) - 1e-4,
            "the tip must clear the panel's canvas layers"
        );
    }

    #[test]
    fn canvas_layers_counts_the_stack_not_the_objects() {
        // Ten labels side by side on the face, one badge two layers up: the
        // tip clears three layers, not ten objects.
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
    fn a_grazing_ray_still_lifts_its_tip_clear_of_the_canvas() {
        let panel = test_panel();
        let hit = canvas_to_panel_world(CANVAS, &panel, vec2(500.0, 120.0));
        // A strongly oblique controller ray must not displace the marker.
        let direction =
            (panel.normal() * -0.1 + panel.rotation.rotate_vector(vec3(1.0, 0.0, 0.0))).normalize();
        let mut ray = pass(
            hand_aimed_at(CANVAS, vec2(500.0, 120.0), 0.0),
            hand_aimed_away(0.0),
        )
        .rays[0];
        ray.origin = hit - direction;
        ray.direction = direction;
        let tip = pointer_beam_end(&ray, CANVAS, &panel, LAYERS).unwrap();
        assert!((tip - hit).dot(panel.normal()) >= tip_lift(LAYERS) - 1e-4);
    }

    #[test]
    fn a_ray_that_misses_the_panel_draws_no_beam() {
        let pass = pass(hand_aimed_away(0.0), hand_aimed_away(0.0));
        assert_eq!(pass.rays.len(), 2);
        assert!(rays(false, &pass, &test_panel(), None).is_empty());
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
    fn only_the_controller_the_menu_is_listening_to_draws_a_beam_and_dot() {
        // Both hands on the panel, but the menu reads one: a second beam would
        // end on a button that will not hover or click.
        let pass = pass(
            hand_aimed_at(CANVAS, vec2(320.0, 240.0), 0.0),
            hand_aimed_at(CANVAS, vec2(100.0, 100.0), 0.0),
        );
        assert_eq!(pass.rays.len(), 2);
        // Two proxies, plus one halo, core and dot.
        assert_eq!(rays(true, &pass, &test_panel(), None).len(), 2 + 3);
    }

    #[test]
    fn inventory_shows_matching_beams_and_hit_dots_for_both_hands() {
        let pass = pass(
            hand_aimed_at(CANVAS, vec2(320.0, 240.0), 0.0),
            hand_aimed_at(CANVAS, vec2(100.0, 100.0), 0.0),
        );
        let objects = render_pointer_rays(
            None,
            false,
            &pass,
            CANVAS,
            &test_panel(),
            LAYERS,
            None,
            PointerStyle {
                both_hands: true,
                ..STYLE
            },
            0.0,
        );
        // Each hand gets a halo, core and hit dot; no duplicate gloves.
        assert_eq!(objects.len(), 6);
        let missing = pass.remap_hits(|_| None);
        assert!(
            render_pointer_rays(
                None,
                false,
                &missing,
                CANVAS,
                &test_panel(),
                LAYERS,
                None,
                PointerStyle {
                    both_hands: true,
                    ..STYLE
                },
                0.0
            )
            .is_empty()
        );
    }

    #[test]
    fn a_zero_dot_size_hides_the_dot_but_keeps_the_beam() {
        let pass = pass(
            hand_aimed_at(CANVAS, vec2(320.0, 240.0), 0.0),
            hand_aimed_away(0.0),
        );
        let objects = render_pointer_rays(
            None,
            false,
            &pass,
            CANVAS,
            &test_panel(),
            LAYERS,
            None,
            PointerStyle {
                dot_size: 0.0,
                ..STYLE
            },
            0.0,
        );
        // Halo and core only.
        assert_eq!(objects.len(), 2);
    }

    #[test]
    fn an_idle_hand_on_the_panel_draws_nothing_while_the_other_holds_its_trigger() {
        // The menu ignores the idle hand here (`vr_frontend_pointer_pass`'s
        // held-hand gate), so its beam would promise a click it will not take.
        let pass = pass(
            hand_aimed_at(CANVAS, vec2(320.0, 240.0), 0.0),
            hand_aimed_away(1.0),
        );
        assert_eq!(pass.point(), None);
        assert!(pass.rays[0].canvas_hit.is_some());
        assert!(rays(false, &pass, &test_panel(), None).is_empty());
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
        // Both beam cylinders and the dot keep their exact transforms;
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
    fn the_beam_is_a_halo_and_core_from_the_fingertips_to_the_panel() {
        let pass = pass(
            hand_aimed_at(CANVAS, vec2(320.0, 240.0), 0.0),
            hand_aimed_away(0.0),
        );
        let origin = pass.rays[0].origin;
        let tip = end(&pass, 0).unwrap();
        let objects = rays(false, &pass, &test_panel(), None);
        // Right hand: halo, core, dot. Left hand (off panel): nothing.
        assert_eq!(objects.len(), 3);
        let direction = (tip - origin).normalize();
        for beam in &objects[..2] {
            let transform = beam.get_transform();
            // The unit cylinder runs from z = 0 to z = 1.
            let start = transform.w.truncate();
            let end = start + transform.z.truncate();
            let expected_start = origin + direction * BEAM_HAND_CLEARANCE;
            assert!((start - expected_start).magnitude() < 1e-4);
            assert!((end - tip).magnitude() < 1e-4);
            // Blends over the panel instead of punching a hole in its depth.
            assert!(!beam.depth_write);
            assert!(beam.backface_culling().is_some());
        }
    }

    #[test]
    fn a_hand_right_up_against_the_panel_draws_no_beam() {
        // The whole beam would be inside the glove, so drawing it just buries a
        // bright cylinder in the hand.
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
        assert!(pass.rays[0].canvas_hit.is_some());
        // Just the proxy and the dot - no beam buried in the hand.
        assert_eq!(rays(true, &pass, &panel, None).len(), 2);
    }

    #[test]
    fn only_the_hand_the_menu_is_listening_to_points() {
        // Pose is arbitrated by the same pass as the hover: the left hand is on
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
}
