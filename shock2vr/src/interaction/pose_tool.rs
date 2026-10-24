//! In-VR grip authoring (a developer cheat). The posing hand is held where it
//! was, so the item stays put and stays held after its controller is set
//! down; a tracked hand then forms the grip around it. The other hand's
//! controller trigger (or its tracked pinch) stages that pose live; its lower
//! button saves it as a headset override and its upper button discards it.
use std::collections::VecDeque;
use std::path::{Path, PathBuf};

use cgmath::{
    Deg, EuclideanSpace, InnerSpace, Matrix4, Point3, Quaternion, Rotation, Rotation3, Vector3,
};
use shipyard::EntityId;

use crate::hand_capture::HandJoints;
use crate::vr_grip::{BakedGripEntry, GripLibrary, GripSurface, ResolvedGrip};
use crate::vr_support::GripPose;

/// Thumb and index tips closer than this pinch (world units).
const PINCH_DISTANCE: f32 = 0.02 / crate::METERS_PER_WORLD_UNIT;
/// Seconds of steady posing-hand tracking averaged into a capture.
const STEADY_SECONDS: f32 = 0.3;
/// Every joint stays this close to its window mean to count as steady
/// (world units).
const STEADY_DISTANCE: f32 = 0.005 / crate::METERS_PER_WORLD_UNIT;
/// Natural-log item scale change per second at full thumbstick.
const SCALE_RATE: f32 = 0.7;
/// The grip editor's item scale range.
const SCALE_RANGE: std::ops::RangeInclusive<f32> = 0.1..=3.0;
/// A glove fingertip this close to the item's surface touches it (world units).
const TOUCH_DISTANCE: f32 = 0.01 / crate::METERS_PER_WORLD_UNIT;
/// How far beside the item each extra view of it sits (world units).
const VIEW_SPACING: f32 = 0.35 / crate::METERS_PER_WORLD_UNIT;

pub(super) struct GripPoseTool {
    /// The posing hand (0 left, 1 right) and the item it holds.
    pub hand: usize,
    pub entity: EntityId,
    /// World pose the posing hand is held at, so its item does not move.
    pub hand_pose: GripPose,
    /// World-space joints of each tracked hand this frame, drawn as gloves.
    pub tracked: [Option<HandJoints>; 2],
    /// Averaged posing-hand joints awaiting capture.
    pub capture: Option<HandJoints>,
    /// A capture shown live but not yet saved.
    pub staged: Option<super::StagedGrip>,
    pub last_save_error: Option<String>,
    pub overrides_path: PathBuf,
    /// The held item's model frame (where its grip surface lies) and scale,
    /// and that surface, for the fingertip contacts and the extra views.
    pub item: Option<(GripPose, f32)>,
    pub surface: Option<GripSurface>,
    /// The player, whom the extra views face.
    pub viewer: Vector3<f32>,
    /// Recent posing-hand frames, each with the time since the one before
    /// (zero for the oldest), spanning the steady window.
    recent: VecDeque<(HandJoints, f32)>,
    requested: bool,
}

impl GripPoseTool {
    pub fn new(
        hand: usize,
        entity: EntityId,
        hand_pose: GripPose,
        overrides_path: PathBuf,
    ) -> Self {
        Self {
            hand,
            entity,
            hand_pose,
            tracked: [None, None],
            capture: None,
            staged: None,
            last_save_error: None,
            overrides_path,
            item: None,
            surface: None,
            viewer: hand_pose.position,
            recent: VecDeque::new(),
            requested: false,
        }
    }

