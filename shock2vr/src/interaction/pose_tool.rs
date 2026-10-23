//! In-VR grip authoring (a developer cheat): a workbench that freezes the
//! mission and shows, in a plain room of its own, the held item with its saved
//! grip's glove floating in front of the player. A tracked hand forms the grip around it; the other hand's
//! controller trigger (or its tracked pinch) stages that pose live, its lower
//! button saves it as a headset override and its upper button discards it,
//! and its squeeze turns the bench to examine it.
//! A panel beside the bench (`panel`) shows the state and repeats the controls.
pub(super) mod panel;

use std::collections::VecDeque;
use std::path::{Path, PathBuf};

use std::rc::Rc;

use cgmath::{Deg, InnerSpace, Matrix4, Quaternion, Rad, Rotation, Rotation3, Vector3};
use shipyard::EntityId;

use engine::scene::{SceneObject, light::LightArray};

use crate::hand_capture::{HandFit, HandJoints};
use crate::hand_glove::{GloveRenderer, HandLight};
use crate::hand_pose::{HandShape, Pose};
use crate::vr_config::Handedness;
use crate::vr_grip::{BakedGripEntry, GripOverrides, ResolvedGrip};
use crate::vr_support::{GripPose, SupportPose};

// Distances below are physical (metres): tracked hands, the glove and the
// held item keep their real size at any world scale. `world` converts.

/// Thumb and index tips closer than this start a pinch, and farther than the
/// release distance end it, so tracking noise at the edge does not re-fire it.
const PINCH_PRESS: f32 = 0.02;
const PINCH_RELEASE: f32 = 0.03;
/// Seconds of steady posing-hand tracking averaged into a capture.
const STEADY_SECONDS: f32 = 0.3;
/// Every joint stays this close to its window mean to count as steady.
/// Quest 3 hands held still stray up to ~13 mm.
const STEADY_DISTANCE: f32 = 0.015;
/// Natural-log item scale change per second at full thumbstick, and the
/// thumbstick travel below which it does nothing.
const SCALE_RATE: f32 = 0.7;
const SCALE_DEAD_ZONE: f32 = 0.2;
/// The grip editor's authored item scale range.
const SCALE_RANGE: std::ops::RangeInclusive<f32> = 0.1..=3.0;
/// How far beside the item each extra view of it sits.
const VIEW_SPACING: f32 = 0.35;
/// Where the workbench holds the item relative to the eye: ahead along the
/// head's yaw, down to chest height, and aside toward the posing hand.
const BENCH_AHEAD: f32 = 0.45;
const BENCH_DROP: f32 = 0.3;
const BENCH_ASIDE: f32 = 0.1;
/// The workbench room's width, centred on where the player stood.
const ROOM_SIZE: f32 = 12.0;
/// The panel beam starts this far out of the controller (its glove holds
/// the controller, not a pointing finger), and runs this far when it misses.
const BEAM_CLEARANCE: f32 = 0.04;
const BEAM_MISS_LENGTH: f32 = 1.0;

/// Whether a hand's controller is held, so aims the panel ray. The runtime
/// must track it; the posing hand's is set down once that hand is tracked.
/// The free hand's joints say nothing: with simultaneous hands and
/// controllers a hand holding its controller can report joints too.
pub(super) fn controller_in_hand(posing: bool, joints: bool, tracked: bool) -> bool {
    tracked && !(posing && joints)
}

/// A physical distance in world units at the active world scale.
fn world(metres: f32) -> f32 {
    metres / crate::world_scale::meters_per_world_unit()
}

/// A world-unit distance in millimetres at the active world scale.
fn millimetres(world_units: f32) -> f32 {
    world_units * crate::world_scale::meters_per_world_unit() * 1000.0
}

pub(super) struct GripPoseTool {
    /// The grip hand (0 left, 1 right), whose grip places the item and which
    /// is posed unless in support mode (`posing`), the item and the hand that
    /// holds it in the mission (the grip hand, unless switched).
    pub hand: usize,
    pub entity: EntityId,
    pub holder: usize,
    /// World pose of the grip hand's glove: where it held the item until
    /// the workbench is placed, then on the workbench.
    pub hand_pose: GripPose,
    /// Whether the workbench has been placed in front of the player.
    pub placed: bool,
    /// World-space joints of each tracked hand this frame, drawn as gloves.
    pub tracked: [Option<HandJoints>; 2],
    /// Averaged posing-hand joints awaiting capture.
    pub capture: Option<HandJoints>,
    /// A capture shown live but not yet saved.
    pub staged: Option<super::StagedGrip>,
    pub last_save_error: Option<String>,
    pub overrides_path: PathBuf,
    /// The held item's model frame and scale, for the extra views.
    pub item: Option<(GripPose, f32)>,
    /// The item's world pose on the workbench (contact offset included).
    pub item_placed: Option<GripPose>,
    /// Where the player stood when the workbench was placed: the extra
    /// views face it.
    pub viewer: Vector3<f32>,
    /// And the way they faced, which the panel is placed from.
    gaze: Quaternion<f32>,
    /// World height of the player's real floor, which the workbench floor
    /// lies on.
    pub floor_y: f32,
    /// Each untracked hand's controller (world pose, trigger, squeeze), drawn
    /// as a glove so the hand on the controls can be seen.
    pub controllers: [Option<(GripPose, f32, f32)>; 2],
    /// How far each glove fingertip (thumb..little) lands from the posing
    /// hand's tracked one this frame, in millimetres: the mapping's error.
    pub tip_error_mm: Option<[f32; 5]>,
    /// The posing hand's size over the glove's this frame (wrist to middle
    /// tip along the bones).
    pub hand_size_ratio: Option<f32>,
    /// Why the last capture request captured nothing, until one succeeds.
    pub capture_failure: Option<String>,
    /// A save succeeded and nothing was staged since.
    pub saved: bool,
    /// The control panel, placed with the bench, and this frame's pointing
    /// at it: the controller rays and the pointer in canvas pixels (a ray's,
    /// else the free fingertip's).
    pub panel: Option<crate::ui::WorldPanel>,
    pub pointer: crate::ui::FrontendPointerPass,
    pub hover: Option<cgmath::Vector2<f32>>,
    /// Who each controller's (left, right) trigger press belongs to.
    pub triggers: [panel::TriggerArbiter; 2],
    pub poke: panel::Poke,
    /// Whether the other hand can be posed instead, or why not.
    pub switch: Result<(), String>,
    /// Support mode: the other hand is posed as the support grip, against
    /// the grip hand's glove frozen at its grip. `support_mode` is whether
    /// the item offers it, or why not.
    pub support: bool,
    pub support_mode: Result<(), String>,
    /// The free hand is tracked, so it works the panel by fingertip.
    fingertip: bool,
    /// How the bench is turned for viewing about `pivot`, and while the
    /// free controller's squeeze turns it, that controller's rotation and
    /// the turn as the squeeze began.
    pub turn: Quaternion<f32>,
    turning: Option<(Quaternion<f32>, Quaternion<f32>)>,
    squeezed: bool,
    /// The free thumbstick was seen centred in hand since last out of it.
    stick_armed: bool,
    /// Recent posing-hand frames, each with the time since the one before
    /// (zero for the oldest), spanning the steady window.
    recent: VecDeque<(HandJoints, f32)>,
    requested: bool,
    pinching: bool,
    /// The free hand was lost since last seen open.
    pinch_lost: bool,
}

impl GripPoseTool {
    pub fn new(
        hand: usize,
        entity: EntityId,
        hand_pose: GripPose,
        overrides_path: PathBuf,
    ) -> Self {
        // A controller untracked as the workbench opens reports the zero
        // quaternion, which would make every item and glove transform NaN.
        let hand_pose = GripPose {
            rotation: crate::util::tracked_rotation(hand_pose.rotation)
                .unwrap_or(Quaternion::new(1.0, 0.0, 0.0, 0.0)),
            ..hand_pose
        };
        Self {
            hand,
            entity,
            holder: hand,
            hand_pose,
            placed: false,
            tracked: [None, None],
            capture: None,
            staged: None,
            last_save_error: None,
            overrides_path,
            item: None,
            item_placed: None,
            viewer: hand_pose.position,
            gaze: Quaternion::new(1.0, 0.0, 0.0, 0.0),
            floor_y: hand_pose.position.y,
            controllers: [None, None],
            tip_error_mm: None,
            hand_size_ratio: None,
            capture_failure: None,
            saved: false,
            panel: None,
            pointer: Default::default(),
            hover: None,
            triggers: [panel::TriggerArbiter::new(); 2],
            poke: Default::default(),
            switch: Ok(()),
            support: false,
            support_mode: Err("one-handed".to_owned()),
            fingertip: false,
            turn: Quaternion::new(1.0, 0.0, 0.0, 0.0),
            turning: None,
            squeezed: true,
            stick_armed: false,
            recent: VecDeque::new(),
            // Entered "already pressed": a trigger or pinch held as the
            // workbench opens must be let go before it acts.
            requested: true,
            pinching: false,
            pinch_lost: false,
        }
    }

