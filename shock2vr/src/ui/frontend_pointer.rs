//! The VR pointer shared by every frontend screen.
//!
//! In VR there is no 2D cursor: the "pointer" is where a controller ray meets
//! the screen's anchored world panel, and the trigger is the button. That rule is
//! identical for the main menu, the load screen and the game-over screen, so it
//! lives here once rather than being re-derived per scene - the same reason
//! placement lives in one layout pass (see `AGENTS.md` section 3): independent
//! copies drift silently.

use cgmath::{InnerSpace, Quaternion, Rotation, Vector2, Vector3, vec3};

use crate::{
    input_context::{Hand, InputContext},
    ui::{WorldPanel, ray_to_canvas},
    vr_config::Handedness,
};

/// A VR trigger past this counts as "pressed", matching the hand code's
/// grab/fire threshold.
pub const VR_TRIGGER_THRESHOLD: f32 = 0.5;

/// The direction a hand points, or `None` when the controller is not tracked.
///
/// An untracked hand reports a zero quaternion rather than an identity one.
/// Rotating by it collapses the ray to the zero vector, so guarding here keeps
/// an untracked controller from being treated as a hand aimed anywhere at all.
fn hand_ray(rotation: Quaternion<f32>) -> Option<(Quaternion<f32>, Vector3<f32>)> {
    if rotation.magnitude2() < 1e-6 {
        return None;
    }
    let rotation = rotation.normalize();
    Some((rotation, rotation.rotate_vector(vec3(0.0, 0.0, -1.0))))
}

fn held(hand: &Hand) -> bool {
    hand.trigger_value > VR_TRIGGER_THRESHOLD
}

/// One controller's aim ray against a frontend panel: where it starts, where it
/// points, and where it lands on the canvas (if it lands at all).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FrontendRay {
    /// The controller's position, in world space.
    pub origin: Vector3<f32>,
    /// The direction the controller points (its local -Z), normalized.
    pub direction: Vector3<f32>,
    /// Where the ray meets the panel, in canvas pixels, or `None` when it
    /// misses.
    pub canvas_hit: Option<Vector2<f32>>,
    /// Which hand this ray belongs to, so the drawn hand is the right one -
    /// the glove model is right-handed and mirrored for the left.
    pub handedness: Handedness,
    /// The controller's own orientation, normalized. The aim is derived from
    /// it, but the drawn hand needs the full rotation (roll included), and it
    /// must be the same one the aim came from or hand and beam disagree.
    pub rotation: Quaternion<f32>,
}

/// The ray for one hand, or `None` when that controller is not tracked.
///
/// The one place a hand becomes a ray: everything that pointing means - the
/// tracked guard, the -Z aim, the panel intersection - happens here once.
fn frontend_ray(
    hand: &Hand,
    handedness: Handedness,
    canvas_size: Vector2<f32>,
    panel: &WorldPanel,
) -> Option<FrontendRay> {
    let (rotation, direction) = hand_ray(hand.rotation)?;
    Some(FrontendRay {
        origin: hand.position,
        direction,
        canvas_hit: ray_to_canvas(canvas_size, panel, hand.position, direction),
        handedness,
        rotation,
    })
}

/// One frame of frontend pointing: every tracked controller's ray, *which* of
/// them the menu is listening to, and whether a trigger is down.
///
/// This is the single unit of frontend pointing. The hover highlight, the click
/// and the drawn hit dot ([`crate::ui::PointerVisuals`]) all read the same
/// pass, so the dot cannot appear anywhere but the pixel the menu hit-tested,
/// and a controller the menu is ignoring cannot draw a dot that says otherwise.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FrontendPointerPass {
    /// Every tracked controller's ray, right hand first. An untracked
    /// controller contributes nothing, so it draws nothing - the same guard
    /// that keeps it from driving the highlight.
    pub rays: Vec<FrontendRay>,
    /// Index into `rays` of the controller actually driving the menu, when one
    /// of them is on the panel.
    active: Option<usize>,
    /// Whether either trigger is held.
    pub pressed: bool,
}