    /// Take this frame's tracked hands. The leading edge of `trigger` (the
    /// other controller's) or the other hand's pinch queues a capture of the
    /// posing hand, if it has held steady for the last `STEADY_SECONDS`.
    /// Losing its tracking restarts that window.
    pub fn observe(&mut self, tracked: [Option<HandJoints>; 2], trigger: bool, dt: f32) {
        match tracked[self.hand] {
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
        let requested =
            trigger || tracked[1 - self.hand].is_some_and(|joints| is_pinching(&joints));
        if requested && !self.requested {
            self.capture = steady_mean(&self.recent);
            if self.capture.is_none() {
                println!("SHOCK2QUEST_VR_GRIP_POSE_TOOL captured=false reason=unsteady");
            }
        }
        self.requested = requested;
        self.tracked = tracked;
    }

    /// Thumbstick y grows or shrinks the item while its controller is held.
    pub fn scale(item_scale: f32, thumbstick_y: f32, dt: f32) -> f32 {
        if thumbstick_y.abs() < 0.2 {
            return item_scale;
        }
        (item_scale * (thumbstick_y * SCALE_RATE * dt).exp())
            .clamp(*SCALE_RANGE.start(), *SCALE_RANGE.end())
    }

    /// A tracked hand draws as a live glove instead of its set-down
    /// controller's; the posing hand keeps its held glove as well, showing
    /// the saved grip to compare the live hand against.
    pub fn hides_controller_glove(&self, hand: usize) -> bool {
        hand != self.hand && self.tracked[hand].is_some()
    }

    /// Transforms that redraw the item and its gloves beside it, turned to
    /// show the viewer its far side, its top and its profile.
    pub fn views(&self) -> Option<[Matrix4<f32>; 3]> {
        let (item, _) = self.item?;
        let item = item.position;
        let up = Vector3::unit_y();
        let toward = crate::scripts::ai::ai_util::normalized_horizontal(self.viewer - item)?;
        let side = up.cross(toward);
        let around = |rotation: Quaternion<f32>, offset: Vector3<f32>| {
            Matrix4::from_translation(item + offset)
                * Matrix4::from(rotation)
                * Matrix4::from_translation(-item)
        };
        Some([
            around(
                Quaternion::from_axis_angle(up, Deg(180.0)),
                side * VIEW_SPACING,
            ),
            around(
                Quaternion::between_vectors(up, toward),
                -side * VIEW_SPACING,
            ),
            around(
                Quaternion::from_axis_angle(up, Deg(90.0)),
                up * VIEW_SPACING,
            ),
        ])
    }

    /// Which of a glove's world-space fingertips touch the held item.
    pub fn touching(&self, tips: [Vector3<f32>; 5]) -> [bool; 5] {
        let (Some((item, scale)), Some(surface)) = (self.item, &self.surface) else {
            return [false; 5];
        };
        tips.map(|tip| {
            let local = item.rotation.invert().rotate_vector(tip - item.position) / scale;
            surface.distance(Point3::from_vec(local)) * scale < TOUCH_DISTANCE
        })
    }
}

fn is_pinching(joints: &HandJoints) -> bool {
    (joints[5] - joints[10]).magnitude() < PINCH_DISTANCE
}

fn window_seconds(frames: &VecDeque<(HandJoints, f32)>) -> f32 {
    frames.iter().map(|(_, dt)| dt).sum()
}

/// f32 sums of 1/60 s frames fall a hair short of the window.
fn spans_window(seconds: f32) -> bool {
    seconds >= STEADY_SECONDS - 1e-4
}

/// The mean pose over a full window in which no joint strays from it.
fn steady_mean(frames: &VecDeque<(HandJoints, f32)>) -> Option<HandJoints> {
    if !spans_window(window_seconds(frames)) {
        return None;
    }
    let mean: HandJoints = std::array::from_fn(|joint| {
        frames
            .iter()
            .map(|(frame, _)| frame[joint])
            .sum::<Vector3<f32>>()
            / frames.len() as f32
    });
    frames
        .iter()
        .all(|(frame, _)| {
            frame
                .iter()
                .zip(&mean)
                .all(|(p, m)| (p - m).magnitude() <= STEADY_DISTANCE)
        })
        .then_some(mean)
}

/// Where a grip puts its item for a hand pose (as `fit_held_items` places it).
pub(super) fn item_pose(hand: GripPose, grip: &ResolvedGrip, contact: Vector3<f32>) -> GripPose {
    let offset = grip.offset + grip.rotation.rotate_vector(contact * grip.item_scale);
    GripPose {
        position: hand.point(offset),
        rotation: hand.rotation * grip.rotation,
    }
}

/// Write `entry` into the overrides file, replacing its model and hand. A
/// file that exists but cannot be read is left alone rather than replaced.
pub(super) fn save_override(path: &Path, entry: &BakedGripEntry) -> std::io::Result<()> {
    let mut library = match std::fs::read_to_string(path) {
        Ok(text) => serde_json::from_str::<GripLibrary>(&text)
            .ok()
            .filter(|library| library.version == 1)
            .ok_or_else(|| std::io::Error::other("unreadable overrides file"))?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => GripLibrary::default(),
        Err(error) => return Err(error),
    };
    library
        .entries
        .retain(|e| e.model != entry.model || e.hand != entry.hand);
    library.entries.push(entry.clone());
    let mut bytes = serde_json::to_vec_pretty(&library)?;
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
    use cgmath::{Deg, Quaternion, Rotation3, vec3};

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

    fn posing(x: f32) -> Option<HandJoints> {
        Some([vec3(x, 0.0, 0.0); HAND_JOINT_COUNT])
    }

    /// Feed `frames` posing-hand frames (x from `xs`) at `dt`, trigger released.
    fn hold(tool: &mut GripPoseTool, xs: impl IntoIterator<Item = f32>, dt: f32) {
        for x in xs {
            tool.observe([None, posing(x)], false, dt);
        }
    }

    const FRAME: f32 = 1.0 / 60.0;
    /// 18 frame intervals at 60 Hz span the window.
    const STEADY_FRAMES: usize = 19;

    #[test]
    fn a_trigger_edge_captures_the_mean_of_a_steady_window() {
        let mut tool = posing_tool();
        let wobble = STEADY_DISTANCE * 0.8;
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
    fn a_jittering_hand_does_not_capture() {
        let mut tool = posing_tool();
        hold(&mut tool, [0.0; STEADY_FRAMES - 1], FRAME);
        tool.observe([None, posing(STEADY_DISTANCE * 2.0)], true, FRAME);
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
            assert!(((at(view, item) - item).magnitude() - VIEW_SPACING).abs() < 1e-4);
        }
        // The far side faces the viewer, and the top tips toward them.
        let back_center = at(back, item);
        assert!((at(back, item - toward) - (back_center + toward)).magnitude() < 1e-4);
        let top_center = at(top, item);
        let up = vec3(0.0, 1.0, 0.0);
        assert!((at(top, item + up) - (top_center + toward)).magnitude() < 1e-4);
        assert!((at(profile, item) - (item + up * VIEW_SPACING)).magnitude() < 1e-4);
        // Straight above the item there is no direction to face.
        assert!(tool_with_item(item, item + up).views().is_none());
    }

    #[test]
    fn fingertips_touch_within_a_centimetre_of_the_scaled_surface() {
        let p = |x: f32, y: f32, z: f32| Point3::new(x, y, z);
        // A wide square in the model's y = 0 plane.
        let plane = [
            [p(-1.0, 0.0, -1.0), p(1.0, 0.0, -1.0), p(1.0, 0.0, 1.0)],
            [p(-1.0, 0.0, -1.0), p(1.0, 0.0, 1.0), p(-1.0, 0.0, 1.0)],
        ];
        let item = vec3(0.0, 5.0, 0.0);
        let mut tool = tool_with_item(item, item + vec3(0.0, 0.0, 4.0));
        assert_eq!(tool.touching([item; 5]), [false; 5], "no surface yet");
        tool.surface = GripSurface::new(&plane);
        let near = item + vec3(0.0, TOUCH_DISTANCE * 0.5, 0.0);
        let far = item + vec3(0.0, TOUCH_DISTANCE * 2.0, 0.0);
        assert_eq!(
            tool.touching([near, far, near, far, near]),
            [true, false, true, false, true]
        );
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
        let library = GripLibrary::load_overrides(&path).unwrap();
        assert_eq!(library.entries.len(), 2);
        assert_eq!(library.lookup("mug", "left").unwrap().item_scale, 1.5);
        assert_eq!(library.lookup("mug", "right").unwrap().item_scale, 0.8);
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
