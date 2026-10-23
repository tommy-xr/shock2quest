//! In-VR grip authoring (a developer cheat). The posing hand is held where it
//! was, so the item stays put and stays held after its controller is set
//! down; a tracked hand then forms the grip around it, and a pinch of the
//! other hand saves that pose as a headset override that applies at once.
use std::collections::VecDeque;
use std::path::{Path, PathBuf};

use cgmath::{InnerSpace, Rotation, Vector3};
use shipyard::EntityId;

use crate::hand_capture::HandJoints;
use crate::vr_grip::{BakedGripEntry, GripLibrary, ResolvedGrip};
use crate::vr_support::GripPose;

/// Thumb and index tips closer than this pinch (world units).
const PINCH_DISTANCE: f32 = 0.02 / crate::METERS_PER_WORLD_UNIT;
/// Frames of the posing hand averaged into a capture, to steady tracking.
const CAPTURE_FRAMES: usize = 10;
/// Natural-log item scale change per second at full thumbstick.
const SCALE_RATE: f32 = 0.7;
/// The grip editor's item scale range.
const SCALE_RANGE: std::ops::RangeInclusive<f32> = 0.1..=3.0;

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
    pub overrides_path: PathBuf,
    recent: VecDeque<HandJoints>,
    pinched: bool,
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
            overrides_path,
            recent: VecDeque::new(),
            pinched: false,
        }
    }

    /// Take this frame's tracked hands; the other hand's pinch (on its
    /// leading edge) queues a capture of the posing hand's recent frames.
    pub fn observe(&mut self, tracked: [Option<HandJoints>; 2]) {
        match tracked[self.hand] {
            Some(joints) => {
                self.recent.push_back(joints);
                if self.recent.len() > CAPTURE_FRAMES {
                    self.recent.pop_front();
                }
            }
            None => self.recent.clear(),
        }
        let pinching = tracked[1 - self.hand].is_some_and(|joints| is_pinching(&joints));
        if pinching && !self.pinched && self.recent.len() == CAPTURE_FRAMES {
            self.capture = Some(mean_joints(&self.recent));
        }
        self.pinched = pinching;
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
}

fn is_pinching(joints: &HandJoints) -> bool {
    (joints[5] - joints[10]).magnitude() < PINCH_DISTANCE
}

fn mean_joints(frames: &VecDeque<HandJoints>) -> HandJoints {
    std::array::from_fn(|joint| {
        frames
            .iter()
            .map(|frame| frame[joint])
            .sum::<Vector3<f32>>()
            / frames.len() as f32
    })
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

    #[test]
    fn a_pinch_edge_captures_the_steady_posing_hand() {
        let entity = EntityId::dead();
        let pose = GripPose {
            position: vec3(0.0, 0.0, 0.0),
            rotation: Quaternion::from_angle_y(Deg(0.0)),
        };
        let mut tool = GripPoseTool::new(1, entity, pose, PathBuf::new());
        let open = Some(hand_with_tips(1.0));
        let pinch = Some(hand_with_tips(0.0));
        let posing = |x: f32| Some([vec3(x, 0.0, 0.0); HAND_JOINT_COUNT]);
        tool.observe([pinch, posing(0.0)]);
        assert!(tool.capture.is_none(), "too few posing frames to capture");
        for frame in 0..CAPTURE_FRAMES {
            tool.observe([open, posing(frame as f32)]);
        }
        tool.observe([pinch, posing(CAPTURE_FRAMES as f32)]);
        let captured = tool.capture.take().expect("pinch edge captures");
        // The mean of frames 1..=10.
        assert!((captured[0].x - 5.5).abs() < 1e-5);
        tool.observe([pinch, posing(0.0)]);
        assert!(tool.capture.is_none(), "a held pinch captures once");
        tool.observe([open, None]);
        tool.observe([pinch, posing(0.0)]);
        assert!(
            tool.capture.is_none(),
            "losing the posing hand restarts its window"
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
