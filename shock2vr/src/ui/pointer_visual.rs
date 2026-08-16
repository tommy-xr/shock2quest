//! The visible half of the VR frontend pointer: a proxy at each tracked
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
use engine::scene::{SceneObject, color_material, cube};

use crate::ui::{
    FrontendPointerPass, FrontendRay, VR_COMPONENT_Z_STEP, WorldPanel, canvas_to_panel_world,
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
/// Size of the controller proxy: a stubby box at the hand, aimed down the ray.
const CONTROLLER_PROXY_SIZE: Vector3<f32> = Vector3 {
    x: 0.035,
    y: 0.035,
    z: 0.09,
};

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

/// The scene objects for a frame of frontend pointing: a proxy and beam for
/// every tracked controller, plus a dot on the one the menu is listening to.
///
/// `panel_layers` is how many objects the panel's canvas already emitted (see
/// [`dot_lift`]). Shared by every frontend screen that uses the VR pointer, so
/// a screen cannot end up with a pointer that hit-tests but does not show.
pub fn render_pointer_rays(
    pass: &FrontendPointerPass,
    canvas_size: Vector2<f32>,
    panel: &WorldPanel,
    panel_layers: usize,
) -> Vec<SceneObject> {
    let mut objects = Vec::new();
    for (index, ray) in pass.rays.iter().enumerate() {
        let geometry =
            pointer_ray_geometry(ray, canvas_size, panel, panel_layers, pass.is_active(index));

        let along = geometry.end - geometry.start;
        let length = along.magnitude();
        // A degenerate beam (the hand pushed right up to its own hit point) has
        // no orientation to speak of, so skip the beam and proxy rather than
        // build a NaN transform - but still mark the hit, which needs none.
        if length >= 1e-4 {
            // `from_arc` is fine at the antiparallel extreme: cgmath falls back
            // to an arbitrary perpendicular axis, and both the beam and the
            // proxy are square in cross-section, so the free roll is invisible.
            let aim = Quaternion::from_arc(vec3(0.0, 0.0, 1.0), along / length, None);
            objects.push(box_object(
                geometry.start,
                CONTROLLER_PROXY_SIZE,
                aim,
                CONTROLLER_COLOR,
            ));
            objects.push(box_object(
                geometry.start + along * 0.5,
                vec3(POINTER_BEAM_THICKNESS, POINTER_BEAM_THICKNESS, length),
                aim,
                BEAM_COLOR,
            ));
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
        assert!(render_pointer_rays(&pass, CANVAS, &test_panel(), LAYERS).is_empty());
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
        // Right hand: proxy + beam + dot. Left hand: proxy + beam.
        assert_eq!(
            render_pointer_rays(&pass, CANVAS, &test_panel(), LAYERS).len(),
            5
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
