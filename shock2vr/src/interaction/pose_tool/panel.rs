//! The grip pose workbench's control panel: its status line, its buttons and
//! a legend naming the free controller's buttons. Placed once beside the bench
//! on the free hand's side, it is pressed by either controller's ray (the
//! shared frontend pointer) or by the free hand's tracked index fingertip.
use cgmath::{InnerSpace, Quaternion, Vector2, Vector3, vec2};

use super::{ContactView, world};
use crate::ui::{HAlign, PanelPlacement, Rect, UiCanvas, VAlign, WorldPanel};
use crate::vr_config::Handedness;

pub(in crate::interaction) const CANVAS: Vector2<f32> = Vector2 { x: 400.0, y: 300.0 };
/// Physical panel width; its height keeps the canvas aspect.
const PANEL_WIDTH: f32 = 0.3;
/// Where the panel hangs from the eye: nearer and lower than the bench, and
/// to the free hand's side, clear of the item's side views and within reach.
const PANEL_AHEAD: f32 = 0.33;
const PANEL_DROP: f32 = 0.47;
const PANEL_ASIDE: f32 = 0.24;
/// A fingertip this far through the panel presses it; pulled back to the
/// release depth it lets go. It must come from in front to press again.
const POKE_DEPTH: f32 = 0.01;
const POKE_RELEASE: f32 = 0.005;
/// Fingertips farther than this from the panel's plane are not near it.
const POKE_REACH: f32 = 0.05;
/// OpenXR's index fingertip joint.
const INDEX_TIP: usize = crate::hand_capture::TIPS[1];

const FONT: &str = "mainfont.fon";
const BACKGROUND: [u8; 3] = [16, 20, 26];
const BUTTON: [u8; 3] = [44, 56, 72];
const BUTTON_HOVER: [u8; 3] = [74, 104, 140];
const BUTTON_DISABLED: [u8; 3] = [28, 32, 38];

const TITLE: Rect = Rect::new(10.0, 4.0, 380.0, 26.0);
const STATUS: Rect = Rect::new(10.0, 32.0, 380.0, 34.0);
const SAVE: Rect = Rect::new(10.0, 74.0, 186.0, 44.0);
const DISCARD: Rect = Rect::new(204.0, 74.0, 186.0, 44.0);
const POSE_HAND: Rect = Rect::new(10.0, 124.0, 186.0, 44.0);
const CONTACTS: Rect = Rect::new(204.0, 124.0, 186.0, 44.0);
const EXIT: Rect = Rect::new(10.0, 174.0, 380.0, 34.0);
const LEGEND: Rect = Rect::new(10.0, 216.0, 380.0, 80.0);

/// What a panel button does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::interaction) enum PanelEvent {
    Save,
    Discard,
    SwitchHand,
    Contacts,
    Exit,
}

/// The status line, from the tool's state. A failure says why until the next
/// capture, save or discard; otherwise a staged pose is unsaved.
#[derive(Clone, Debug, PartialEq)]
pub(in crate::interaction) enum Status {
    SaveFailed(String),
    CaptureFailed(String),
    Unsaved,
    Saved,
    Ready,
}

impl Status {
    pub fn derive(
        staged: bool,
        saved: bool,
        last_save_error: Option<&str>,
        capture_failure: Option<&str>,
    ) -> Self {
        if let Some(error) = last_save_error {
            Status::SaveFailed(error.to_owned())
        } else if let Some(reason) = capture_failure {
            Status::CaptureFailed(reason.to_owned())
        } else if staged {
            Status::Unsaved
        } else if saved {
            Status::Saved
        } else {
            Status::Ready
        }
    }

    pub fn text(&self) -> String {
        match self {
            Status::SaveFailed(error) => format!("Save failed: {error}"),
            // The reasons `GripPoseTool::observe` and `capture_grip` give.
            Status::CaptureFailed(reason) => match reason.as_str() {
                "no_joints" => "No hand tracked".to_owned(),
                "window_short" => "Hold still...".to_owned(),
                "degenerate_joints" => "Tracking glitch, try again".to_owned(),
                jitter => jitter.strip_prefix("jitter max_mm=").map_or_else(
                    || jitter.to_owned(),
                    |mm| format!("Hold still (moved {mm} mm)"),
                ),
            },
            Status::Unsaved => "Unsaved pose".to_owned(),
            // The bitmap fonts carry no check mark.
            Status::Saved => "Saved".to_owned(),
            Status::Ready => "Pose, then snapshot".to_owned(),
        }
    }