    /// Move the posing hand, and so its item, from `item_position` onto the
    /// workbench in front of this head pose, and hang the panel beside it.
    pub fn place(
        &mut self,
        head_position: Vector3<f32>,
        head_rotation: Quaternion<f32>,
        item_position: Vector3<f32>,
    ) {
        let spot = bench_spot(head_position, head_rotation, self.hand);
        self.hand_pose.position += spot - item_position;
        self.viewer = head_position;
        self.gaze = head_rotation;
        self.panel = Some(panel::place(
            head_position,
            head_rotation,
            [Handedness::Left, Handedness::Right][self.posing()],
        ));
        self.placed = true;
    }

    /// Pose the other hand instead, its glove at `hand_pose` (holding the
    /// item where it is): what was tracked, averaged or pressed for the old
    /// hand starts over.
    pub fn retarget(&mut self, hand: usize, hand_pose: GripPose) {
        self.hand = hand;
        self.hand_pose = hand_pose;
        self.restart_posing();
    }

    /// Pose the support hand (`support`) against the primary glove frozen
    /// at its grip, or the primary hand again: the other hand is posed.
    pub fn set_support(&mut self, support: bool) {
        self.support = support;
        self.restart_posing();
    }

    /// The hand being posed: the primary (grip) hand, or in support mode
    /// the other one.
    pub fn posing(&self) -> usize {
        if self.support {
            1 - self.hand
        } else {
            self.hand
        }
    }

    /// A different hand is posed: what was tracked, averaged or pressed for
    /// the old one starts over.
    fn restart_posing(&mut self) {
        self.tip_error_mm = None;
        self.hand_size_ratio = None;
        self.capture_failure = None;
        self.saved = false;
        // What the newly free hand holds (a squeeze round the item, a
        // trigger, a pinch) acts only once let go.
        self.pinching = false;
        self.rearm();
        // The panel moves to the newly free hand's side.
        if self.placed {
            self.panel = Some(panel::place(
                self.viewer,
                self.gaze,
                [Handedness::Left, Handedness::Right][self.posing()],
            ));
        }
    }

    /// Ignore whatever is held now, as on entry: for frames the workbench
    /// was not updated (the pause menu over it, or the app unfocused).
    pub fn rearm(&mut self) {
        self.triggers = [panel::TriggerArbiter::new(); 2];
        self.requested = true;
        self.pinch_lost = true;
        self.squeezed = true;
        self.turning = None;
        self.stick_armed = false;
        self.poke = Default::default();
        // Frames from before the pause are not part of a steady window now.
        self.recent.clear();
    }

    /// This frame's pointing at the panel, from world-space `input` whose
    /// rays are only the controllers held (an untracked or set-down one has
    /// the zero rotation): the button a controller ray clicked or the free
    /// hand's index fingertip poked, and whether the trigger press in flight
    /// is the snapshot's rather than the panel's.
    pub fn point(
        &mut self,
        input: &crate::input_context::InputContext,
    ) -> (Option<panel::PanelEvent>, bool) {
        let Some(placed) = self.panel else {
            return (None, false);
        };
        // A controller with no ray (set down, untracked) cannot claim the
        // panel, even resting on its trigger.
        let mut aimed = input.clone();
        for hand in [&mut aimed.left_hand, &mut aimed.right_hand] {
            if crate::util::tracked_rotation(hand.rotation).is_none() {
                hand.trigger_value = 0.0;
            }
        }
        self.pointer = crate::ui::vr_frontend_pointer_pass(&aimed, panel::CANVAS, &placed);
        // Each trigger's press belongs to whatever its own ray was on as it
        // went down; only the free controller's can snapshot. A controller
        // with no ray presses nothing, and is disarmed (as if held) until let
        // go with one.
        let mut click = None;
        let mut snapshot = false;
        for (index, hand) in [&input.left_hand, &input.right_hand]
            .into_iter()
            .enumerate()
        {
            let ray = self
                .pointer
                .rays
                .iter()
                .find(|ray| crate::vr_config::hand_slot(ray.handedness) == index);
            if ray.is_none() {
                self.triggers[index] = panel::TriggerArbiter::new();
                continue;
            }
            let hit = ray.and_then(|ray| ray.canvas_hit);
            // A ray just off the panel's edge is a missed button, not a
            // snapshot.
            let near = ray.is_some_and(|ray| {
                panel::ray_distance(&placed, ray.origin, ray.direction) <= panel::PANEL_MARGIN
            });
            let (clicked, world) = self.triggers[index].update(
                hand.trigger_value > crate::ui::VR_TRIGGER_THRESHOLD,
                hit,
                near,
            );
            click = click.or(clicked);
            if index != self.posing() {
                snapshot = world;
            }
        }
        // The free fingertip presses only with no controller in that hand: a
        // hand reaching with its controller would press what its ray skipped.
        let free = [&input.left_hand, &input.right_hand][1 - self.posing()];
        let free = free
            .joints
            .filter(|_| crate::util::tracked_rotation(free.rotation).is_none());
        self.fingertip = free.is_some();
        let poked = self.poke.update(&placed, free.as_ref());
        self.hover = self
            .pointer
            .point()
            .or_else(|| panel::fingertip_hover(&placed, free.as_ref()));
        let (_, targets) = panel::emit(&self.panel_view(), self.hover);
        let event = click
            .or(poked)
            .and_then(|point| panel::event_at(&targets, point));
        (event, snapshot)
    }

    /// The panel's state for `/v1/info`: its status line, and where it and
    /// each enabled button are (world space), for automation to press them.
    pub fn panel_diagnostics(&self) -> serde_json::Value {
        let view = self.panel_view();
        let placed = self.panel.map(|placed| {
            let (_, targets) = panel::emit(&view, None);
            serde_json::json!({
                "center": placed.center,
                "rotation": placed.rotation,
                "normal": placed.normal(),
                "size": placed.size,
                "buttons": targets.iter().map(|(rect, event)| serde_json::json!({
                    "event": format!("{event:?}"),
                    "center": crate::ui::canvas_to_panel_world(panel::CANVAS, &placed, rect.center()),
                })).collect::<Vec<_>>(),
            })
        });
        serde_json::json!({
            "status": view.status.text(),
            "switch_disabled": view.switch.err(),
            "support_disabled": view.support_mode.err(),
            "hover": self.hover,
            "panel": placed,
            // The bench's view turn about its pivot (world space).
            "view": { "turn": self.turn, "pivot": self.pivot() },
        })
    }

    /// What the panel shows.
    pub fn panel_view(&self) -> panel::PanelView {
        panel::PanelView {
            posing: [Handedness::Left, Handedness::Right][self.posing()],
            status: panel::Status::derive(
                self.staged.is_some(),
                self.saved,
                self.last_save_error.as_deref(),
                self.capture_failure.as_deref(),
            ),
            switch: self.switch.clone(),
            support: self.support,
            support_mode: self.support_mode.clone(),
            staged: self.staged.is_some(),
            fingertip: self.fingertip,
        }
    }

    /// While the free controller's squeeze is held (from its leading edge),
    /// the bench turns 1:1 with the controller; let go, it stays turned.
    /// `controller` is its world rotation, `None` while it aims no ray.
    pub fn turn_with(&mut self, controller: Option<Quaternion<f32>>, squeeze: bool) {
        // Out of hand it lets go, and is disarmed (as if held) until
        // released in hand: a grab never resumes against a stale rotation.
        let Some(now) = controller else {
            self.turning = None;
            self.squeezed = true;
            return;
        };
        if !squeeze {
            self.turning = None;
        } else {
            match self.turning {
                Some((grabbed, turn)) => {
                    self.turn = (now * grabbed.invert() * turn).normalize();
                }
                None if !self.squeezed => self.turning = Some((now, self.turn)),
                None => {}
            }
        }
        self.squeezed = squeeze;
    }

    /// The free controller's thumbstick y for scaling: zero out of hand, and
    /// after that (or on entry) until it is seen centred in hand.
    pub fn scale_input(&mut self, thumbstick_y: f32, in_hand: bool) -> f32 {
        if !in_hand {
            self.stick_armed = false;
        } else if thumbstick_y.abs() < SCALE_DEAD_ZONE {
            self.stick_armed = true;
        }
        if self.stick_armed { thumbstick_y } else { 0.0 }
    }

    pub fn reset_view(&mut self) {
        self.turn = Quaternion::new(1.0, 0.0, 0.0, 0.0);
        self.turning = None;
    }

