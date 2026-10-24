//! The grip pose workbench's control panel: its status line, its buttons and
//! a legend naming the free controller's buttons. Placed once beside the bench
//! on the free hand's side, it is pressed by either controller's ray (the
//! shared frontend pointer) or by the free hand's tracked index fingertip.
use cgmath::{InnerSpace, Quaternion, Rotation, Vector2, Vector3, vec2};

use super::{Slot, world};
use crate::ui::{HAlign, PanelPlacement, Rect, UiCanvas, VAlign, WorldPanel};
use crate::vr_config::Handedness;

pub(in crate::interaction) const CANVAS: Vector2<f32> = Vector2 { x: 400.0, y: 450.0 };
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
const TEST_HOLD: Rect = Rect::new(10.0, 124.0, 186.0, 44.0);
const ALIGN_LESS: Rect = Rect::new(204.0, 124.0, 40.0, 44.0);
const ALIGN_AIM: Rect = Rect::new(248.0, 124.0, 98.0, 44.0);
const ALIGN_MORE: Rect = Rect::new(350.0, 124.0, 40.0, 44.0);
const POSE_HAND: Rect = Rect::new(10.0, 174.0, 186.0, 44.0);
const SUPPORT_MODE: Rect = Rect::new(204.0, 174.0, 186.0, 44.0);
const SLOT: Rect = Rect::new(10.0, 224.0, 186.0, 44.0);
// Beside the slot, what it offers: the placement toggle at rest, the
// trigger index steps around their readout when pulled.
const PLACEMENT: Rect = Rect::new(204.0, 224.0, 186.0, 44.0);
const INDEX_LESS: Rect = Rect::new(204.0, 224.0, 40.0, 44.0);
const INDEX: Rect = Rect::new(248.0, 224.0, 98.0, 44.0);
const INDEX_MORE: Rect = Rect::new(350.0, 224.0, 40.0, 44.0);
/// Closes the slot's fingers onto the item.
const TIGHTEN: Rect = Rect::new(10.0, 274.0, 380.0, 44.0);
const SHOW_AIM: Rect = Rect::new(10.0, 324.0, 122.0, 32.0);
pub(in crate::interaction) const RESET_VIEW: Rect = Rect::new(139.0, 324.0, 122.0, 32.0);
const EXIT: Rect = Rect::new(268.0, 324.0, 122.0, 32.0);
const LEGEND: Rect = Rect::new(10.0, 364.0, 380.0, 80.0);
/// A test-held gun aiming within this many degrees of its controller reads
/// as aligned.
const ALIGNED_DEG: f32 = 1.0;

/// What a panel button does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::interaction) enum PanelEvent {
    Save,
    Discard,
    TestHold,
    /// Align fully; the steps move it a tenth less or more.
    AlignAim,
    AlignLess,
    AlignMore,
    ShowAim,
    SwitchHand,
    SupportMode,
    Slot,
    /// Whether a rest snapshot keeps the placement (Rest slot).
    Placement,
    /// The trigger index a tenth less or more curled (Trigger slot).
    IndexLess,
    IndexMore,
    /// Close the slot's fingers onto the item (`GripSurface::tighten`).
    Tighten,
    ResetView,
    Exit,
}

/// Test hold as the status line shows it: the posing hand's controller,
/// whether it is tracked (else the player must pick it up), a gun's aim
/// error against it, and whether the grip was saved since the last edit.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(in crate::interaction) struct Hold {
    pub controller: Handedness,
    pub tracked: bool,
    pub aim_error_deg: Option<f32>,
    pub saved: bool,
}