    fn color(&self) -> [u8; 3] {
        match self {
            Status::SaveFailed(_) | Status::CaptureFailed(_) => [150, 40, 34],
            Status::Unsaved => [150, 108, 24],
            Status::Saved => [34, 120, 56],
            Status::Ready => [48, 58, 76],
        }
    }
}

/// What the panel shows this frame.
pub(in crate::interaction) struct PanelView {
    /// The hand being posed; the other holds the controller the legend names.
    pub posing: Handedness,
    pub status: Status,
    pub contact_view: ContactView,
    /// Whether the other hand can be posed instead, or why not.
    pub switch: Result<(), String>,
    /// A snapshot awaits Save or Discard.
    pub staged: bool,
}

fn hand_name(hand: Handedness) -> &'static str {
    match hand {
        Handedness::Left => "Left",
        Handedness::Right => "Right",
    }
}

fn other(hand: Handedness) -> Handedness {
    match hand {
        Handedness::Left => Handedness::Right,
        Handedness::Right => Handedness::Left,
    }
}

/// The free controller's buttons, as printed on it: X/Y on the left, A/B on
/// the right.
pub(in crate::interaction) fn legend(free: Handedness) -> [String; 4] {
    let (lower, upper) = match free {
        Handedness::Left => ("X", "Y"),
        Handedness::Right => ("A", "B"),
    };
    [
        format!("{} controller   Trigger: snapshot", hand_name(free)),
        format!("{lower}: save    {upper}: discard"),
        "Stick up/down: item scale".to_owned(),
        "Stick click: contacts    Finger: press".to_owned(),
    ]
}

/// The panel's canvas, and each enabled button's rect and event (the same
/// rects it draws, so a press lands where the art is). `hover` is the pointer
/// in canvas pixels.
pub(in crate::interaction) fn emit(
    view: &PanelView,
    hover: Option<Vector2<f32>>,
) -> (UiCanvas, Vec<(Rect, PanelEvent)>) {
    let mut canvas = UiCanvas::new(CANVAS);
    let mut targets = Vec::new();
    canvas.fill(Rect::new(0.0, 0.0, CANVAS.x, CANVAS.y), BACKGROUND);
    canvas.text_fit(
        TITLE,
        &format!("Grip pose: {} hand", hand_name(view.posing)),
        FONT,
        16.0,
        HAlign::Left,
        VAlign::Middle,
    );
    canvas.fill(STATUS, view.status.color());
    canvas.text_fit(
        STATUS,
        &view.status.text(),
        FONT,
        20.0,
        HAlign::Center,
        VAlign::Middle,
    );
    let contacts = match view.contact_view {
        ContactView::Contacts => "on",
        ContactView::Full => "full",
        ContactView::Off => "off",
    };
    // Save and Discard act on a staged snapshot only.
    let staged = if view.staged {
        Ok(())
    } else {
        Err("nothing staged".to_owned())
    };
    let buttons = [
        (SAVE, "Save".to_owned(), PanelEvent::Save, staged.clone()),
        (DISCARD, "Discard".to_owned(), PanelEvent::Discard, staged),
        (
            POSE_HAND,
            format!("Pose: {}", hand_name(other(view.posing))),
            PanelEvent::SwitchHand,
            view.switch.clone(),
        ),
        (
            CONTACTS,
            format!("Contacts: {contacts}"),
            PanelEvent::Contacts,
            Ok(()),
        ),
        (EXIT, "Exit".to_owned(), PanelEvent::Exit, Ok(())),
    ];
    for (rect, label, event, enabled) in buttons {
        let color = match &enabled {
            Err(_) => BUTTON_DISABLED,
            Ok(()) if hover.is_some_and(|p| rect.contains(p)) => BUTTON_HOVER,
            Ok(()) => BUTTON,
        };
        canvas.fill(rect, color);
        match enabled {
            Ok(()) => {
                canvas.text_fit(rect, &label, FONT, 18.0, HAlign::Center, VAlign::Middle);
                targets.push((rect, event));
            }
            // Dimmed, with why under the label.
            Err(reason) => {
                let (top, bottom) = (
                    Rect::new(rect.x, rect.y + 2.0, rect.w, 24.0),
                    Rect::new(rect.x, rect.y + 25.0, rect.w, 16.0),
                );
                canvas
                    .text_fit(top, &label, FONT, 18.0, HAlign::Center, VAlign::Middle)
                    .opacity(0.45);
                canvas
                    .text_fit(bottom, &reason, FONT, 12.0, HAlign::Center, VAlign::Middle)
                    .opacity(0.7);
            }
        }
    }
    let lines = legend(other(view.posing));
    let line_height = LEGEND.h / lines.len() as f32;
    for (i, line) in lines.iter().enumerate() {
        canvas.text_fit(
            Rect::new(
                LEGEND.x,
                LEGEND.y + i as f32 * line_height,
                LEGEND.w,
                line_height,
            ),
            line,
            FONT,
            14.0,
            HAlign::Left,
            VAlign::Middle,
        );
    }
    (canvas, targets)
}