    /// What the bench is turned about: the item's origin.
    pub fn pivot(&self) -> Vector3<f32> {
        self.item
            .map_or(self.hand_pose.position, |(item, _)| item.position)
    }

    /// Draws the bench (item, posed gloves, their spheres) as turned. The
    /// tool works in the unturned bench, so captures stay relative to the
    /// item.
    pub fn turn_matrix(&self) -> Matrix4<f32> {
        let pivot = self.pivot();
        Matrix4::from_translation(pivot)
            * Matrix4::from(self.turn)
            * Matrix4::from_translation(-pivot)
    }

    /// A world point (where the real hand is) in the unturned bench.
    pub fn to_bench(&self, point: Vector3<f32>) -> Vector3<f32> {
        let pivot = self.pivot();
        pivot + self.turn.invert().rotate_vector(point - pivot)
    }

    /// A bench point where it is drawn, in the world.
    pub fn from_bench(&self, point: Vector3<f32>) -> Vector3<f32> {
        let pivot = self.pivot();
        pivot + self.turn.rotate_vector(point - pivot)
    }

    /// Fixed lights for the workbench (no mission light reaches it), around
    /// the item and turned with the player who placed it: the key light comes
    /// from behind their left shoulder.
    pub fn lights(&self) -> Option<Rc<LightArray>> {
        let item = self
            .item
            .map_or(self.hand_pose.position, |(item, _)| item.position);
        let yaw = crate::scripts::ai::ai_util::normalized_horizontal(item - self.viewer)
            // The turn about +Y that points -Z (ahead) at the item.
            .map_or(Quaternion::new(1.0, 0.0, 0.0, 0.0), |ahead| {
                Quaternion::from_angle_y(Rad((-ahead.x).atan2(-ahead.z)))
            });
        crate::object_lighting::gallery_lights(item, yaw)
    }

    /// The workbench's plain room, for spatial reference: a grid floor at the
    /// player's real floor inside a solid backdrop.
    pub fn environment(&self) -> Vec<SceneObject> {
        let below = Vector3::new(self.viewer.x, self.floor_y, self.viewer.z);
        let thickness = world(0.02);
        let room_size = world(ROOM_SIZE);
        // Inset from the walls, so its edges do not fight them for depth.
        let floor_size = room_size - 2.0 * thickness;
        let floor = crate::scenes::debug_common::shaded_cube_object(
            Vector3::new(0.32, 0.33, 0.35),
            below - Vector3::unit_y() * (thickness / 2.0),
            Vector3::new(floor_size, thickness, floor_size),
        );
        let mut backdrop = crate::scenes::debug_common::cube_object(
            Vector3::new(0.1, 0.11, 0.13),
            below,
            Vector3::new(room_size, room_size, room_size),
        );
        // Seen from inside, so no face is culled.
        backdrop.set_backface_culling(None);
        let mut room = [(floor, "workbench floor"), (backdrop, "workbench backdrop")];
        for (object, name) in &mut room {
            object.set_debug_tag(Some(Rc::new(engine::scene::SceneObjectDebugTag {
                name: Some((*name).to_owned()),
                source: Some(crate::util::render_source::GRIP_WORKBENCH.to_owned()),
                ..Default::default()
            })));
        }
        room.into_iter().map(|(object, _)| object).collect()
    }

    /// Take this frame's tracked hands. The leading edge of `trigger` (the
    /// other controller's) or the other hand's pinch queues a capture of the
    /// posing hand, if it has held steady for the last `STEADY_SECONDS`.
    /// Losing its tracking restarts that window.
    pub fn observe(&mut self, tracked: [Option<HandJoints>; 2], trigger: bool, dt: f32) {
        match tracked[self.posing()] {
            Some(joints) => {
                // A first frame spans no time: one long frame is not
                // a steady window.
                let since = if self.recent.is_empty() { 0.0 } else { dt };
                self.recent.push_back((joints, since));
                // Drop the oldest frame while the rest still span the window.
                while self.recent.len() > 1
                    && spans_window(window_seconds(&self.recent) - self.recent[1].1)
                {
                    self.recent.pop_front();
                    self.recent[0].1 = 0.0;
                }
            }
            None => self.recent.clear(),
        }
        // Losing the free hand is not letting go of a pinch, nor is finding
        // it pinched one: after a loss it must be seen open to pinch again.
        match tracked[1 - self.posing()].as_ref() {
            None => {
                self.pinch_lost = true;
                self.pinching = false;
            }
            Some(joints) if self.pinch_lost => {
                if !pinching(true, Some(joints)) {
                    self.pinch_lost = false;
                    self.pinching = false;
                }
            }
            Some(joints) => self.pinching = pinching(self.pinching, Some(joints)),
        }
        let requested = trigger || self.pinching;
        if requested && !self.requested {
            match steady_mean(&self.recent) {
                Ok(mean) => {
                    self.capture = Some(mean);
                    self.capture_failure = None;
                }
                Err(reason) => {
                    println!("SHOCK2QUEST_VR_GRIP_POSE_TOOL captured=false reason={reason}");
                    self.capture_failure = Some(reason.to_string());
                }
            }
        }
        self.requested = requested;
        self.tracked = tracked;
    }

    /// Thumbstick y grows or shrinks the item (`item_scale` as drawn) while
    /// its controller is held.
    pub fn scale(item_scale: f32, thumbstick_y: f32, dt: f32) -> f32 {
        if thumbstick_y.abs() < SCALE_DEAD_ZONE {
            return item_scale;
        }
        let personal = crate::world_scale::WorldScale::active().personal();
        (item_scale * (thumbstick_y * SCALE_RATE * dt).exp())
            .clamp(SCALE_RANGE.start() * personal, SCALE_RANGE.end() * personal)
    }

    /// Transforms that redraw the item and its gloves beside it, turned to
    /// show the viewer its far side, its top and its profile.
    pub fn views(&self) -> Option<[Matrix4<f32>; 3]> {
        let (item, _) = self.item?;
        let item = item.position;
        let up = Vector3::unit_y();
        let toward = crate::scripts::ai::ai_util::normalized_horizontal(self.viewer - item)?;
        let side = up.cross(toward);
        let spacing = world(VIEW_SPACING);
        let around = |rotation: Quaternion<f32>, offset: Vector3<f32>| {
            Matrix4::from_translation(item + offset)
                * Matrix4::from(rotation)
                * Matrix4::from_translation(-item)
        };
        Some([
            around(Quaternion::from_axis_angle(up, Deg(180.0)), side * spacing),
            around(Quaternion::between_vectors(up, toward), -side * spacing),
            around(Quaternion::from_axis_angle(up, Deg(90.0)), up * spacing),
        ])
    }
}

/// Why a capture request found no steady window to average.
#[derive(Debug, PartialEq)]
enum Unsteady {
    /// The posing hand is not tracked (e.g. its controller is still held
    /// without simultaneous hands and controllers).
    NoJoints,
    /// Tracked for less than `STEADY_SECONDS`.
    WindowShort,
    /// Some joint strayed this far (mm) from the window's mean.
    Jitter { max_mm: f32 },
}

impl std::fmt::Display for Unsteady {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Unsteady::NoJoints => f.write_str("no_joints"),
            Unsteady::WindowShort => f.write_str("window_short"),
            Unsteady::Jitter { max_mm } => write!(f, "jitter max_mm={max_mm:.1}"),
        }
    }
}

/// Where the workbench holds the item: ahead of the eye along its yaw (so a
/// pitched head still gets a level bench), at chest height, toward the
/// posing hand's side.
fn bench_spot(
    head_position: Vector3<f32>,
    head_rotation: Quaternion<f32>,
    hand: usize,
) -> Vector3<f32> {
    let forward = crate::ui::PanelPlacement::from_head(head_position, head_rotation).forward;
    let right = forward.cross(Vector3::unit_y());
    let aside = world(if hand == 0 { -BENCH_ASIDE } else { BENCH_ASIDE });
    head_position + forward * world(BENCH_AHEAD) + right * aside
        - Vector3::unit_y() * world(BENCH_DROP)
}

/// Whether a hand pinches, given whether it did last frame; an untracked
/// hand lets go.
fn pinching(was: bool, joints: Option<&HandJoints>) -> bool {
    let Some(joints) = joints else {
        return false;
    };
    let gap = (joints[5] - joints[10]).magnitude();
    gap < world(if was { PINCH_RELEASE } else { PINCH_PRESS })
}

fn window_seconds(frames: &VecDeque<(HandJoints, f32)>) -> f32 {
    frames.iter().map(|(_, dt)| dt).sum()
}