/// The status line, from the tool's state. A failed save says why until the
/// next save or discard; then test hold shows its controller and aim; then a
/// failed capture says why until the next one; otherwise a staged pose is
/// unsaved.
#[derive(Clone, Debug, PartialEq)]
pub(in crate::interaction) enum Status {
    SaveFailed(String),
    TestHold(Hold),
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
        test_hold: Option<Hold>,
    ) -> Self {
        if let Some(error) = last_save_error {
            Status::SaveFailed(error.to_owned())
        } else if let Some(hold) = test_hold {
            Status::TestHold(hold)
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
            Status::TestHold(hold) if !hold.tracked => {
                format!("Pick up the {} controller", hand_name(hold.controller))
            }
            // The bitmap fonts carry no degree sign.
            Status::TestHold(hold) => {
                let text = hold.aim_error_deg.map_or("Test hold".to_owned(), |deg| {
                    format!("Aim off by {deg:.1} deg")
                });
                // Save is clicked while holding: confirm it here too.
                if hold.saved {
                    format!("{text}, saved")
                } else {
                    text
                }
            }
            // The reasons `GripPoseTool::observe` and `capture_grip` give.
            Status::CaptureFailed(reason) => match reason.as_str() {
                "no_joints" => "No hand tracked".to_owned(),
                "window_short" => "Hold still...".to_owned(),
                "degenerate_joints" => "Tracking glitch, try again".to_owned(),
                "invalid_support_pose" => "Pose not usable, try again".to_owned(),
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
            Status::TestHold(Hold {
                tracked: true,
                aim_error_deg: Some(deg),
                ..
            }) if *deg <= ALIGNED_DEG => [34, 120, 56],
            Status::TestHold(Hold {
                tracked: true,
                aim_error_deg: None,
                ..
            }) => [48, 58, 76],
            Status::Unsaved | Status::TestHold(_) => [150, 108, 24],
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
    /// Whether the other hand can be posed instead, or why not.
    pub switch: Result<(), String>,
    /// Posing the support grip, and whether the item offers one, or why not.
    pub support: bool,
    pub support_mode: Result<(), String>,
    /// The pose a snapshot sets, and whether it can switch or why not.
    pub slot: Slot,
    pub slot_entry: Result<(), String>,
    /// Whether a rest snapshot keeps the placement, and whether that can
    /// switch or why not.
    pub keep_placement: bool,
    pub placement_entry: Result<(), String>,
    /// How far the trigger pose curls the index (`None`: no trigger pose).
    pub trigger_index: Option<f32>,
    /// Whether Tighten can act, or why not.
    pub tighten: Result<(), String>,
    /// A snapshot awaits Save or Discard.
    pub staged: bool,
    /// Test holding, and whether it can start or why not.
    pub test_hold: bool,
    pub test_hold_entry: Result<(), String>,
    /// Whether Align aim can act, or why not, and how far it is turned.
    pub align: Result<(), String>,
    pub align_amount: Option<f32>,
    /// Whether the aim toggle can act or why not, whether it is on, and
    /// the aim error of the lines drawn this frame.
    pub aim: Result<(), String>,
    pub show_aim: bool,
    pub aim_error_deg: Option<f32>,
    /// The free hand is tracked with no controller in it: it taps the
    /// buttons and pinches to snapshot.
    pub fingertip: bool,
}

/// The aim toggle's label: off, or on with the drawn lines' error (none
/// while a test-held controller is put down). The bitmap fonts carry no
/// degree sign.
fn aim_label(show: bool, error_deg: Option<f32>) -> String {
    match (show, error_deg) {
        (false, _) => "Aim: Off".to_owned(),
        (true, None) => "Aim: On".to_owned(),
        (true, Some(deg)) => format!("Aim: {deg:.1} deg"),
    }
}

fn hand_name(hand: Handedness) -> &'static str {
    match hand {
        Handedness::Left => "Left",
        Handedness::Right => "Right",
    }
}

/// How the free hand works the tool. With its controller: the buttons as
/// printed on it (X/Y on the left, A/B on the right). Tracked with no
/// controller in it: fingertip taps and pinches. Saving is on the panel. A
/// snapshot names the slot's pose it sets.
pub(in crate::interaction) fn legend(
    free: Handedness,
    fingertip: bool,
    support: bool,
    test_hold: bool,
    slot: Slot,
) -> Vec<String> {
    let hold = format!(
        "Hold the item with the {} controller",
        hand_name(free.other())
    );
    let snapshot = match slot {
        Slot::Rest => "snapshot",
        Slot::Trigger => "snapshot pulled index",
    };
    if fingertip {
        return vec![
            "Tap buttons with your fingertip".to_owned(),
            if test_hold {
                hold
            } else {
                format!("Pinch: {snapshot}")
            },
        ];
    }
    let (lower, upper) = match free {
        Handedness::Left => ("X", "Y"),
        Handedness::Right => ("A", "B"),
    };
    if test_hold {
        return vec![
            hold,
            format!("{lower}: end test hold    {upper}: discard"),
            "Squeeze: move the item in the hand".to_owned(),
        ];
    }
    vec![
        format!("{} controller   Trigger: {snapshot}", hand_name(free)),
        format!("{lower}: test hold    {upper}: discard"),
        // A support pose leaves the item's scale to the grip.
        if support {
            "Squeeze: turn".to_owned()
        } else {
            "Stick up/down: item scale    Squeeze: turn".to_owned()
        },
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
        &format!(
            "{}: {} hand",
            match (view.support, view.slot) {
                (true, _) => "Support grip",
                (false, Slot::Rest) => "Grip pose",
                (false, Slot::Trigger) => "Trigger pose",
            },
            hand_name(view.posing)
        ),
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
    // Save and Discard act on a staged snapshot only.
    let staged = if view.staged {
        Ok(())
    } else {
        Err("nothing staged".to_owned())
    };
    // The steps move a turn on Align's arc; a kept turn off it needs Align first.
    let steps = view.align.clone().and_then(|()| {
        view.align_amount
            .map(|_| ())
            .ok_or_else(|| "align first".to_owned())
    });
    let mut buttons = vec![
        (SAVE, "Save".to_owned(), PanelEvent::Save, staged.clone()),
        (DISCARD, "Discard".to_owned(), PanelEvent::Discard, staged),
        if view.test_hold {
            (
                TEST_HOLD,
                "End test hold".to_owned(),
                PanelEvent::TestHold,
                Ok(()),
            )
        } else {
            (
                TEST_HOLD,
                "Test hold".to_owned(),
                PanelEvent::TestHold,
                view.test_hold_entry.clone(),
            )
        },
        (
            ALIGN_LESS,
            "-".to_owned(),
            PanelEvent::AlignLess,
            steps.clone(),
        ),
        (
            ALIGN_AIM,
            view.align_amount.map_or("Align".to_owned(), |amount| {
                format!("Align {:.0}%", amount * 100.0)
            }),
            PanelEvent::AlignAim,
            view.align.clone(),
        ),
        (ALIGN_MORE, "+".to_owned(), PanelEvent::AlignMore, steps),
        (
            POSE_HAND,
            format!("Pose: {}", hand_name(view.posing.other())),
            PanelEvent::SwitchHand,
            view.switch.clone(),
        ),
        (
            SUPPORT_MODE,
            if view.support {
                "Grip: Support"
            } else {
                "Grip: Primary"
            }
            .to_owned(),
            PanelEvent::SupportMode,
            view.support_mode.clone(),
        ),
        (
            SLOT,
            match view.slot {
                Slot::Rest => "Slot: Rest",
                Slot::Trigger => "Slot: Trigger",
            }
            .to_owned(),
            PanelEvent::Slot,
            view.slot_entry.clone(),
        ),
    ];
    match view.slot {
        Slot::Rest => buttons.push((
            PLACEMENT,
            if view.keep_placement {
                "Placement: keep"
            } else {
                "Placement: capture"
            }
            .to_owned(),
            PanelEvent::Placement,
            view.placement_entry.clone(),
        )),
        Slot::Trigger => {
            buttons.push((INDEX_LESS, "-".to_owned(), PanelEvent::IndexLess, Ok(())));
            buttons.push((INDEX_MORE, "+".to_owned(), PanelEvent::IndexMore, Ok(())));
            // A readout, not a button: no trigger pose reads as the rest.
            canvas.fill(INDEX, BUTTON_DISABLED);
            canvas.text_fit(
                INDEX,
                &format!("Index +{:.0}%", view.trigger_index.unwrap_or(0.0) * 100.0),
                FONT,
                18.0,
                HAlign::Center,
                VAlign::Middle,
            );
        }
    }
    buttons.extend([
        (
            TIGHTEN,
            match view.slot {
                Slot::Rest => "Tighten fingers",
                Slot::Trigger => "Tighten index",
            }
            .to_owned(),
            PanelEvent::Tighten,
            view.tighten.clone(),
        ),
        (
            RESET_VIEW,
            "Reset view".to_owned(),
            PanelEvent::ResetView,
            Ok(()),
        ),
        (
            SHOW_AIM,
            aim_label(view.show_aim, view.aim_error_deg),
            PanelEvent::ShowAim,
            view.aim.clone(),
        ),
        (EXIT, "Exit".to_owned(), PanelEvent::Exit, Ok(())),
    ]);
    // Only a gun has an aim to show: the toggle is hidden for anything else,
    // as are Align's steps while it cannot act (too narrow to say why).
    let hidden_when_disabled = [
        PanelEvent::ShowAim,
        PanelEvent::AlignLess,
        PanelEvent::AlignMore,
    ];
    let shown = buttons
        .into_iter()
        .filter(|(_, _, event, enabled)| !hidden_when_disabled.contains(event) || enabled.is_ok());
    for (rect, label, event, enabled) in shown {
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
    let lines = legend(
        view.posing.other(),
        view.fingertip,
        view.support,
        view.test_hold,
        view.slot,
    );
    // Spaced for the controller legend, which is the longest.
    let line_height = LEGEND.h / 3.0;
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

/// Rays passing this close (metres) to the panel's edge belong to it: a
/// press aimed at a button that just misses must not snapshot.
pub(in crate::interaction) const PANEL_MARGIN: f32 = 0.05;

/// How close (metres) a ray from `origin` along `direction` passes to the
/// panel's face, zero when it hits it.
pub(in crate::interaction) fn ray_distance(
    panel: &WorldPanel,
    origin: Vector3<f32>,
    direction: Vector3<f32>,
) -> f32 {
    let to_local = panel.rotation.conjugate();
    let (origin, direction) = (
        to_local.rotate_vector(origin - panel.center),
        to_local.rotate_vector(direction.normalize()),
    );
    let half = panel.size / 2.0;
    // Squared distance from the face to the ray's point at `t`: convex in t.
    let gap = |t: f32| {
        let p = origin + direction * t;
        let (x, y) = ((p.x.abs() - half.x).max(0.0), (p.y.abs() - half.y).max(0.0));
        x * x + y * y + p.z * p.z
    };
    // Beyond this the ray only moves away from every point of the face.
    let (mut low, mut high) = (0.0, origin.magnitude() + half.magnitude() * 2.0);
    for _ in 0..60 {
        let (a, b) = (low + (high - low) / 3.0, high - (high - low) / 3.0);
        if gap(a) <= gap(b) {
            high = b;
        } else {
            low = a;
        }
    }
    gap((low + high) / 2.0).sqrt() * crate::world_scale::meters_per_world_unit()
}

/// Splits trigger presses between the panel and the snapshot. A press that
/// starts with a ray on the panel, or passing within `PANEL_MARGIN` of it,
/// is the panel's until let go, wherever it drifts (on a button it clicks
/// it); any other press is the snapshot's. Starts "already pressed", so a
/// trigger held as the workbench opens is neither.
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
    /// pointer pass lands on the panel; `near`, whether the ray is on it or
    /// within its margin.
    pub fn update(
        &mut self,
        pressed: bool,
        point: Option<Vector2<f32>>,
        near: bool,
    ) -> (Option<Vector2<f32>>, bool) {
        let mut click = None;
        if pressed && !self.pressed {
            self.owner = Some(if point.is_some() || near {
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
            switch: Ok(()),
            support: false,
            support_mode: Err("one-handed".to_owned()),
            slot: Slot::Rest,
            slot_entry: Ok(()),
            keep_placement: true,
            placement_entry: Ok(()),
            trigger_index: None,
            tighten: Ok(()),
            staged: false,
            fingertip: false,
            test_hold: false,
            test_hold_entry: Err("snapshot first".to_owned()),
            align: Err("no barrel".to_owned()),
            align_amount: None,
            aim: Err("no barrel".to_owned()),
            show_aim: true,
            aim_error_deg: None,
        }
    }

    #[test]
    fn the_aim_toggle_shows_only_for_a_gun_with_its_readout() {
        let label = |view: &PanelView| {
            let (canvas, targets) = emit(view, None);
            let shown = targets.iter().any(|(_, e)| *e == PanelEvent::ShowAim);
            let text = texts(&canvas).into_iter().find(|t| t.starts_with("Aim:"));
            (shown, text)
        };
        assert_eq!(label(&view(Status::Ready)), (false, None), "no barrel");
        let mut gun = view(Status::Ready);
        gun.aim = Ok(());
        gun.aim_error_deg = Some(3.24);
        assert_eq!(label(&gun), (true, Some("Aim: 3.2 deg".to_owned())));
        gun.aim_error_deg = None;
        assert_eq!(label(&gun), (true, Some("Aim: On".to_owned())));
        gun.show_aim = false;
        assert_eq!(label(&gun), (true, Some("Aim: Off".to_owned())));
    }

    #[test]
    fn a_tracked_free_hand_is_told_to_tap_not_shown_controller_buttons() {
        let lines = legend(Handedness::Left, true, false, false, Slot::Rest);
        assert_eq!(lines[0], "Tap buttons with your fingertip");
        let all = lines.join(" ");
        assert!(
            !all.contains("Trigger") && !all.contains("X:") && !all.contains("Stick"),
            "{all}"
        );
        let mut tracked = view(Status::Ready);
        tracked.fingertip = true;
        let (canvas, _) = emit(&tracked, None);
        assert!(texts(&canvas).contains(&"Tap buttons with your fingertip".to_owned()));
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
            Status::derive(staged, saved, save, capture, None).text()
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
        let left = legend(Handedness::Left, false, false, false, Slot::Rest).join(" ");
        assert!(
            left.contains("X: test hold") && left.contains("Y: discard"),
            "{left}"
        );
        assert!(left.contains("Squeeze: turn"), "{left}");
        let right = legend(Handedness::Right, false, false, false, Slot::Rest).join(" ");
        assert!(
            right.contains("A: test hold") && right.contains("B: discard"),
            "{right}"
        );
        // A controller in the free hand cannot poke the panel.
        assert!(
            !right.contains("Finger") && !right.contains("Tap"),
            "{right}"
        );
        // Saving is on the panel only.
        assert!(!right.contains("save"), "{right}");
        // Posing the right hand frees the left controller.
        let (canvas, _) = emit(&view(Status::Ready), None);
        assert!(texts(&canvas).iter().any(|t| t.contains("X: test hold")));
    }

    #[test]
    fn test_hold_names_the_controller_to_hold_and_how_to_go_back() {
        let lines = legend(Handedness::Left, false, false, true, Slot::Rest).join(" ");
        assert!(
            lines.contains("Hold the item with the Right controller")
                && lines.contains("X: end test hold")
                && lines.contains("Y: discard")
                && lines.contains("Squeeze: move the item in the hand"),
            "{lines}"
        );
        assert!(
            !lines.contains("snapshot") && !lines.contains("turn"),
            "{lines}"
        );
        let tapping = legend(Handedness::Left, true, false, true, Slot::Rest).join(" ");
        assert!(!tapping.contains("Pinch"), "{tapping}");
    }

    #[test]
    fn test_hold_reports_its_controller_and_the_guns_aim() {
        let hold = |tracked, aim_error_deg| Hold {
            controller: Handedness::Right,
            tracked,
            aim_error_deg,
            saved: false,
        };
        let line = |capture: Option<&str>, held| {
            Status::derive(true, false, None, capture, Some(held)).text()
        };
        assert_eq!(
            line(None, hold(false, Some(3.0))),
            "Pick up the Right controller"
        );
        assert_eq!(line(None, hold(true, Some(28.24))), "Aim off by 28.2 deg");
        let saved = Hold {
            saved: true,
            ..hold(true, Some(0.0))
        };
        assert_eq!(line(None, saved), "Aim off by 0.0 deg, saved");
        assert_eq!(line(None, hold(true, None)), "Test hold", "not a gun");
        assert_eq!(
            line(Some("no_joints"), hold(true, None)),
            "Test hold",
            "a stale capture failure gives way to test hold"
        );
        assert_eq!(
            Status::derive(true, false, Some("disk full"), None, Some(hold(true, None))).text(),
            "Save failed: disk full"
        );
        assert_eq!(
            Status::TestHold(hold(true, Some(0.4))).color(),
            Status::Saved.color(),
            "aligned reads as done"
        );
    }

    #[test]
    fn test_hold_needs_a_snapshot_and_align_needs_a_gun() {
        let (canvas, targets) = emit(&view(Status::Ready), None);
        let shown = texts(&canvas);
        assert!(shown.contains(&"snapshot first".to_owned()));
        assert!(shown.contains(&"no barrel".to_owned()));
        assert!(
            !shown.contains(&"+".to_owned()),
            "no steps without a barrel"
        );
        assert_eq!(event_at(&targets, TEST_HOLD.center()), None);
        assert_eq!(event_at(&targets, ALIGN_AIM.center()), None);
        assert_eq!(event_at(&targets, ALIGN_MORE.center()), None);
        let mut holding = view(Status::Unsaved);
        holding.test_hold = true;
        holding.test_hold_entry = Err("snapshot first".to_owned());
        holding.align = Ok(());
        holding.align_amount = Some(0.7);
        let (canvas, targets) = emit(&holding, None);
        let shown = texts(&canvas);
        assert!(shown.contains(&"End test hold".to_owned()));
        assert!(shown.contains(&"Align 70%".to_owned()), "{shown:?}");
        assert_eq!(
            event_at(&targets, TEST_HOLD.center()),
            Some(PanelEvent::TestHold),
            "ending test hold is always allowed"
        );
        for (rect, event) in [
            (ALIGN_LESS, PanelEvent::AlignLess),
            (ALIGN_AIM, PanelEvent::AlignAim),
            (ALIGN_MORE, PanelEvent::AlignMore),
        ] {
            assert_eq!(event_at(&targets, rect.center()), Some(event));
        }
        // A kept turn off Align's arc: Align, but no steps from it.
        holding.align_amount = None;
        let (canvas, targets) = emit(&holding, None);
        assert!(texts(&canvas).contains(&"Align".to_owned()));
        assert_eq!(
            event_at(&targets, ALIGN_AIM.center()),
            Some(PanelEvent::AlignAim)
        );
        assert_eq!(event_at(&targets, ALIGN_LESS.center()), None);
    }

    #[test]
    fn the_support_button_is_offered_only_for_two_handed_items() {
        let (canvas, targets) = emit(&view(Status::Ready), None);
        assert!(texts(&canvas).contains(&"one-handed".to_owned()));
        assert_eq!(event_at(&targets, SUPPORT_MODE.center()), None);
        let mut support = view(Status::Ready);
        support.support = true;
        support.posing = Handedness::Left;
        support.support_mode = Ok(());
        let (canvas, targets) = emit(&support, None);
        let texts = texts(&canvas);
        assert!(texts.contains(&"Support grip: Left hand".to_owned()));
        assert!(texts.contains(&"Grip: Support".to_owned()));
        assert!(!texts.iter().any(|t| t.contains("item scale")), "{texts:?}");
        assert_eq!(
            event_at(&targets, SUPPORT_MODE.center()),
            Some(PanelEvent::SupportMode)
        );
    }

    #[test]
    fn the_slot_button_names_the_pose_a_snapshot_sets() {
        let (canvas, targets) = emit(&view(Status::Ready), None);
        let shown = texts(&canvas);
        assert!(shown.contains(&"Slot: Rest".to_owned()));
        assert!(shown.contains(&"Grip pose: Right hand".to_owned()));
        assert!(shown.iter().any(|t| t.ends_with("Trigger: snapshot")));
        assert_eq!(event_at(&targets, SLOT.center()), Some(PanelEvent::Slot));
        let mut trigger = view(Status::Ready);
        trigger.slot = Slot::Trigger;
        let (canvas, _) = emit(&trigger, None);
        let shown = texts(&canvas);
        assert!(shown.contains(&"Slot: Trigger".to_owned()), "{shown:?}");
        assert!(shown.contains(&"Trigger pose: Right hand".to_owned()));
        assert!(
            shown
                .iter()
                .any(|t| t.ends_with("Trigger: snapshot pulled index")),
            "{shown:?}"
        );
        trigger.fingertip = true;
        let (canvas, _) = emit(&trigger, None);
        assert!(texts(&canvas).contains(&"Pinch: snapshot pulled index".to_owned()));
        // In support mode it is disabled, saying why.
        let mut support = view(Status::Ready);
        support.slot_entry = Err("primary grip only".to_owned());
        let (canvas, targets) = emit(&support, None);
        assert!(texts(&canvas).contains(&"primary grip only".to_owned()));
        assert_eq!(event_at(&targets, SLOT.center()), None);
    }

    /// At rest the slot is joined by the placement toggle; pulled, by the
    /// trigger index steps around their readout.
    #[test]
    fn the_slot_offers_placement_at_rest_and_index_steps_when_pulled() {
        let steps = |targets: &[(Rect, PanelEvent)]| {
            targets
                .iter()
                .any(|(_, e)| matches!(e, PanelEvent::IndexLess | PanelEvent::IndexMore))
        };
        let (canvas, targets) = emit(&view(Status::Ready), None);
        assert!(
            texts(&canvas).contains(&"Placement: keep".to_owned()),
            "{:?}",
            texts(&canvas)
        );
        assert_eq!(
            event_at(&targets, PLACEMENT.center()),
            Some(PanelEvent::Placement)
        );
        assert!(!steps(&targets));
        let mut capture = view(Status::Ready);
        capture.keep_placement = false;
        assert!(texts(&emit(&capture, None).0).contains(&"Placement: capture".to_owned()));
        let mut support = view(Status::Ready);
        support.placement_entry = Err("primary grip only".to_owned());
        assert_eq!(event_at(&emit(&support, None).1, PLACEMENT.center()), None);

        let mut trigger = view(Status::Ready);
        trigger.slot = Slot::Trigger;
        trigger.trigger_index = Some(0.4);
        let (canvas, targets) = emit(&trigger, None);
        let shown = texts(&canvas);
        assert!(shown.contains(&"Index +40%".to_owned()), "{shown:?}");
        assert!(
            !shown.iter().any(|t| t.starts_with("Placement")),
            "{shown:?}"
        );
        assert_eq!(
            event_at(&targets, INDEX_LESS.center()),
            Some(PanelEvent::IndexLess)
        );
        assert_eq!(
            event_at(&targets, INDEX_MORE.center()),
            Some(PanelEvent::IndexMore)
        );
        assert_eq!(event_at(&targets, INDEX.center()), None, "a readout");
        trigger.trigger_index = None;
        assert!(texts(&emit(&trigger, None).0).contains(&"Index +0%".to_owned()));
        // Reset view and Exit share the bottom row with the aim toggle.
        assert_eq!(
            event_at(&targets, RESET_VIEW.center()),
            Some(PanelEvent::ResetView)
        );
        assert_eq!(event_at(&targets, EXIT.center()), Some(PanelEvent::Exit));
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
    fn tighten_names_what_the_slot_closes_and_says_why_it_cannot() {
        let rest = view(Status::Ready);
        let (canvas, targets) = emit(&rest, None);
        assert!(texts(&canvas).contains(&"Tighten fingers".to_owned()));
        assert_eq!(
            event_at(&targets, TIGHTEN.center()),
            Some(PanelEvent::Tighten)
        );
        let mut trigger = view(Status::Ready);
        trigger.slot = Slot::Trigger;
        assert!(texts(&emit(&trigger, None).0).contains(&"Tighten index".to_owned()));
        let mut support = view(Status::Ready);
        support.tighten = Err("primary grip only".to_owned());
        let (canvas, targets) = emit(&support, None);
        assert!(texts(&canvas).contains(&"primary grip only".to_owned()));
        assert_eq!(event_at(&targets, TIGHTEN.center()), None);
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
        assert_eq!(trigger.update(false, on, true), (None, false));
        assert_eq!(
            trigger.update(true, on, true),
            (on, false),
            "clicks at the edge"
        );
        // Held and drifting off the panel: still the panel's.
        assert_eq!(trigger.update(true, None, false), (None, false));
        assert_eq!(trigger.update(false, None, false), (None, false));
    }

    #[test]
    fn a_press_off_the_panel_snapshots_even_if_aim_drifts_onto_it() {
        let on = Some(vec2(100.0, 100.0));
        let mut trigger = TriggerArbiter::new();
        trigger.update(false, None, false);
        assert_eq!(trigger.update(true, None, false), (None, true));
        assert_eq!(
            trigger.update(true, on, true),
            (None, true),
            "no click mid-press"
        );
        assert_eq!(trigger.update(false, on, true), (None, false));
    }

    #[test]
    fn a_press_just_off_the_panel_edge_is_the_panels_and_clicks_nothing() {
        let mut trigger = TriggerArbiter::new();
        trigger.update(false, None, true);
        assert_eq!(trigger.update(true, None, true), (None, false));
        assert_eq!(trigger.update(false, None, true), (None, false));
    }

    #[test]
    fn a_trigger_held_as_the_panel_opens_is_neither_click_nor_snapshot() {
        let on = Some(vec2(100.0, 100.0));
        let mut trigger = TriggerArbiter::new();
        assert_eq!(trigger.update(true, on, true), (None, false));
        assert_eq!(trigger.update(true, None, false), (None, false));
    }

    /// A ray from in front of `facing_panel` toward `target` (panel-local
    /// metres, the panel 0.3 x 0.225 m about its centre).
    fn distance_to(origin: Vector3<f32>, target: Vector3<f32>) -> f32 {
        let (origin, target) = (origin.map(world), target.map(world));
        ray_distance(&facing_panel(), origin, target - origin)
    }

    #[test]
    fn a_ray_on_the_face_is_at_zero_and_off_the_edge_at_its_gap() {
        let front = vec3(0.1, 0.05, 0.3);
        assert!(distance_to(front, vec3(0.0, 0.0, 0.0)) < 1e-4);
        // Aimed 3 cm right of the right edge (x = 0.15): within the margin.
        let gap = distance_to(front, vec3(0.18, 0.0, 0.0));
        assert!((gap - 0.03).abs() < 2e-3, "{gap}");
        assert!(gap <= PANEL_MARGIN);
        // 10 cm off: the snapshot's.
        assert!(distance_to(front, vec3(0.25, 0.0, 0.0)) > PANEL_MARGIN);
    }

    #[test]
    fn a_ray_missing_the_plane_still_counts_how_close_it_passes() {
        // Parallel to the face, 2 cm in front of it: hits no plane.
        let skim = distance_to(vec3(-0.5, 0.0, 0.02), vec3(0.5, 0.0, 0.02));
        assert!((skim - 0.02).abs() < 2e-3, "{skim}");
        // Aimed away from a panel 4 cm behind the controller: its origin is
        // the closest the ray gets.
        let away = distance_to(vec3(0.0, 0.0, 0.04), vec3(0.0, 0.0, 1.0));
        assert!((away - 0.04).abs() < 2e-3, "{away}");
        // Aimed away from far off: nowhere near.
        assert!(distance_to(vec3(0.0, 0.0, 0.3), vec3(0.0, 0.0, 1.0)) > PANEL_MARGIN);
    }
}