/// The enabled button at a canvas point.
pub(in crate::interaction) fn event_at(
    targets: &[(Rect, PanelEvent)],
    point: Vector2<f32>,
) -> Option<PanelEvent> {
    targets
        .iter()
        .find(|(rect, _)| rect.contains(point))
        .map(|&(_, event)| event)
}

/// Where the panel hangs for this head pose: below and nearer than the bench,
/// on the side away from the posing hand, facing the head with no roll.
pub(in crate::interaction) fn place(
    head_position: Vector3<f32>,
    head_rotation: Quaternion<f32>,
    posing: Handedness,
) -> WorldPanel {
    let forward = PanelPlacement::from_head(head_position, head_rotation).forward;
    let right = forward.cross(Vector3::unit_y());
    let aside = world(match posing {
        Handedness::Left => PANEL_ASIDE,
        Handedness::Right => -PANEL_ASIDE,
    });
    let center = head_position + forward * world(PANEL_AHEAD) + right * aside
        - Vector3::unit_y() * world(PANEL_DROP);
    // Tilted back to face the eye, like a lectern: well below eye level an
    // upright panel is read ~55 degrees off its face. Fixed at placement and
    // never rolled (its x axis stays level).
    let toward = head_position - center;
    let toward = if toward.magnitude2() > 1e-8 {
        toward.normalize()
    } else {
        -forward
    };
    WorldPanel {
        center,
        rotation: crate::util::get_rotation_from_forward_vector(toward),
        size: vec2(world(PANEL_WIDTH), world(PANEL_WIDTH * CANVAS.y / CANVAS.x)),
    }
}

/// A world point on the panel: where it falls in canvas pixels (`None` off
/// the panel's face) and how far in front of it (metres, negative behind).
fn panel_point(panel: &WorldPanel, point: Vector3<f32>) -> (Option<Vector2<f32>>, f32) {
    let (point, depth) = crate::ui::point_to_canvas(CANVAS, panel, point);
    (point, depth * crate::world_scale::meters_per_world_unit())
}

/// The free hand's index fingertip (world space) if it hovers just in front
/// of the panel's face, in canvas pixels.
pub(in crate::interaction) fn fingertip_hover(
    panel: &WorldPanel,
    joints: Option<&crate::hand_capture::HandJoints>,
) -> Option<Vector2<f32>> {
    let (point, depth) = panel_point(panel, joints?[INDEX_TIP]);
    point.filter(|_| (-POKE_DEPTH..=POKE_REACH).contains(&depth))
}

/// A fingertip pressing the panel: it presses on crossing `POKE_DEPTH` into
/// the face from in front, and lets go once pulled back past `POKE_RELEASE`.
/// It starts disarmed, so a fingertip already through the panel as it
/// appears, or one sliding in from the side, presses nothing.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(in crate::interaction) struct Poke {
    armed: bool,
    pressed: bool,
}

impl Poke {
    /// The canvas point pressed this frame, given the free hand's joints.
    pub fn update(
        &mut self,
        panel: &WorldPanel,
        joints: Option<&crate::hand_capture::HandJoints>,
    ) -> Option<Vector2<f32>> {
        let near = joints
            .map(|joints| panel_point(panel, joints[INDEX_TIP]))
            .and_then(|(point, depth)| Some((point?, depth)))
            .filter(|(_, depth)| depth.abs() <= POKE_REACH);
        let Some((point, depth)) = near else {
            *self = Self::default();
            return None;
        };
        if self.pressed {
            self.pressed = depth < -POKE_RELEASE;
            return None;
        }
        if depth > 0.0 {
            self.armed = true;
        } else if self.armed && depth <= -POKE_DEPTH {
            self.armed = false;
            self.pressed = true;
            return Some(point);
        }
        None
    }
}

/// Who a trigger press belongs to, decided at its leading edge.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Owner {
    Panel,
    World,
}