impl FrontendPointerPass {
    /// Where the menu is being pointed, in canvas pixels.
    pub fn point(&self) -> Option<Vector2<f32>> {
        self.active.and_then(|index| self.rays[index].canvas_hit)
    }

    /// Whether `rays[index]` is the ray the menu is listening to.
    pub fn is_active(&self, index: usize) -> bool {
        self.active == Some(index)
    }
}

/// Resolve one frame of frontend pointing.
///
/// Both controllers are always posed in VR, so "whichever hand hits the panel"
/// would always resolve to the same one. Instead a hand **with its trigger
/// held** wins, so either controller can click; ties and idle triggers fall
/// back to the right hand, which then drives the hover highlight.
pub fn vr_frontend_pointer_pass(
    input_context: &InputContext,
    canvas_size: Vector2<f32>,
    panel: &WorldPanel,
) -> FrontendPointerPass {
    let hands = [&input_context.right_hand, &input_context.left_hand];
    let handedness = [Handedness::Right, Handedness::Left];

    let mut rays = Vec::with_capacity(hands.len());
    // Where each hand's ray landed in `rays`, since untracked hands are skipped.
    let mut ray_of_hand = [None, None];
    for (slot, hand) in hands.iter().enumerate() {
        if let Some(ray) = frontend_ray(hand, handedness[slot], canvas_size, panel) {
            ray_of_hand[slot] = Some(rays.len());
            rays.push(ray);
        }
    }

    let any_held = held(hands[0]) || held(hands[1]);
    let order = if held(hands[1]) && !held(hands[0]) {
        [1, 0]
    } else {
        [0, 1]
    };

    let mut active = None;
    for slot in order {
        // While a trigger is down, only the hand holding it may supply the
        // point: otherwise an idle hand resting on the panel would report
        // "not pressed" and clear the held state, so sweeping the pressed hand
        // onto a button would read as a fresh edge and click it.
        if any_held && !held(hands[slot]) {
            continue;
        }
        let Some(index) = ray_of_hand[slot] else {
            continue;
        };
        if rays[index].canvas_hit.is_some() {
            active = Some(index);
            break;
        }
    }

    FrontendPointerPass {
        rays,
        active,
        // Nothing may be pointing at the screen; report the trigger anyway so a
        // press that starts off-panel is still consumed as "held" rather than
        // becoming a fresh edge the moment the ray crosses onto a button.
        pressed: any_held,
    }
}

/// Test-only rig for aiming a controller at a canvas point, shared by the
/// frontend scenes' tests so each one exercises the real `vr_frontend_pointer`
/// rather than a hand-rolled approximation of it.
#[cfg(test)]
pub mod test_support {
    use super::*;
    use crate::ui::{FRONTEND_PANEL_DISTANCE, FrontendPanelAnchor, canvas_to_panel_world};
    use std::time::Duration;

    /// The head facing the tests aim against: the default camera orientation.
    pub fn test_head() -> Quaternion<f32> {
        Quaternion::new(1.0, 0.0, 0.0, 0.0)
    }

    /// The panel the tests aim at: what a frontend scene's anchor places on
    /// entry from the default head pose. Built through the real anchor so the
    /// tests track placement changes instead of re-deriving them.
    pub fn test_panel() -> WorldPanel {
        FrontendPanelAnchor::new().update(
            crate::input_context::Head::default().position,
            test_head(),
            Duration::ZERO,
        )
    }

    /// A hand aimed at a given canvas point, derived from the panel's own basis
    /// rather than world axes - so the tests stay honest whichever way the
    /// panel ends up facing. This is the inverse of [`ray_to_canvas`].
    pub fn hand_aimed_at(canvas_size: Vector2<f32>, point: Vector2<f32>, trigger: f32) -> Hand {
        let panel = test_panel();
        let normal = panel.normal();
        let target = canvas_to_panel_world(canvas_size, &panel, point);

        Hand {
            // Stand back on the viewer's side (the panel's normal points at the
            // viewer) and aim at the target.
            position: target + normal * FRONTEND_PANEL_DISTANCE,
            rotation: Quaternion::from_arc(vec3(0.0, 0.0, -1.0), -normal, None),
            trigger_value: trigger,
            ..Hand::default()
        }
    }