/// f32 sums of 1/60 s frames fall a hair short of the window.
fn spans_window(seconds: f32) -> bool {
    seconds >= STEADY_SECONDS - 1e-4
}

/// The mean pose over a full window in which no joint strays from it.
fn steady_mean(frames: &VecDeque<(HandJoints, f32)>) -> Result<HandJoints, Unsteady> {
    if frames.is_empty() {
        return Err(Unsteady::NoJoints);
    }
    if !spans_window(window_seconds(frames)) {
        return Err(Unsteady::WindowShort);
    }
    let mean: HandJoints = std::array::from_fn(|joint| {
        frames
            .iter()
            .map(|(frame, _)| frame[joint])
            .sum::<Vector3<f32>>()
            / frames.len() as f32
    });
    let max = frames
        .iter()
        .flat_map(|(frame, _)| frame.iter().zip(&mean).map(|(p, m)| (p - m).magnitude()))
        .fold(0.0, f32::max);
    if max > world(STEADY_DISTANCE) {
        return Err(Unsteady::Jitter {
            max_mm: millimetres(max),
        });
    }
    Ok(mean)
}

/// A tracked hand drawn as the glove: the glove fitted over it (world space)
/// and the finger pose it draws with.
pub(super) struct TrackedGlove {
    pub fit: HandFit,
    pub pose: Pose,
}

impl TrackedGlove {
    /// How far (mm) each glove fingertip (thumb..little) lands from the
    /// tracked one.
    pub fn tip_error_mm(&self) -> [f32; 5] {
        self.fit.tip_errors.map(millimetres)
    }
}

pub(super) fn tracked_glove(
    renderer: &mut GloveRenderer,
    joints: &HandJoints,
    handedness: Handedness,
) -> Option<TrackedGlove> {
    let fit = crate::hand_capture::fit_hand(joints, &renderer.rig(handedness))?;
    Some(TrackedGlove {
        pose: Pose::from_finger_bones(&fit.bones),
        fit,
    })
}

/// A saved or staged glove, gripping, named so `/v1/scene` tells it apart
/// from the live gloves.
fn named_glove(
    renderer: &mut GloveRenderer,
    pose: GripPose,
    hand: Handedness,
    shape: &HandShape,
    name: String,
    lights: Option<Rc<LightArray>>,
) -> Vec<SceneObject> {
    let mut glove = renderer.render_hand(
        pose.position,
        pose.rotation,
        hand,
        0.0,
        1.0,
        true,
        None,
        Some((shape.clone(), 1.0)),
        HandLight::Off,
        lights,
    );
    for object in &mut glove {
        let tag = engine::scene::SceneObjectDebugTag {
            name: Some(name.clone()),
            ..object.debug_tag().cloned().unwrap_or_default()
        };
        object.set_debug_tag(Some(Rc::new(tag)));
    }
    glove
}

/// The workbench's hands, lit by `lights`: the posing hand's glove posed on
/// the item at its grip, each tracked hand live, each untracked controller
/// as an open glove, and the posing gloves again in every extra view. While
/// the posing hand is tracked its live glove replaces the saved grip's, which
/// would only be in its way; a staged grip always shows, as the preview.
/// In support mode the posed glove is `support` (its saved or staged pose)
/// and the grip hand's glove is drawn frozen at its grip.
pub(super) fn render(
    tool: &GripPoseTool,
    renderer: &mut GloveRenderer,
    grip: Option<&ResolvedGrip>,
    support: Option<(GripPose, HandShape)>,
    lights: Option<Rc<LightArray>>,
) -> Vec<SceneObject> {
    let handedness = |index: usize| [Handedness::Left, Handedness::Right][index];
    let mut objs = Vec::new();
    let mut posed = Vec::new();
    let mut posing_tracked = false;
    for (index, joints) in tool.tracked.iter().enumerate() {
        let Some(joints) = joints else { continue };
        let Some(glove) = tracked_glove(renderer, joints, handedness(index)) else {
            continue;
        };
        let drawn = renderer.render_hand(
            glove.fit.position,
            glove.fit.rotation,
            handedness(index),
            0.0,
            0.0,
            false,
            None,
            Some((HandShape::Pose(glove.pose), 1.0)),
            HandLight::Off,
            lights.clone(),
        );
        // Only the posing hand belongs in the item's views; the other is
        // pinching somewhere else.
        if index != tool.posing() {
            objs.extend(drawn);
            continue;
        }
        posing_tracked = true;
        posed.extend(drawn);
    }
    let held = grip.map(|grip| (tool.hand_pose, grip.fingers.hand_shape_at(0.0)));
    let (posed_glove, frozen) = if tool.support {
        (support, held)
    } else {
        (held, None)
    };
    if let Some((pose, shape)) = posed_glove.filter(|_| tool.staged.is_some() || !posing_tracked) {
        let hand = handedness(tool.posing());
        posed.extend(named_glove(
            renderer,
            pose,
            hand,
            &shape,
            format!("{hand:?} grip glove"),
            lights.clone(),
        ));
    }
    // In support mode the grip hand's glove stays at its grip: the support
    // pose is relative to it.
    if let Some((pose, shape)) = frozen {
        let hand = handedness(tool.hand);
        posed.extend(named_glove(
            renderer,
            pose,
            hand,
            &shape,
            format!("{hand:?} primary glove"),
            lights.clone(),
        ));
    }
    for (index, controller) in tool.controllers.iter().enumerate() {
        let Some((pose, trigger, squeeze)) = controller else {
            continue;
        };
        // The posing hand's controller is set down somewhere; it has a glove.
        if index == tool.posing() {
            continue;
        }
        objs.extend(renderer.render_hand(
            pose.position,
            pose.rotation,
            handedness(index),
            *trigger,
            *squeeze,
            false,
            None,
            None,
            HandLight::Off,
            lights.clone(),
        ));
    }
    // Built in the unturned bench; drawn turned with it (the live glove so
    // lands back on the real hand).
    let turn = tool.turn_matrix();
    for object in &mut posed {
        object.set_transform(turn * object.get_transform());
    }
    // The posing gloves again in each extra view; the mission redraws the
    // item itself into those views.
    for view in tool.views().into_iter().flatten() {
        for object in &posed {
            let mut copy = object.clone();
            copy.set_transform(view * object.get_transform());
            objs.push(copy);
        }
    }
    objs.append(&mut posed);
    objs
}

/// The control panel, and a beam from each controller in hand, wherever it
/// points: the panel is within reach, so the player needs to see the aim
/// before the ray reaches it.
pub(super) fn render_panel(
    tool: &GripPoseTool,
    assets: &mut engine::assets::asset_cache::AssetCache,
) -> Vec<SceneObject> {
    let Some(placed) = tool.panel else {
        return Vec::new();
    };
    let (canvas, _) = panel::emit(&tool.panel_view(), tool.hover);
    let mut objects = canvas.render_world_space(
        assets,
        placed.transform(),
        None,
        None,
        crate::ui::VR_COMPONENT_Z_STEP,
    );
    let layers = crate::ui::canvas_layers(&objects, &placed);
    crate::util::tag_render_source(
        &mut objects,
        crate::util::render_source::GRIP_WORKBENCH_PANEL,
    );
    let mut beams = crate::ui::tool_pointer_beams(
        &tool.pointer,
        panel::CANVAS,
        &placed,
        layers,
        world(BEAM_CLEARANCE),
        world(BEAM_MISS_LENGTH),
    );
    crate::util::tag_render_source(
        &mut beams,
        crate::util::render_source::GRIP_WORKBENCH_POINTER,
    );
    objects.extend(beams);
    objects
}

/// Where a grip puts its item for a hand pose (as `fit_held_items` places it).
pub(super) fn item_pose(hand: GripPose, grip: &ResolvedGrip, contact: Vector3<f32>) -> GripPose {
    let offset = grip.offset + grip.rotation.rotate_vector(contact * grip.item_scale);
    GripPose {
        position: hand.point(offset),
        rotation: hand.rotation * grip.rotation,
    }
}

/// The grip that holds the item (`item`, drawn at `item_scale` about
/// `contact`) where the glove fitted over a tracked hand (`fit`) holds it,
/// and the hand pose that draws that glove. The grip is in authored units
/// (`personal` being the world scale's `WorldScale::personal`), as the
/// library and overrides store grips, so it holds the item alike at any
/// world scale.
pub(super) fn captured_grip(
    fit: &HandFit,
    item: GripPose,
    contact: Vector3<f32>,
    item_scale: f32,
    personal: f32,
) -> (ResolvedGrip, GripPose) {
    let to_hand = fit.rotation.invert();
    // The item in the controller-grip frame that draws the glove.
    let origin = to_hand.rotate_vector(item.position - fit.position);
    let rotation = (to_hand * item.rotation).normalize();
    let grip = ResolvedGrip {
        item_scale,
        pose_family: "captured".into(),
        // The item origin, less the per-item contact placement adds.
        offset: origin - rotation.rotate_vector(contact * item_scale),
        rotation,
        fingers: crate::vr_grip::FingerPose::Bones {
            bones: fit.bones,
            trigger_bones: None,
        },
        contacts: [None; 5],
        anchor: [0.0; 3],
        score: 0.0,
    };
    (
        grip.rescaled(1.0 / personal),
        GripPose {
            position: fit.position,
            rotation: fit.rotation,
        },
    )
}