/// Splits trigger presses between the panel and the snapshot. A press that
/// starts with a ray on the panel clicks it and is the panel's until let go,
/// wherever it drifts; any other press is the snapshot's. Starts "already
/// pressed", so a trigger held as the workbench opens is neither.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(in crate::interaction) struct TriggerArbiter {
    pressed: bool,
    owner: Option<Owner>,
}

impl TriggerArbiter {
    pub fn new() -> Self {
        Self {
            pressed: true,
            owner: None,
        }
    }

    /// This frame's click on the panel (canvas pixels), and whether the
    /// trigger press in flight belongs to the snapshot. `point` is where the
    /// pointer pass lands on the panel.
    pub fn update(
        &mut self,
        pressed: bool,
        point: Option<Vector2<f32>>,
    ) -> (Option<Vector2<f32>>, bool) {
        let mut click = None;
        if pressed && !self.pressed {
            self.owner = Some(if point.is_some() {
                Owner::Panel
            } else {
                Owner::World
            });
            click = point;
        } else if !pressed {
            self.owner = None;
        }
        self.pressed = pressed;
        (click, self.owner == Some(Owner::World))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cgmath::{Deg, Rotation, Rotation3, vec3};

    fn view(status: Status) -> PanelView {
        PanelView {
            posing: Handedness::Right,
            status,
            contact_view: ContactView::Contacts,
            switch: Ok(()),
            staged: false,
        }
    }

    fn texts(canvas: &UiCanvas) -> Vec<String> {
        canvas
            .elements()
            .iter()
            .filter_map(|e| match e {
                crate::ui::UiElement::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn the_status_line_reports_failures_first_then_the_staged_pose() {
        let line = |staged, saved, save: Option<&str>, capture: Option<&str>| {
            Status::derive(staged, saved, save, capture).text()
        };
        assert_eq!(line(false, false, None, None), "Pose, then snapshot");
        assert_eq!(line(true, false, None, None), "Unsaved pose");
        assert_eq!(line(false, true, None, None), "Saved");
        assert_eq!(
            line(true, false, Some("disk full"), None),
            "Save failed: disk full"
        );
        assert_eq!(
            line(true, false, None, Some("no_joints")),
            "No hand tracked"
        );
        assert_eq!(
            line(false, true, None, Some("window_short")),
            "Hold still..."
        );
        assert_eq!(
            line(true, false, None, Some("jitter max_mm=12.5")),
            "Hold still (moved 12.5 mm)"
        );
        assert_eq!(
            line(true, false, Some("disk full"), Some("no_joints")),
            "Save failed: disk full",
            "the save error outranks a capture failure"
        );
    }

    #[test]
    fn the_legend_names_the_free_controllers_own_buttons() {
        let left = legend(Handedness::Left).join(" ");
        assert!(
            left.contains("X: save") && left.contains("Y: discard"),
            "{left}"
        );
        let right = legend(Handedness::Right).join(" ");
        assert!(
            right.contains("A: save") && right.contains("B: discard"),
            "{right}"
        );
        // Posing the right hand frees the left controller.
        let (canvas, _) = emit(&view(Status::Ready), None);
        assert!(texts(&canvas).iter().any(|t| t.contains("X: save")));
    }

    #[test]
    fn a_disabled_button_says_why_and_cannot_be_pressed() {
        let mut staged = view(Status::Unsaved);
        staged.staged = true;
        staged.switch = Err("save or discard first".to_owned());
        let (canvas, targets) = emit(&staged, None);
        assert!(texts(&canvas).contains(&"save or discard first".to_owned()));
        assert_eq!(event_at(&targets, POSE_HAND.center()), None);
        assert_eq!(event_at(&targets, SAVE.center()), Some(PanelEvent::Save));
        assert_eq!(
            event_at(&targets, DISCARD.center()),
            Some(PanelEvent::Discard)
        );
        assert_eq!(event_at(&targets, EXIT.center()), Some(PanelEvent::Exit));
        // With nothing staged, Save and Discard have nothing to act on.
        let (canvas, targets) = emit(&view(Status::Saved), None);
        assert!(texts(&canvas).contains(&"nothing staged".to_owned()));
        assert_eq!(event_at(&targets, SAVE.center()), None);
        assert_eq!(event_at(&targets, DISCARD.center()), None);
    }

    #[test]
    fn the_panel_hangs_on_the_free_side_within_reach_facing_the_head() {
        let head = vec3(1.0, 5.0, 2.0);
        let turned = Quaternion::from_angle_y(Deg(30.0));
        for (posing, side) in [(Handedness::Right, -1.0), (Handedness::Left, 1.0)] {
            let panel = place(head, turned, posing);
            let forward = turned.rotate_vector(vec3(0.0, 0.0, -1.0));
            let right = forward.cross(Vector3::unit_y());
            let offset = panel.center - head;
            assert!(
                offset.dot(right) * side > 0.0,
                "{posing:?}: on the free side"
            );
            assert!(offset.y < 0.0, "below the eye");
            let reach = offset.magnitude() * crate::world_scale::meters_per_world_unit();
            assert!(reach < 0.7, "within reach: {reach} m");
            // Faces the head, its rows level.
            assert!(panel.normal().dot(-offset.normalize()) > 0.999);
            assert!(panel.rotation.rotate_vector(Vector3::unit_x()).y.abs() < 1e-5);
        }
    }

    /// A panel facing +Z at the origin and the index tip `depth` metres in
    /// front of its centre.
    fn tip(depth: f32) -> crate::hand_capture::HandJoints {
        let mut joints = [vec3(0.0, 0.0, 0.0); crate::hand_capture::HAND_JOINT_COUNT];
        joints[INDEX_TIP] = vec3(0.0, 0.0, world(depth));
        joints
    }

    fn facing_panel() -> WorldPanel {
        WorldPanel {
            center: vec3(0.0, 0.0, 0.0),
            rotation: Quaternion::new(1.0, 0.0, 0.0, 0.0),
            size: vec2(world(0.3), world(0.225)),
        }
    }

    #[test]
    fn a_fingertip_presses_once_through_the_face_and_releases_on_pulling_back() {
        let panel = facing_panel();
        let mut poke = Poke::default();
        let mut push = |depth: f32| poke.update(&panel, Some(&tip(depth))).is_some();
        assert!(!push(0.02), "in front");
        assert!(!push(-0.005), "touching is not yet a press");
        assert!(push(-0.012), "a centimetre through presses");
        assert!(!push(-0.02), "held through, once");
        assert!(!push(-0.007), "not yet pulled back past the release");
        assert!(!push(-0.012), "still the same press");
        assert!(!push(-0.003), "released...");
        assert!(!push(-0.012), "...but it must come from in front again");
        assert!(!push(0.01));
        assert!(push(-0.012), "a second poke");
    }

    #[test]
    fn a_fingertip_already_through_the_panel_or_sliding_in_presses_nothing() {
        let panel = facing_panel();
        let mut poke = Poke::default();
        assert_eq!(poke.update(&panel, Some(&tip(-0.02))), None);
        assert_eq!(poke.update(&panel, Some(&tip(-0.03))), None);
        // Armed in front, then lost (untracked) and back through: no press.
        let mut poke = Poke::default();
        poke.update(&panel, Some(&tip(0.01)));
        poke.update(&panel, None);
        assert_eq!(poke.update(&panel, Some(&tip(-0.02))), None);
        // Armed in front, slid off the face, back on behind it: no press.
        let mut poke = Poke::default();
        poke.update(&panel, Some(&tip(0.01)));
        let mut aside = tip(-0.02);
        aside[INDEX_TIP].x = world(0.5);
        poke.update(&panel, Some(&aside));
        assert_eq!(poke.update(&panel, Some(&tip(-0.02))), None);
    }

    #[test]
    fn a_press_on_the_panel_clicks_it_and_never_snapshots() {
        let on = Some(vec2(100.0, 100.0));
        let mut trigger = TriggerArbiter::new();
        assert_eq!(trigger.update(false, on), (None, false));
        assert_eq!(trigger.update(true, on), (on, false), "clicks at the edge");
        // Held and drifting off the panel: still the panel's.
        assert_eq!(trigger.update(true, None), (None, false));
        assert_eq!(trigger.update(false, None), (None, false));
    }

    #[test]
    fn a_press_off_the_panel_snapshots_even_if_aim_drifts_onto_it() {
        let on = Some(vec2(100.0, 100.0));
        let mut trigger = TriggerArbiter::new();
        trigger.update(false, None);
        assert_eq!(trigger.update(true, None), (None, true));
        assert_eq!(trigger.update(true, on), (None, true), "no click mid-press");
        assert_eq!(trigger.update(false, on), (None, false));
    }

    #[test]
    fn a_trigger_held_as_the_panel_opens_is_neither_click_nor_snapshot() {
        let on = Some(vec2(100.0, 100.0));
        let mut trigger = TriggerArbiter::new();
        assert_eq!(trigger.update(true, on), (None, false));
        assert_eq!(trigger.update(true, None), (None, false));
    }
}