    /// A hand on the viewer's side of the panel, aimed directly away from it.
    ///
    /// The origin is deliberately off the panel plane: sitting exactly on
    /// `panel.center` makes `ray_to_canvas` bail on distance before it ever
    /// looks at the direction, so the test would pass even aimed at the panel.
    pub fn hand_aimed_away(trigger: f32) -> Hand {
        let panel = test_panel();
        // The panel's normal points at the viewer, so this stands in front of
        // it and looks the other way.
        let normal = panel.normal();
        Hand {
            position: panel.center + normal * FRONTEND_PANEL_DISTANCE,
            rotation: Quaternion::from_arc(vec3(0.0, 0.0, -1.0), normal, None),
            trigger_value: trigger,
            ..Hand::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::*;
    use super::*;
    use cgmath::{Zero, vec2};

    const CANVAS: Vector2<f32> = Vector2 { x: 640.0, y: 480.0 };

    fn vr_input(right: Hand, left: Hand) -> InputContext {
        InputContext {
            right_hand: right,
            left_hand: left,
            ..InputContext::default()
        }
    }

    #[test]
    fn an_untracked_hand_yields_no_pointer() {
        // A zero rotation is what a runtime reports for a controller that is
        // not tracked. Treating it as a rotation would aim a ray straight down
        // -Z from the origin, which can land on the panel and drive the
        // highlight (or a click) from a controller nobody is holding.
        let untracked = Hand {
            rotation: Quaternion::zero(),
            // At eye level, where a hand held up in front of the player sits.
            position: crate::input_context::Head::default().position,
            trigger_value: 1.0,
            ..Hand::default()
        };
        let pass = vr_frontend_pointer_pass(
            &vr_input(untracked.clone(), untracked),
            CANVAS,
            &test_panel(),
        );
        assert_eq!(
            pass.point(),
            None,
            "an untracked controller must not report a pointer"
        );
    }

    #[test]
    fn a_tracked_hand_still_points_when_the_other_is_untracked() {
        let untracked = Hand {
            rotation: Quaternion::zero(),
            ..Hand::default()
        };
        let target = vec2(320.0, 240.0);
        let pass = vr_frontend_pointer_pass(
            &vr_input(untracked, hand_aimed_at(CANVAS, target, 1.0)),
            CANVAS,
            &test_panel(),
        );
        let (point, pressed) = (pass.point(), pass.pressed);
        let point = point.expect("the tracked hand should land on the panel");
        assert!((point.x - target.x).abs() < 1.0 && (point.y - target.y).abs() < 1.0);
        assert!(pressed);
    }

    #[test]
    fn an_idle_hand_on_the_panel_does_not_release_the_other_hands_trigger() {
        // Left trigger held but aimed off-panel, right hand idle and resting on
        // it. Reporting the right hand's idle trigger would clear `last_pressed`
        // and turn the still-held left press into a fresh edge as soon as it
        // swept onto a button - a click nobody made.
        let input = vr_input(
            hand_aimed_at(CANVAS, vec2(320.0, 240.0), 0.0),
            hand_aimed_away(1.0),
        );
        let pass = vr_frontend_pointer_pass(&input, CANVAS, &test_panel());
        let (point, pressed) = (pass.point(), pass.pressed);
        assert!(pressed, "a held trigger must stay reported as held");
        assert_eq!(
            point, None,
            "the idle hand must not supply a point while the other is pressed"
        );
    }

    #[test]
    fn aiming_away_yields_no_point_but_keeps_the_trigger() {
        let pass = vr_frontend_pointer_pass(
            &vr_input(hand_aimed_away(1.0), hand_aimed_away(1.0)),
            CANVAS,
            &test_panel(),
        );
        let (point, pressed) = (pass.point(), pass.pressed);
        assert_eq!(point, None);
        assert!(pressed);
    }
}