/// Write `entry` into the overrides file, replacing its model and hand.
pub(super) fn save_override(path: &Path, entry: &BakedGripEntry) -> std::io::Result<()> {
    edit_overrides(path, |overrides| {
        let entries = &mut overrides.grips.entries;
        entries.retain(|e| e.model != entry.model || e.hand != entry.hand);
        entries.push(entry.clone());
    })
}

/// Write a captured support pose into the overrides file, replacing its
/// model's.
pub(super) fn save_support_override(
    path: &Path,
    model: &str,
    pose: &SupportPose,
) -> std::io::Result<()> {
    edit_overrides(path, |overrides| {
        overrides.support.insert(model.to_owned(), pose.clone());
    })
}

/// Rewrite the overrides file with `edit` applied. A file that exists but
/// cannot be read is left alone rather than replaced.
fn edit_overrides(path: &Path, edit: impl FnOnce(&mut GripOverrides)) -> std::io::Result<()> {
    let mut overrides = match std::fs::read_to_string(path) {
        Ok(text) => serde_json::from_str::<GripOverrides>(&text)
            .ok()
            .filter(|overrides| overrides.grips.version == 1)
            .ok_or_else(|| std::io::Error::other("unreadable overrides file"))?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => GripOverrides::default(),
        Err(error) => return Err(error),
    };
    edit(&mut overrides);
    let mut bytes = serde_json::to_vec_pretty(&overrides)?;
    bytes.push(b'\n');
    // Write then rename, so an interrupted save never leaves a torn file.
    let staged = path.with_extension("json.tmp");
    std::fs::write(&staged, bytes)?;
    std::fs::rename(&staged, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&staged);
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hand_capture::HAND_JOINT_COUNT;
    use crate::vr_grip::FingerPose;
    use cgmath::{Deg, EuclideanSpace, Point3, Quaternion, Rotation3, vec3};

    fn hand_with_tips(gap: f32) -> HandJoints {
        let mut joints = [vec3(0.0, 0.0, 0.0); HAND_JOINT_COUNT];
        joints[10] = vec3(gap, 0.0, 0.0);
        joints
    }

    fn grip() -> ResolvedGrip {
        ResolvedGrip {
            item_scale: 0.8,
            pose_family: "captured".into(),
            offset: vec3(0.1, -0.2, 0.3),
            rotation: Quaternion::from_angle_y(Deg(30.0)),
            fingers: FingerPose::curls([0.5; 5]),
            contacts: [None; 5],
            anchor: [0.0; 3],
            score: 0.0,
        }
    }

    fn posing_tool() -> GripPoseTool {
        let pose = GripPose {
            position: vec3(0.0, 0.0, 0.0),
            rotation: Quaternion::from_angle_y(Deg(0.0)),
        };
        GripPoseTool::new(1, EntityId::dead(), pose, PathBuf::new())
    }

    #[test]
    fn each_trigger_press_is_decided_by_its_own_ray() {
        use crate::input_context::{Hand, InputContext};
        use crate::ui::test_support::{hand_aimed_at, hand_aimed_away, test_panel};
        let mut tool = posing_tool();
        tool.panel = Some(test_panel());
        let reset_view = cgmath::vec2(103.0, 191.0);
        let mut input = InputContext::default();
        // The posing (right) controller set down, resting on its trigger.
        input.right_hand = Hand {
            rotation: Quaternion::new(0.0, 0.0, 0.0, 0.0),
            trigger_value: 1.0,
            ..Hand::default()
        };
        input.left_hand = hand_aimed_at(panel::CANVAS, reset_view, 0.0);
        assert_eq!(tool.point(&input), (None, false));
        input.left_hand.trigger_value = 1.0;
        assert_eq!(
            tool.point(&input),
            (Some(panel::PanelEvent::ResetView), false),
            "the free trigger clicks where its ray is"
        );
        input.left_hand = hand_aimed_away(0.0);
        tool.point(&input);
        input.left_hand.trigger_value = 1.0;
        assert_eq!(tool.point(&input), (None, true), "and snapshots off it");
    }

    #[test]
    fn a_trigger_held_through_a_skipped_frame_neither_clicks_nor_snapshots() {
        use crate::input_context::InputContext;
        use crate::ui::test_support::{hand_aimed_at, hand_aimed_away, test_panel};
        let mut tool = posing_tool();
        tool.panel = Some(test_panel());
        let mut input = InputContext::default();
        input.left_hand = hand_aimed_at(panel::CANVAS, cgmath::vec2(103.0, 191.0), 0.0);
        tool.point(&input);
        // The pause menu over the workbench takes this press (its Resume).
        tool.rearm();
        input.left_hand.trigger_value = 1.0;
        assert_eq!(tool.point(&input), (None, false));
        input.left_hand = hand_aimed_away(1.0);
        assert_eq!(tool.point(&input), (None, false));
    }

    #[test]
    fn switching_hands_moves_the_panel_to_the_newly_free_side() {
        let mut tool = posing_tool();
        let head = vec3(0.0, 5.0, 0.0);
        tool.place(head, Quaternion::new(1.0, 0.0, 0.0, 0.0), head);
        // Posing right, ahead is -Z: the panel hangs on the left (-X).
        assert!(tool.panel.unwrap().center.x < 0.0);
        tool.retarget(0, tool.hand_pose);
        assert!(tool.panel.unwrap().center.x > 0.0);
    }

    fn posing(x: f32) -> Option<HandJoints> {
        Some([vec3(x, 0.0, 0.0); HAND_JOINT_COUNT])
    }

    /// Feed `frames` posing-hand frames (x from `xs`) at `dt`, trigger released.
    fn hold(tool: &mut GripPoseTool, xs: impl IntoIterator<Item = f32>, dt: f32) {
        for x in xs {
            tool.observe([None, posing(x)], false, dt);
        }
    }

    /// A right hand as tracking reports one, in metres.
    fn real_hand() -> HandJoints {
        crate::hand_capture::tests::openxr_right_hand(0.0)
    }

    /// A right-hand glove whose open joints are `open` fitted over `joints`.
    fn fitted(joints: &HandJoints, open: HandJoints) -> HandFit {
        let rig = crate::hand_capture::GloveRig {
            open,
            ..crate::hand_capture::tests::pose_rig(Handedness::Right)
        };
        crate::hand_capture::fit_hand(joints, &rig).unwrap()
    }

    /// A grip captured at world scale `captured` and held at `resolved`, in
    /// metres: the item origin from each glove joint, the item's size per
    /// model unit and its rotation; and the grip as saved.
    fn held_at(
        captured: f32,
        resolved: f32,
    ) -> (Vec<Vector3<f32>>, f32, Quaternion<f32>, ResolvedGrip) {
        // The glove in authored units: smaller than the hand, set off in
        // its controller frame.
        let glove =
            real_hand().map(|p| vec3(0.01, -0.02, 0.03) + p * 0.9 / crate::METERS_PER_WORLD_UNIT);
        let contact = vec3(0.0, 0.1, 0.0);
        let unit = crate::METERS_PER_WORLD_UNIT * captured;
        let item = GripPose {
            position: vec3(0.02, -0.04, -0.08) / unit,
            rotation: Quaternion::from_angle_x(Deg(20.0)),
        };
        let (saved, _) = captured_grip(
            &fitted(&real_hand().map(|p| p / unit), glove.map(|p| p / captured)),
            item,
            contact,
            0.8 / captured,
            1.0 / captured,
        );
        let unit = crate::METERS_PER_WORLD_UNIT * resolved;
        let grip = saved.clone().rescaled(1.0 / resolved);
        let hand = GripPose {
            position: vec3(0.0, 0.0, 0.0),
            rotation: Quaternion::new(1.0, 0.0, 0.0, 0.0),
        };
        let item = item_pose(hand, &grip, contact);
        let from_glove = glove
            .iter()
            .map(|joint| (item.position - joint / resolved) * unit)
            .collect();
        (from_glove, grip.item_scale * unit, item.rotation, saved)
    }

    #[test]
    fn a_capture_holds_the_item_alike_at_every_world_scale() {
        let (baseline, size, rotation, saved) = held_at(1.0, 1.0);
        for (captured, resolved) in [(0.85, 1.0), (1.0, 0.85), (0.85, 0.85), (0.6, 1.3)] {
            let (from_glove, s, r, grip) = held_at(captured, resolved);
            for (a, b) in from_glove.iter().zip(&baseline) {
                assert!(
                    (a - b).magnitude() < 1e-5,
                    "{captured}->{resolved}: {a:?} vs {b:?}"
                );
            }
            assert!(
                (s - size).abs() < 1e-6,
                "{captured}->{resolved}: size {s} vs {size}"
            );
            assert!(r.dot(rotation).abs() > 1.0 - 1e-6);
            // Saved in authored units: the same file at any scale.
            assert!((grip.item_scale - 0.8).abs() < 1e-6, "{}", grip.item_scale);
            assert!((grip.offset - saved.offset).magnitude() < 1e-5);
        }
    }

    const FRAME: f32 = 1.0 / 60.0;
    /// 18 frame intervals at 60 Hz span the window.
    const STEADY_FRAMES: usize = 19;

    #[test]
    fn a_trigger_edge_captures_the_mean_of_a_steady_window() {
        let mut tool = posing_tool();
        let wobble = world(STEADY_DISTANCE) * 0.8;
        hold(
            &mut tool,
            (0..STEADY_FRAMES - 1).map(|i| if i % 2 == 0 { wobble } else { -wobble }),
            FRAME,
        );
        tool.observe([None, posing(0.0)], true, FRAME);
        let captured = tool.capture.take().expect("a steady hand captures");
        // 18 frames alternate +w/-w from +w, then a 0: the mean is 0.
        assert!(captured[0].x.abs() < 1e-6);
    }

    #[test]
    fn a_trigger_held_as_the_workbench_opens_does_not_capture() {
        let mut tool = posing_tool();
        for _ in 0..STEADY_FRAMES * 2 {
            tool.observe([None, posing(0.0)], true, FRAME);
        }
        assert!(tool.capture.is_none(), "entered already pressed");
        tool.observe([None, posing(0.0)], false, FRAME);
        tool.observe([None, posing(0.0)], true, FRAME);
        assert!(tool.capture.is_some(), "a fresh press captures");
    }

    #[test]
    fn the_window_is_measured_in_seconds_not_frames() {
        let mut tool = posing_tool();
        hold(&mut tool, [0.0; STEADY_FRAMES - 2], FRAME);
        tool.observe([None, posing(0.0)], true, FRAME);
        assert!(tool.capture.is_none(), "0.283 s is not yet steady");
        // Four frames 0.1 s apart span the window at any frame rate, but one
        // long frame spans none.
        let mut slow = posing_tool();
        slow.observe([None, posing(0.0)], true, 0.3);
        assert!(slow.capture.is_none(), "a lone long frame");
        hold(&mut slow, [0.0; 3], 0.1);
        slow.observe([None, posing(0.0)], true, 0.1);
        assert!(slow.capture.is_some());
        // At 120 Hz the window is 37 frames, and older frames fall out of it.
        let mut fast = posing_tool();
        hold(&mut fast, [10.0; 50], 1.0 / 120.0);
        hold(&mut fast, [0.0; 36], 1.0 / 120.0);
        fast.observe([None, posing(0.0)], true, 1.0 / 120.0);
        assert_eq!(fast.capture.expect("37 steady frames")[0].x, 0.0);
    }

    #[test]
    fn a_real_hand_held_still_captures_despite_tracking_jitter() {
        // Quest 3 hands held still strayed up to ~13 mm from their mean.
        let mut tool = posing_tool();
        let wobble = world(0.0133);
        hold(
            &mut tool,
            (0..STEADY_FRAMES - 1).map(|i| if i % 2 == 0 { wobble } else { -wobble }),
            FRAME,
        );
        tool.observe([None, posing(0.0)], true, FRAME);
        assert!(tool.capture.is_some(), "{:?}", tool.capture_failure);
    }

    #[test]
    fn a_jittering_hand_does_not_capture() {
        let mut tool = posing_tool();
        hold(&mut tool, [0.0; STEADY_FRAMES - 1], FRAME);
        tool.observe([None, posing(world(STEADY_DISTANCE) * 2.0)], true, FRAME);
        assert!(tool.capture.is_none());
    }

    #[test]
    fn losing_tracking_restarts_the_window() {
        let mut tool = posing_tool();
        hold(&mut tool, [0.0; STEADY_FRAMES], FRAME);
        tool.observe([None, None], false, FRAME);
        tool.observe([None, posing(0.0)], true, FRAME);
        assert!(tool.capture.is_none());
    }

    #[test]
    fn a_capture_rearms_only_once_trigger_and_pinch_release() {
        let mut tool = posing_tool();
        let open = Some(hand_with_tips(1.0));
        let pinch = Some(hand_with_tips(0.0));
        hold(&mut tool, [0.0; STEADY_FRAMES], FRAME);
        tool.observe([None, posing(0.0)], true, FRAME);
        assert!(tool.capture.take().is_some(), "trigger edge captures");
        tool.observe([pinch, posing(0.0)], true, FRAME);
        assert!(
            tool.capture.is_none(),
            "a held trigger or pinch captures once"
        );
        tool.observe([open, posing(0.0)], false, FRAME);
        tool.observe([pinch, posing(0.0)], false, FRAME);
        assert!(tool.capture.take().is_some(), "a fresh pinch captures");
        // A press while unsteady is spent: steadying under it fires nothing.
        tool.observe([open, posing(1.0)], false, FRAME);
        tool.observe([open, posing(0.0)], true, FRAME);
        assert!(tool.capture.is_none(), "unsteady");
        for _ in 0..STEADY_FRAMES {
            tool.observe([open, posing(0.0)], true, FRAME);
        }
        assert!(tool.capture.is_none(), "still the same press");
    }

    fn tool_with_item(position: Vector3<f32>, viewer: Vector3<f32>) -> GripPoseTool {
        let mut tool = GripPoseTool::new(
            1,
            EntityId::dead(),
            GripPose {
                position,
                rotation: Quaternion::from_angle_y(Deg(0.0)),
            },
            PathBuf::new(),
        );
        tool.item = Some((
            GripPose {
                position,
                rotation: Quaternion::from_angle_y(Deg(90.0)),
            },
            2.0,
        ));
        tool.viewer = viewer;
        tool
    }

    fn turned_by(a: Quaternion<f32>, b: Quaternion<f32>) -> bool {
        a.normalize().dot(b.normalize()).abs() > 1.0 - 1e-5
    }

    #[test]
    fn a_control_held_on_the_newly_free_controller_waits_for_release() {
        let identity = Quaternion::new(1.0, 0.0, 0.0, 0.0);
        let mut tool = tool_with_item(vec3(1.0, 2.0, 3.0), vec3(1.0, 2.0, 6.0));
        let grab = Quaternion::from_angle_x(Deg(10.0));
        // The free (left) controller's squeeze let go: armed.
        tool.turn_with(Some(grab), false);
        // Support mode frees the right controller, its squeeze still round
        // the item it held: that is no turn, nor is its stick a resize.
        tool.set_support(true);
        tool.turn_with(Some(grab), true);
        tool.turn_with(Some(Quaternion::from_angle_y(Deg(90.0)) * grab), true);
        assert!(turned_by(tool.turn, identity));
        assert_eq!(tool.scale_input(1.0, true), 0.0);
    }

    #[test]
    fn a_squeeze_turns_the_bench_with_the_controller_and_it_stays_turned() {
        let identity = Quaternion::new(1.0, 0.0, 0.0, 0.0);
        let mut tool = tool_with_item(vec3(1.0, 2.0, 3.0), vec3(1.0, 2.0, 6.0));
        let grab = Quaternion::from_angle_x(Deg(10.0));
        let quarter = Quaternion::from_angle_y(Deg(90.0));
        // A squeeze held as the workbench opens turns nothing.
        tool.turn_with(Some(grab), true);
        tool.turn_with(Some(quarter * grab), true);
        assert!(turned_by(tool.turn, identity));
        // A fresh squeeze turns it 1:1 with the controller.
        tool.turn_with(Some(grab), false);
        tool.turn_with(Some(grab), true);
        tool.turn_with(Some(quarter * grab), true);
        assert!(turned_by(tool.turn, quarter));
        // Lost mid-turn: it holds; let go: it stays.
        tool.turn_with(None, true);
        tool.turn_with(Some(grab), false);
        assert!(turned_by(tool.turn, quarter));
        // A second squeeze turns on from there.
        tool.turn_with(Some(grab), true);
        tool.turn_with(Some(quarter * grab), true);
        assert!(turned_by(tool.turn, quarter * quarter));
        // About the item, which stays put.
        let pivot = tool.pivot();
        let moved = tool.turn_matrix() * pivot.extend(1.0);
        assert!((moved.truncate() - pivot).magnitude() < 1e-5);
        tool.reset_view();
        assert!(turned_by(tool.turn, identity));
    }

    #[test]
    fn turning_the_bench_never_changes_what_a_hand_captures() {
        let mut tool = tool_with_item(vec3(1.0, 2.0, 3.0), vec3(1.0, 2.0, 6.0));
        let (item, _) = tool.item.unwrap();
        let unit = crate::world_scale::meters_per_world_unit();
        let hand = real_hand().map(|p| item.position + vec3(0.02, -0.03, 0.05) / unit + p / unit);
        let glove = real_hand().map(|p| p * 0.9 / crate::METERS_PER_WORLD_UNIT);
        let contact = vec3(0.0, 0.1, 0.0);
        let capture =
            |joints: &HandJoints| captured_grip(&fitted(joints, glove), item, contact, 0.8, 1.0).0;
        let unturned = capture(&hand);
        // Turned, the real hand holds the drawn (turned) item the same way:
        // the tool reads it back in the unturned bench.
        tool.turn = Quaternion::from_axis_angle(vec3(0.3, 1.0, 0.2).normalize(), Deg(75.0));
        let seen = hand.map(|p| tool.from_bench(p));
        let turned = capture(&seen.map(|p| tool.to_bench(p)));
        assert!((turned.offset - unturned.offset).magnitude() < 1e-4);
        assert!(turned_by(turned.rotation, unturned.rotation));
        assert_eq!(turned.item_scale, unturned.item_scale);
    }

    #[test]
    fn a_free_controller_out_of_hand_never_snapshots_and_must_let_go_on_return() {
        use crate::input_context::InputContext;
        use crate::ui::test_support::{hand_aimed_away, test_panel};
        let mut tool = posing_tool();
        tool.panel = Some(test_panel());
        let mut input = InputContext::default();
        input.left_hand = hand_aimed_away(0.0);
        tool.point(&input);
        // Set down (no ray), its trigger pressed: nothing.
        input.left_hand.rotation = Quaternion::new(0.0, 0.0, 0.0, 0.0);
        input.left_hand.trigger_value = 1.0;
        assert_eq!(tool.point(&input), (None, false));
        // Back in hand with the trigger still down: still nothing.
        input.left_hand = hand_aimed_away(1.0);
        assert_eq!(tool.point(&input), (None, false));
        input.left_hand.trigger_value = 0.0;
        tool.point(&input);
        input.left_hand.trigger_value = 1.0;
        assert_eq!(tool.point(&input), (None, true), "a fresh press in hand");
    }

    #[test]
    fn a_turn_or_resize_lost_out_of_hand_resumes_only_after_letting_go() {
        let identity = Quaternion::new(1.0, 0.0, 0.0, 0.0);
        let mut tool = tool_with_item(vec3(1.0, 2.0, 3.0), vec3(1.0, 2.0, 6.0));
        let grab = Quaternion::from_angle_x(Deg(10.0));
        let quarter = Quaternion::from_angle_y(Deg(90.0));
        tool.turn_with(Some(grab), false);
        tool.turn_with(Some(grab), true);
        // Lost mid-turn, found again still squeezed and turned: no jump.
        tool.turn_with(None, true);
        tool.turn_with(Some(quarter * grab), true);
        assert!(turned_by(tool.turn, identity));
        // The stick resizes only once seen centred in hand.
        assert_eq!(
            tool.scale_input(1.0, true),
            0.0,
            "pushed as the bench opened"
        );
        assert_eq!(tool.scale_input(0.0, true), 0.0);
        assert_eq!(tool.scale_input(1.0, true), 1.0);
        assert_eq!(tool.scale_input(1.0, false), 0.0, "out of hand");
        assert_eq!(
            tool.scale_input(1.0, true),
            0.0,
            "back in hand, still pushed"
        );
    }

    #[test]
    fn losing_the_pinching_hand_never_makes_a_second_capture() {
        let mut tool = posing_tool();
        let open = Some(hand_with_tips(1.0));
        let pinch = Some(hand_with_tips(0.0));
        hold(&mut tool, [0.0; STEADY_FRAMES], FRAME);
        tool.observe([open, posing(0.0)], false, FRAME);
        tool.observe([pinch, posing(0.0)], false, FRAME);
        assert!(tool.capture.take().is_some(), "a pinch captures");
        tool.observe([None, posing(0.0)], false, FRAME);
        tool.observe([pinch, posing(0.0)], false, FRAME);
        assert!(
            tool.capture.is_none(),
            "found still pinched: the same pinch"
        );
        tool.observe([open, posing(0.0)], false, FRAME);
        tool.observe([pinch, posing(0.0)], false, FRAME);
        assert!(tool.capture.is_some(), "seen open, a new pinch captures");
    }

    #[test]
    fn a_free_controller_aims_even_while_its_hand_reports_joints() {
        // Quest multimodal: the hand holding the free controller is tracked
        // too. That controller still aims; only the posing hand's tracked
        // joints mean its controller is set down.
        assert!(controller_in_hand(false, true, true));
        assert!(controller_in_hand(false, false, true));
        assert!(!controller_in_hand(true, true, true));
        assert!(controller_in_hand(true, false, true));
        assert!(!controller_in_hand(false, false, false), "untracked");
    }

    #[test]
    fn a_tracked_free_hand_taps_only_without_its_controller() {
        use crate::input_context::InputContext;
        use crate::ui::test_support::{hand_aimed_away, test_panel};
        let panel = test_panel();
        let reset_view =
            crate::ui::canvas_to_panel_world(panel::CANVAS, &panel, cgmath::vec2(103.0, 191.0));
        // The free (left) index tip pushed from in front to through Reset view.
        let tip_at = |depth: f32| {
            let mut joints = [vec3(0.0, 0.0, 0.0); HAND_JOINT_COUNT];
            joints[crate::hand_capture::TIPS[1]] = reset_view + panel.normal() * world(depth);
            Some(joints)
        };
        for (holding, presses) in [(true, false), (false, true)] {
            let mut tool = posing_tool();
            tool.panel = Some(panel);
            let mut input = InputContext::default();
            input.left_hand = hand_aimed_away(0.0);
            if !holding {
                input.left_hand.rotation = Quaternion::new(0.0, 0.0, 0.0, 0.0);
            }
            input.left_hand.joints = tip_at(0.02);
            tool.point(&input);
            assert_eq!(tool.panel_view().fingertip, !holding);
            input.left_hand.joints = tip_at(-0.015);
            let (event, _) = tool.point(&input);
            assert_eq!(
                event == Some(panel::PanelEvent::ResetView),
                presses,
                "holding the controller: {holding}"
            );
        }
    }

    #[test]
    fn a_press_aimed_just_off_the_panel_edge_does_not_snapshot() {
        use crate::input_context::InputContext;
        use crate::ui::test_support::{hand_aimed_at, test_panel};
        // Just right of Exit, past the panel's edge (x 400): ~2 cm off on
        // the test panel, then ~15 cm off.
        for (x, snapshots) in [(405.0, false), (440.0, true)] {
            let mut tool = posing_tool();
            tool.panel = Some(test_panel());
            let mut input = InputContext::default();
            input.left_hand = hand_aimed_at(panel::CANVAS, cgmath::vec2(x, 191.0), 0.0);
            assert_eq!(tool.point(&input), (None, false));
            input.left_hand.trigger_value = 1.0;
            assert_eq!(tool.point(&input), (None, snapshots), "aimed at x {x}");
        }
    }

    #[test]
    fn views_sit_beside_the_item_turned_toward_the_viewer() {
        use cgmath::Transform;
        let item = vec3(1.0, 2.0, 3.0);
        let tool = tool_with_item(item, item + vec3(0.0, -1.0, 4.0));
        let [back, top, profile] = tool.views().unwrap();
        let toward = vec3(0.0, 0.0, 1.0);
        let at = |view: Matrix4<f32>, p: Vector3<f32>| {
            view.transform_point(Point3::from_vec(p)).to_vec()
        };
        // Each view moves the item aside, by the same spacing.
        for view in [back, top, profile] {
            assert!(((at(view, item) - item).magnitude() - world(VIEW_SPACING)).abs() < 1e-4);
        }
        // The far side faces the viewer, and the top tips toward them.
        let back_center = at(back, item);
        assert!((at(back, item - toward) - (back_center + toward)).magnitude() < 1e-4);
        let top_center = at(top, item);
        let up = vec3(0.0, 1.0, 0.0);
        assert!((at(top, item + up) - (top_center + toward)).magnitude() < 1e-4);
        assert!((at(profile, item) - (item + up * world(VIEW_SPACING))).magnitude() < 1e-4);
        // Straight above the item there is no direction to face.
        assert!(tool_with_item(item, item + up).views().is_none());
    }

    #[test]
    fn the_bench_is_level_ahead_at_chest_height_toward_the_posing_hand() {
        let eye = vec3(1.0, 5.0, 2.0);
        // Facing +X (yaw -90 from -Z), pitched well down at the floor.
        let head = Quaternion::from_angle_y(Deg(-90.0)) * Quaternion::from_angle_x(Deg(-60.0));
        let right = bench_spot(eye, head, 1);
        let ahead = vec3(1.0, 0.0, 0.0);
        let offset = right - eye;
        assert!(
            (offset.dot(ahead) - world(BENCH_AHEAD)).abs() < 1e-4,
            "{offset:?}"
        );
        assert!((offset.y + world(BENCH_DROP)).abs() < 1e-4);
        // The viewer's right is +Z when facing +X.
        assert!((offset.z - world(BENCH_ASIDE)).abs() < 1e-4);
        let left = bench_spot(eye, head, 0);
        assert!(((left - eye).z + world(BENCH_ASIDE)).abs() < 1e-4);
        // An untracked (zero) head still gets a defined bench.
        let untracked = bench_spot(eye, Quaternion::new(0.0, 0.0, 0.0, 0.0), 1);
        assert!(untracked.x.is_finite() && untracked.y.is_finite() && untracked.z.is_finite());
    }

    #[test]
    fn the_room_stands_on_the_real_floor_and_its_key_light_is_behind_the_viewer() {
        // The viewer stands off the item's +X, looking along -X.
        let item = vec3(1.0, 2.0, 3.0);
        let mut tool = tool_with_item(item, item + vec3(4.0, 1.0, 0.0));
        tool.floor_y = -0.5;
        let room = tool.environment();
        let floor = &room[0];
        let top = floor.get_transform() * cgmath::Vector4::new(0.0, 0.5, 0.0, 1.0);
        assert!((top.y - tool.floor_y).abs() < 1e-5, "{top:?}");
        assert!((top.x - tool.viewer.x).abs() < 1e-5 && (top.z - tool.viewer.z).abs() < 1e-5);
        // `None` only with object lighting off: a process-wide dev param that
        // a parallel test may flip, so it is not forced here.
        if let Some(lights) = tool.lights() {
            let key = lights.get_light(0).unwrap().position() - item;
            assert!(
                key.x > 0.0,
                "behind the viewer, who looks along -X: {key:?}"
            );
            assert!(key.z > 0.0, "and over their left shoulder (+Z): {key:?}");
        }
    }

    #[test]
    fn placing_carries_the_glove_with_the_item_onto_the_bench() {
        let mut tool = posing_tool();
        let grip_offset = vec3(0.0, 0.0, -0.2);
        let item = tool.hand_pose.position + grip_offset;
        let eye = vec3(3.0, 4.0, 5.0);
        let head = Quaternion::from_angle_y(Deg(0.0));
        tool.place(eye, head, item);
        let spot = bench_spot(eye, head, 1);
        assert!(tool.placed);
        assert!(((tool.hand_pose.position + grip_offset) - spot).magnitude() < 1e-5);
        assert_eq!(tool.viewer, eye, "the views face where the player stood");
    }

    #[test]
    fn a_controller_untracked_as_the_workbench_opens_still_poses() {
        // An untracked controller reports the zero quaternion.
        let untracked = GripPose {
            position: vec3(0.0, 5.0, 0.0),
            rotation: Quaternion::new(0.0, 0.0, 0.0, 0.0),
        };
        let tool = GripPoseTool::new(1, EntityId::dead(), untracked, PathBuf::new());
        assert!((tool.hand_pose.rotation.magnitude() - 1.0).abs() < 1e-6);
        let item = item_pose(tool.hand_pose, &grip(), vec3(0.0, 0.0, 0.0));
        assert!(item.is_tracked(), "a finite item pose");
    }

    #[test]
    fn a_pinch_starts_under_two_centimetres_and_ends_over_three() {
        let gap = |metres: f32| hand_with_tips(world(metres));
        assert!(!pinching(false, Some(&gap(0.025))));
        assert!(pinching(false, Some(&gap(0.015))));
        assert!(pinching(true, Some(&gap(0.025))), "held through the band");
        assert!(!pinching(true, Some(&gap(0.035))));
        assert!(!pinching(true, None), "an untracked hand lets go");
    }

    #[test]
    fn a_failed_capture_says_why() {
        let mut tool = posing_tool();
        let press = |tool: &mut GripPoseTool, hand: Option<HandJoints>| {
            tool.observe([None, hand], false, FRAME);
            tool.observe([None, hand], true, FRAME);
        };
        press(&mut tool, None);
        assert_eq!(tool.capture_failure.as_deref(), Some("no_joints"));
        press(&mut tool, posing(0.0));
        assert_eq!(tool.capture_failure.as_deref(), Some("window_short"));
        hold(&mut tool, [0.0; STEADY_FRAMES - 2], FRAME);
        tool.observe([None, posing(world(STEADY_DISTANCE) * 2.0)], true, FRAME);
        // One frame 2 x 5 mm out pulls the 19-frame mean 1/19 of the way.
        let expected = STEADY_DISTANCE * 2.0 * 18.0 / 19.0 * 1000.0;
        assert_eq!(
            tool.capture_failure.as_deref(),
            Some(format!("jitter max_mm={expected:.1}").as_str())
        );
        hold(&mut tool, [0.0; STEADY_FRAMES], FRAME);
        tool.observe([None, posing(0.0)], true, FRAME);
        assert!(tool.capture.is_some());
        assert_eq!(tool.capture_failure, None, "a capture clears it");
    }

    #[test]
    fn scaling_needs_a_deliberate_push_and_stays_in_range() {
        assert_eq!(GripPoseTool::scale(1.0, 0.1, 1.0), 1.0);
        assert!(GripPoseTool::scale(1.0, 1.0, 0.5) > 1.0);
        assert!(GripPoseTool::scale(1.0, -1.0, 0.5) < 1.0);
        assert_eq!(GripPoseTool::scale(2.9, 1.0, 10.0), 3.0);
    }

    fn entry(hand: &str, item_scale: f32) -> BakedGripEntry {
        BakedGripEntry {
            authored: true,
            model: "mug".into(),
            hand: hand.into(),
            surface_hash: String::new(),
            kinematics_hash: String::new(),
            hints_hash: String::new(),
            grip: ResolvedGrip {
                item_scale,
                ..grip()
            },
        }
    }

    #[test]
    fn saving_replaces_only_that_model_and_hand() {
        let dir = crate::test_support::TempDir::new("pose-tool");
        let path = dir.path().join(crate::vr_grip::GRIP_OVERRIDES_FILE);
        save_override(&path, &entry("left", 0.8)).unwrap();
        save_override(&path, &entry("right", 0.8)).unwrap();
        save_override(&path, &entry("left", 1.5)).unwrap();
        let library = GripOverrides::load(&path).unwrap().grips;
        assert_eq!(library.entries.len(), 2);
        assert_eq!(library.lookup("mug", "left").unwrap().item_scale, 1.5);
        assert_eq!(library.lookup("mug", "right").unwrap().item_scale, 0.8);
    }

    #[test]
    fn grip_and_support_saves_keep_each_other() {
        let dir = crate::test_support::TempDir::new("pose-tool-support");
        let path = dir.path().join(crate::vr_grip::GRIP_OVERRIDES_FILE);
        let pose = |x| SupportPose {
            palm_anchor: [x, 0.0, 0.0],
            rotation_degrees: [0.0; 3],
            fingers: crate::vr_grip::FingerPose::curls([0.5; 5]).to_bones(),
        };
        save_override(&path, &entry("right", 0.8)).unwrap();
        save_support_override(&path, "sg_h", &pose(-0.4)).unwrap();
        save_support_override(&path, "sg_h", &pose(-0.5)).unwrap();
        save_override(&path, &entry("left", 0.8)).unwrap();
        let saved = GripOverrides::load(&path).unwrap();
        assert_eq!(saved.grips.entries.len(), 2);
        assert_eq!(saved.support.len(), 1);
        assert_eq!(saved.support["sg_h"], pose(-0.5));
    }

    #[test]
    fn saving_never_replaces_an_unreadable_overrides_file() {
        let dir = crate::test_support::TempDir::new("pose-tool-corrupt");
        let path = dir.path().join(crate::vr_grip::GRIP_OVERRIDES_FILE);
        std::fs::write(&path, "{ hand edited, oops").unwrap();
        assert!(save_override(&path, &entry("left", 0.8)).is_err());
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "{ hand edited, oops"
        );
    }
}
