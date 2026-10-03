//! Visual-only preparation of an empty glove for its available interaction.
//! Targets come from gameplay arbitration; no input, ownership or wrist motion changes.
use crate::hand_pose::{FingerAmounts, Pose};

#[derive(Clone, Copy, Debug, Default, serde::Serialize)]
pub struct HandAnticipation {
    curls: [f32; 5],
    point: f32,
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) enum Target {
    #[default]
    None,
    Point,
    Grab([f32; 5]),
}

impl HandAnticipation {
    pub(crate) fn update(&mut self, target: Target, dt: f32) {
        let (curls, point) = match target {
            Target::None => ([0.0; 5], 0.0),
            Target::Point => ([0.0; 5], 0.65),
            Target::Grab(curls) => (curls.map(|v| v.clamp(0.0, 1.0) * 0.55), 0.0),
        };
        // Exponential easing has the same response at 60, 90 and 120 Hz.
        // ~95% settled in 0.3 s; leaving or changing targets eases too.
        let alpha = 1.0 - (-10.0 * dt.max(0.0)).exp();
        for (current, target) in self.curls.iter_mut().zip(curls) {
            *current += (target - *current) * alpha;
        }
        self.point += (point - self.point) * alpha;
    }

    pub(crate) fn pose(
        &self,
        open: &Pose,
        point: &Pose,
        fist: &Pose,
        input: FingerAmounts,
    ) -> Pose {
        let [thumb, index, middle, ring, pinky] = self.curls;
        // Explicit analog input always wins: a full squeeze remains a full fist,
        // and a trigger pull still curls the extended index finger.
        open.blend(point, self.point).blend_per_finger(
            fist,
            &FingerAmounts {
                thumb: input.thumb.max(thumb),
                index: input.index.max(index),
                middle: input.middle.max(middle),
                ring: input.ring.max(ring),
                pinky: input.pinky.max(pinky),
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anticipation_eases_in_out_and_between_objects_at_any_frame_rate() {
        let mut results = Vec::new();
        for hz in [60, 90, 120] {
            let mut hand = HandAnticipation::default();
            hand.update(Target::Grab([1.0; 5]), 1.0 / hz as f32);
            assert!(hand.curls[0] > 0.0 && hand.curls[0] < 0.15);
            for _ in 1..hz / 2 {
                hand.update(Target::Grab([1.0; 5]), 1.0 / hz as f32);
            }
            assert!(hand.curls[0] > 0.54 && hand.curls[0] < 0.55);
            results.push(hand.curls[0]);
            hand.update(Target::Grab([0.0, 0.2, 0.4, 0.6, 0.8]), 1.0 / hz as f32);
            assert!(hand.curls[0] > 0.4 && hand.curls[0] < hand.curls[4]);
            for _ in 0..hz {
                hand.update(Target::None, 1.0 / hz as f32);
            }
            assert!(hand.curls.iter().all(|v| *v < 0.0001));
        }
        assert!((results[0] - results[2]).abs() < 1e-6);
    }

    #[test]
    fn explicit_squeeze_overrides_point_and_object_preview() {
        use crate::hand_pose::{fist_right_hand, open_right_hand, point_right_hand};
        use cgmath::InnerSpace;
        let fist = fist_right_hand();
        for target in [Target::Point, Target::Grab([0.2; 5])] {
            let mut hand = HandAnticipation::default();
            hand.update(target, 1.0);
            let pose = hand.pose(
                &open_right_hand(),
                &point_right_hand(),
                &fist,
                FingerAmounts {
                    thumb: 1.0,
                    index: 1.0,
                    middle: 1.0,
                    ring: 1.0,
                    pinky: 1.0,
                },
            );
            for i in 2..26 {
                assert!(pose.bone_rotations[i].dot(fist.bone_rotations[i]).abs() > 0.9999);
            }
        }
    }
}
