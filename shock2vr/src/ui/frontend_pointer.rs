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
};

/// A VR trigger past this counts as "pressed", matching the hand code's
/// grab/fire threshold.
pub const VR_TRIGGER_THRESHOLD: f32 = 0.5;

/// The direction a hand points, or `None` when the controller is not tracked.
///
/// An untracked hand reports a zero quaternion rather than an identity one.
/// Rotating by it collapses the ray to the zero vector, so guarding here keeps
/// an untracked controller from being treated as a hand aimed anywhere at all.
fn hand_ray(rotation: Quaternion<f32>) -> Option<Vector3<f32>> {
    if rotation.magnitude2() < 1e-6 {
        return None;
    }
    Some(rotation.normalize().rotate_vector(vec3(0.0, 0.0, -1.0)))
}

fn held(hand: &Hand) -> bool {
    hand.trigger_value > VR_TRIGGER_THRESHOLD
}

/// Where a hand is pointing on a frontend screen's panel, in canvas pixels,
/// plus whether its trigger is held.
///
/// Both controllers are always posed in VR, so "whichever hand hits the panel"
/// would always resolve to the same one. Instead a hand **with its trigger
/// held** wins, so either controller can click; ties and idle triggers fall
/// back to the right hand, which then drives the hover highlight.
pub fn vr_frontend_pointer(
    input_context: &InputContext,
    canvas_size: Vector2<f32>,
    panel: &WorldPanel,
) -> (Option<Vector2<f32>>, bool) {
    let right = &input_context.right_hand;
    let left = &input_context.left_hand;

    let any_held = held(right) || held(left);
    let order = if held(left) && !held(right) {
        [left, right]
    } else {
        [right, left]
    };

    for hand in order {
        // While a trigger is down, only the hand holding it may supply the
        // point: otherwise an idle hand resting on the panel would report
        // "not pressed" and clear the held state, so sweeping the pressed hand
        // onto a button would read as a fresh edge and click it.
        if any_held && !held(hand) {
            continue;
        }
        let Some(direction) = hand_ray(hand.rotation) else {
            continue;
        };
        if let Some(point) = ray_to_canvas(canvas_size, panel, hand.position, direction) {
            return (Some(point), any_held);
        }
    }

    // Nothing points at the screen; report the trigger anyway so a press that
    // starts off-panel is still consumed as "held" rather than becoming a fresh
    // edge the moment the ray crosses onto a button.
    (None, any_held)
}

/// Test-only rig for aiming a controller at a canvas point, shared by the
/// frontend scenes' tests so each one exercises the real `vr_frontend_pointer`
/// rather than a hand-rolled approximation of it.
#[cfg(test)]
pub mod test_support {
    use super::*;
    use crate::ui::{FRONTEND_PANEL_DISTANCE, FrontendPanelAnchor};
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
        let u = point.x / canvas_size.x - 0.5;
        let v = 0.5 - point.y / canvas_size.y;
        let right = panel.rotation.rotate_vector(vec3(1.0, 0.0, 0.0));
        let up = panel.rotation.rotate_vector(vec3(0.0, 1.0, 0.0));
        let normal = panel.normal();
        let target = panel.center + right * (u * panel.size.x) + up * (v * panel.size.y);

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
            position: vec3(0.0, crate::ui::FRONTEND_PANEL_EYE_HEIGHT, 0.0),
            trigger_value: 1.0,
            ..Hand::default()
        };
        let (point, _) = vr_frontend_pointer(
            &vr_input(untracked.clone(), untracked),
            CANVAS,
            &test_panel(),
        );
        assert_eq!(
            point, None,
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
        let (point, pressed) = vr_frontend_pointer(
            &vr_input(untracked, hand_aimed_at(CANVAS, target, 1.0)),
            CANVAS,
            &test_panel(),
        );
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
        let (point, pressed) = vr_frontend_pointer(&input, CANVAS, &test_panel());
        assert!(pressed, "a held trigger must stay reported as held");
        assert_eq!(
            point, None,
            "the idle hand must not supply a point while the other is pressed"
        );
    }

    #[test]
    fn aiming_away_yields_no_point_but_keeps_the_trigger() {
        let (point, pressed) = vr_frontend_pointer(
            &vr_input(hand_aimed_away(1.0), hand_aimed_away(1.0)),
            CANVAS,
            &test_panel(),
        );
        assert_eq!(point, None);
        assert!(pressed);
    }
}
