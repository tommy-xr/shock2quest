//! Player trail: a debug overlay of where the player's body has been over the
//! last few seconds, coloured by what the character controller was doing
//! (walking, climbing, topping out, airborne). Built to read ladder climbs -
//! including ones that leave the level - from a side view with the free
//! camera, and to let e2e tests dump or assert on the path
//! (`GET /v1/player/trail`). In VR it also records each hand's world path and
//! marks where a hand took or let go of a climb hold, and the hold a hand
//! vault pulled over.
//!
//! Debug instrumentation only: it lives on `MissionCore`, not in the ECS or
//! the save, so a level load starts it empty. Gated by the `player_trail`
//! dev param; turning it off clears it.

use std::collections::VecDeque;

use cgmath::{InnerSpace, Vector3, vec3};
use engine::scene::{RenderLayer, SceneObject, VertexPosition, color_material, lines_mesh};
use serde::Serialize;

use crate::{
    physics::{ClimbGrip, ClimbGripKind},
    vr_config::{Handedness, hand_slot},
};

/// What the character controller was doing on a recorded frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TrailMotion {
    /// On the ground.
    Supported,
    /// Gripping a ladder (or hand-climbing).
    Climbing,
    /// The scripted top-out / mantle transfer owns the body.
    TopOut,
    /// Jumping or falling.
    Airborne,
}

impl TrailMotion {
    pub fn classify(grounded: bool, climbing: bool, topping_out: bool) -> Self {
        if topping_out {
            Self::TopOut
        } else if climbing {
            Self::Climbing
        } else if grounded {
            Self::Supported
        } else {
            Self::Airborne
        }
    }

    fn color(self) -> Vector3<f32> {
        match self {
            Self::Supported => vec3(0.85, 0.85, 0.9),
            Self::Climbing => vec3(0.2, 1.0, 0.35),
            Self::TopOut => vec3(1.0, 0.25, 0.9),
            Self::Airborne => vec3(1.0, 0.55, 0.1),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct TrailSample {
    /// Recorded-frame counter since the trail was last cleared.
    pub frame: u64,
    /// Simulated seconds since the trail was last cleared.
    pub time: f32,
    /// The player position `/v1/player/position` reports.
    #[serde(rename = "pos", serialize_with = "serialize_vec3")]
    pub position: Vector3<f32>,
    #[serde(rename = "state")]
    pub motion: TrailMotion,
    pub crouched: bool,
    /// Each VR hand's world position (left, right); absent in flat.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hands: Option<TrailHandPositions>,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct TrailHandPositions {
    #[serde(serialize_with = "serialize_vec3")]
    pub left: Vector3<f32>,
    #[serde(serialize_with = "serialize_vec3")]
    pub right: Vector3<f32>,
}

/// One VR hand on a recorded frame: where it is, and the climb hold it is on.
#[derive(Clone, Copy, Debug)]
pub struct TrailHand {
    pub position: Vector3<f32>,
    pub hold: Option<TrailHold>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TrailHold {
    pub kind: ClimbGripKind,
    /// The held surface point.
    pub point: Vector3<f32>,
}

impl From<ClimbGrip> for TrailHold {
    fn from(grip: ClimbGrip) -> Self {
        Self {
            kind: grip.kind,
            point: grip.point,
        }
    }
}

/// Both VR hands on a recorded frame (left, right), and the hold a hand
/// vault started from this frame, if one did.
#[derive(Clone, Copy, Debug)]
pub struct TrailHands {
    pub hands: [TrailHand; 2],
    pub top_out: Option<(Handedness, TrailHold)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TrailEventKind {
    /// The hand took a hold.
    Grip,
    /// The hand let go of (or lost) a hold.
    Release,
    /// A hand vault started from this hold: the hold actually pulled over.
    TopOut,
}

/// A hand event, at the held surface point.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct TrailEvent {
    /// The sample frame it happened on.
    pub frame: u64,
    pub time: f32,
    #[serde(serialize_with = "serialize_hand")]
    pub hand: Handedness,
    pub kind: TrailEventKind,
    /// `ladder` or `ledge`.
    #[serde(serialize_with = "serialize_hold_kind")]
    pub hold: ClimbGripKind,
    #[serde(rename = "pos", serialize_with = "serialize_vec3")]
    pub point: Vector3<f32>,
}

fn serialize_hand<S: serde::Serializer>(hand: &Handedness, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_str(match hand {
        Handedness::Left => "left",
        Handedness::Right => "right",
    })
}

fn serialize_hold_kind<S: serde::Serializer>(
    kind: &ClimbGripKind,
    s: S,
) -> Result<S::Ok, S::Error> {
    s.serialize_str(match kind {
        ClimbGripKind::Ladder => "ladder",
        ClimbGripKind::Ledge => "ledge",
    })
}

fn serialize_vec3<S: serde::Serializer>(v: &Vector3<f32>, s: S) -> Result<S::Ok, S::Error> {
    [v.x, v.y, v.z].serialize(s)
}

/// A jump this far between consecutive frames is a teleport, not motion
/// (a fall at terminal speed covers ~0.2 a frame): the trail restarts there.
const TELEPORT_DISTANCE: f32 = 2.0;
/// A tick every half second of simulated time.
const TICK_SECONDS: f32 = 0.5;
/// Hard bound on the buffer whatever the frame rate (two minutes at 144 Hz).
const MAX_SAMPLES: usize = 120 * 144;
const TICK_SIZE: f32 = 0.06;
const TRANSITION_SIZE: f32 = 0.2;
const STROKE: f32 = 0.012;
const STROKE_OFFSETS: [[f32; 3]; 3] = [
    [0.0, 0.0, 0.0],
    [STROKE, STROKE, 0.0],
    [0.0, -STROKE, STROKE],
];
/// Hand path colours, indexed by [`hand_slot`]: left blue, right red.
const HAND_COLORS: [Vector3<f32>; 2] = [vec3(0.2, 0.6, 1.0), vec3(1.0, 0.2, 0.2)];
const HANDS: [Handedness; 2] = [Handedness::Left, Handedness::Right];
const GRIP_SIZE: f32 = 0.1;
const TOP_OUT_SIZE: f32 = 0.25;

/// The half-arms of a `+` marker along the world axes.
fn axis_cross(size: f32) -> [Vector3<f32>; 3] {
    [
        vec3(size, 0.0, 0.0),
        vec3(0.0, size, 0.0),
        vec3(0.0, 0.0, size),
    ]
}

#[derive(Default)]
pub struct DebugTrail {
    samples: VecDeque<TrailSample>,
    /// Hand events, oldest first, trimmed with the samples.
    events: VecDeque<TrailEvent>,
    /// Each hand's hold on the last recorded frame, to spot grips and releases.
    holds: [Option<TrailHold>; 2],
    next_frame: u64,
    /// Simulated seconds since the trail was last cleared.
    elapsed: f32,
}

impl DebugTrail {
    /// Record one simulated frame of `dt` seconds, keeping the last
    /// `keep_seconds` of samples (oldest drop first).
    pub fn record(
        &mut self,
        dt: f32,
        keep_seconds: f32,
        position: Vector3<f32>,
        motion: TrailMotion,
        crouched: bool,
        hands: Option<TrailHands>,
    ) {
        if self
            .samples
            .back()
            .is_some_and(|last| (position - last.position).magnitude() > TELEPORT_DISTANCE)
        {
            self.clear();
        }
        self.elapsed += dt;
        let (frame, time) = (self.next_frame, self.elapsed);
        self.record_hand_events(frame, time, hands);
        self.samples.push_back(TrailSample {
            frame,
            time,
            position,
            motion,
            crouched,
            hands: hands.map(|hands| TrailHandPositions {
                left: hands.hands[0].position,
                right: hands.hands[1].position,
            }),
        });
        self.next_frame += 1;
        while self.samples.len() > MAX_SAMPLES
            || self
                .samples
                .front()
                .is_some_and(|oldest| oldest.time < self.elapsed - keep_seconds)
        {
            self.samples.pop_front();
        }
        let oldest = self.samples.front().map_or(0, |oldest| oldest.frame);
        while self
            .events
            .front()
            .is_some_and(|event| event.frame < oldest)
        {
            self.events.pop_front();
        }
    }

    /// Compare each hand's hold with last frame's: a hold that went away is a
    /// release, a new one a grip (both at the held point), then the vault.
    fn record_hand_events(&mut self, frame: u64, time: f32, hands: Option<TrailHands>) {
        let holds = hands.map_or([None, None], |hands| hands.hands.map(|hand| hand.hold));
        let mut push = |hand: Handedness, kind: TrailEventKind, hold: TrailHold| {
            self.events.push_back(TrailEvent {
                frame,
                time,
                hand,
                kind,
                hold: hold.kind,
                point: hold.point,
            });
        };
        for (index, hand) in HANDS.into_iter().enumerate() {
            let (before, now) = (self.holds[index], holds[index]);
            if before == now {
                continue;
            }
            if let Some(before) = before {
                push(hand, TrailEventKind::Release, before);
            }
            if let Some(now) = now {
                push(hand, TrailEventKind::Grip, now);
            }
        }
        if let Some((hand, hold)) = hands.and_then(|hands| hands.top_out) {
            push(hand, TrailEventKind::TopOut, hold);
        }
        self.holds = holds;
    }

    pub fn clear(&mut self) {
        self.samples.clear();
        self.events.clear();
        self.holds = [None, None];
        self.next_frame = 0;
        self.elapsed = 0.0;
    }

    pub fn samples(&self) -> impl Iterator<Item = &TrailSample> {
        self.samples.iter()
    }

    pub fn events(&self) -> impl Iterator<Item = &TrailEvent> {
        self.events.iter()
    }

    /// Line meshes for the trail, drawn over the world (depth cleared) so it
    /// shows through walls and outside the level. One mesh per colour; each
    /// segment takes the colour of the frame it ends on, darkened while
    /// crouched. Ticks mark every half second, bigger crosses a state change.
    /// VR hand paths are thin lines in their hand's colour, with a `+` where
    /// the hand gripped, an `x` where it let go, and a diamond on the hold a
    /// vault pulled over.
    pub fn render(&self) -> Vec<SceneObject> {
        let mut by_color: Vec<(Vector3<f32>, Vec<VertexPosition>)> = Vec::new();
        let mut push = |color: Vector3<f32>, a: Vector3<f32>, b: Vector3<f32>| {
            let lines = match by_color.iter_mut().find(|(c, _)| *c == color) {
                Some((_, lines)) => lines,
                None => {
                    by_color.push((color, Vec::new()));
                    &mut by_color.last_mut().unwrap().1
                }
            };
            lines.push(VertexPosition { position: a });
            lines.push(VertexPosition { position: b });
        };
        let mut previous: Option<&TrailSample> = None;
        for sample in &self.samples {
            let color = sample.motion.color() * if sample.crouched { 0.6 } else { 1.0 };
            let p = sample.position;
            if let Some(prev) = previous {
                // GL lines are one pixel wide; parallel copies make a
                // readable stroke from any side.
                for offset in STROKE_OFFSETS {
                    let offset = Vector3::from(offset);
                    push(color, prev.position + offset, p + offset);
                }
                if prev.motion != sample.motion {
                    for arm in axis_cross(TRANSITION_SIZE) {
                        push(color, p - arm, p + arm);
                    }
                }
            }
            let tick = |s: &TrailSample| (s.time / TICK_SECONDS) as u64;
            if previous.is_none_or(|prev| tick(prev) != tick(sample)) {
                push(
                    color,
                    p - Vector3::unit_x() * TICK_SIZE,
                    p + Vector3::unit_x() * TICK_SIZE,
                );
                push(
                    color,
                    p - Vector3::unit_z() * TICK_SIZE,
                    p + Vector3::unit_z() * TICK_SIZE,
                );
            }
            previous = Some(sample);
        }
        for (a, b) in self.samples.iter().zip(self.samples.iter().skip(1)) {
            if let (Some(a), Some(b)) = (a.hands, b.hands) {
                push(HAND_COLORS[0], a.left, b.left);
                push(HAND_COLORS[1], a.right, b.right);
            }
        }
        for event in &self.events {
            let (color, p) = (HAND_COLORS[hand_slot(event.hand)], event.point);
            match event.kind {
                TrailEventKind::Grip => {
                    for arm in axis_cross(GRIP_SIZE) {
                        push(color, p - arm, p + arm);
                    }
                }
                TrailEventKind::Release => {
                    let s = GRIP_SIZE;
                    for arm in [
                        vec3(s, s, 0.0),
                        vec3(s, -s, 0.0),
                        vec3(0.0, s, s),
                        vec3(0.0, s, -s),
                    ] {
                        push(color, p - arm, p + arm);
                    }
                }
                TrailEventKind::TopOut => {
                    // Octahedron outline: each equator point to its neighbour
                    // and to both poles.
                    let color = TrailMotion::TopOut.color();
                    let equator = [
                        Vector3::unit_x(),
                        Vector3::unit_z(),
                        -Vector3::unit_x(),
                        -Vector3::unit_z(),
                    ]
                    .map(|v| p + v * TOP_OUT_SIZE);
                    for (i, e) in equator.iter().enumerate() {
                        push(color, *e, equator[(i + 1) % 4]);
                        push(color, *e, p + Vector3::unit_y() * TOP_OUT_SIZE);
                        push(color, *e, p - Vector3::unit_y() * TOP_OUT_SIZE);
                    }
                }
            }
        }
        by_color
            .into_iter()
            .map(|(color, lines)| {
                let mut object = SceneObject::new(
                    color_material::create(color),
                    Box::new(lines_mesh::create(lines)),
                );
                object.set_render_layer(RenderLayer::SceneOverlay);
                object
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cgmath::Zero;

    /// One-second frames, so ages read as frame counts.
    fn record(trail: &mut DebugTrail, keep_seconds: f32, y: f32, motion: TrailMotion) {
        trail.record(1.0, keep_seconds, vec3(0.0, y, 0.0), motion, false, None);
    }

    fn ladder(y: f32) -> TrailHold {
        TrailHold {
            kind: ClimbGripKind::Ladder,
            point: vec3(-7.0, y, 0.0),
        }
    }

    /// One climbing frame with the hands on `holds` (left, right).
    fn record_hands(
        trail: &mut DebugTrail,
        holds: [Option<TrailHold>; 2],
        top_out: Option<(Handedness, TrailHold)>,
    ) {
        let hand = |hold: Option<TrailHold>, x: f32| TrailHand {
            position: vec3(x, 1.0, 0.0),
            hold,
        };
        let hands = TrailHands {
            hands: [hand(holds[0], -0.25), hand(holds[1], 0.25)],
            top_out,
        };
        let (position, motion) = (vec3(0.0, 0.0, 0.0), TrailMotion::Climbing);
        trail.record(1.0, 60.0, position, motion, false, Some(hands));
    }

    fn event_list(trail: &DebugTrail) -> Vec<(u64, Handedness, TrailEventKind, f32)> {
        trail
            .events()
            .map(|e| (e.frame, e.hand, e.kind, e.point.y))
            .collect()
    }

    #[test]
    fn classify_prefers_top_out_then_climb_then_ground() {
        use TrailMotion::*;
        assert_eq!(TrailMotion::classify(true, true, true), TopOut);
        assert_eq!(TrailMotion::classify(true, true, false), Climbing);
        assert_eq!(TrailMotion::classify(true, false, false), Supported);
        assert_eq!(TrailMotion::classify(false, false, false), Airborne);
    }

    #[test]
    fn the_buffer_keeps_only_the_last_seconds() {
        let mut trail = DebugTrail::default();
        for i in 0..10 {
            record(&mut trail, 3.5, i as f32 * 0.1, TrailMotion::Climbing);
        }
        let frames: Vec<u64> = trail.samples().map(|s| s.frame).collect();
        assert_eq!(frames, vec![6, 7, 8, 9]);
    }

    #[test]
    fn clearing_restarts_the_frame_count_and_clock() {
        let mut trail = DebugTrail::default();
        record(&mut trail, 8.0, 0.0, TrailMotion::Supported);
        trail.clear();
        assert_eq!(trail.samples().count(), 0);
        record(&mut trail, 8.0, 0.0, TrailMotion::Supported);
        let first = trail.samples().next().unwrap();
        assert_eq!((first.frame, first.time), (0, 1.0));
    }

    #[test]
    fn a_teleport_sized_jump_restarts_the_trail() {
        let mut trail = DebugTrail::default();
        record(&mut trail, 8.0, 0.0, TrailMotion::Supported);
        record(&mut trail, 8.0, 0.1, TrailMotion::Supported);
        record(&mut trail, 8.0, 5.0, TrailMotion::Supported);
        let ys: Vec<f32> = trail.samples().map(|s| s.position.y).collect();
        assert_eq!(ys, vec![5.0]);
    }

    #[test]
    fn hand_over_hand_records_each_grip_and_release_at_its_hold() {
        use Handedness::*;
        use TrailEventKind::*;
        let mut trail = DebugTrail::default();
        record_hands(&mut trail, [None, None], None);
        record_hands(&mut trail, [Some(ladder(2.0)), None], None);
        record_hands(&mut trail, [Some(ladder(2.0)), None], None);
        // The right hand closes on the next rung before the left lets go.
        record_hands(&mut trail, [Some(ladder(2.0)), Some(ladder(2.8))], None);
        record_hands(&mut trail, [None, Some(ladder(2.8))], None);
        record_hands(&mut trail, [Some(ladder(3.6)), Some(ladder(2.8))], None);
        assert_eq!(
            event_list(&trail),
            vec![
                (1, Left, Grip, 2.0),
                (3, Right, Grip, 2.8),
                (4, Left, Release, 2.0),
                (5, Left, Grip, 3.6),
            ]
        );
    }

    #[test]
    fn a_vault_marks_the_hold_it_pulled_over_after_the_hands_let_go() {
        use Handedness::*;
        use TrailEventKind::*;
        let lip = TrailHold {
            kind: ClimbGripKind::Ledge,
            point: vec3(-7.3, 4.0, 0.0),
        };
        let mut trail = DebugTrail::default();
        record_hands(&mut trail, [Some(ladder(3.6)), Some(lip)], None);
        record_hands(&mut trail, [None, None], Some((Right, lip)));
        assert_eq!(
            event_list(&trail)[2..],
            [
                (1, Left, Release, 3.6),
                (1, Right, Release, 4.0),
                (1, Right, TopOut, 4.0)
            ]
        );
        let top_out = trail.events().last().unwrap();
        assert_eq!(
            (top_out.hold, top_out.point),
            (ClimbGripKind::Ledge, lip.point)
        );
    }

    #[test]
    fn hand_events_age_out_with_their_samples_and_clear_with_the_trail() {
        let mut trail = DebugTrail::default();
        record_hands(&mut trail, [Some(ladder(2.0)), None], None);
        for _ in 0..3 {
            trail.record(
                1.0,
                2.5,
                Vector3::zero(),
                TrailMotion::Climbing,
                false,
                None,
            );
        }
        assert_eq!(trail.samples().next().unwrap().frame, 1);
        let kinds: Vec<_> = trail.events().map(|e| e.kind).collect();
        assert_eq!(
            kinds,
            vec![TrailEventKind::Release],
            "the grip aged out with frame 0"
        );
        trail.record(
            1.0,
            2.5,
            Vector3::zero(),
            TrailMotion::Climbing,
            false,
            None,
        );
        assert_eq!(trail.events().count(), 0);

        record_hands(&mut trail, [Some(ladder(2.0)), None], None);
        trail.clear();
        record_hands(&mut trail, [Some(ladder(2.0)), None], None);
        let kinds: Vec<_> = trail.events().map(|e| e.kind).collect();
        assert_eq!(
            kinds,
            vec![TrailEventKind::Grip],
            "a cleared trail forgets the old holds"
        );
    }

    #[test]
    fn vr_samples_and_events_serialize_their_hands() {
        let mut trail = DebugTrail::default();
        record_hands(&mut trail, [None, Some(ladder(2.0))], None);
        let sample = serde_json::to_value(trail.samples().next().unwrap()).unwrap();
        assert_eq!(
            sample["hands"],
            serde_json::json!({"left": [-0.25, 1.0, 0.0], "right": [0.25, 1.0, 0.0]})
        );
        let event = serde_json::to_value(trail.events().next().unwrap()).unwrap();
        assert_eq!(
            event,
            serde_json::json!({"frame": 0, "time": 1.0, "hand": "right", "kind": "grip", "hold": "ladder", "pos": [-7.0, 2.0, 0.0]})
        );
    }

    #[test]
    fn samples_serialize_with_their_state() {
        let mut trail = DebugTrail::default();
        trail.record(
            0.5,
            8.0,
            vec3(1.0, 2.0, 3.0),
            TrailMotion::TopOut,
            true,
            None,
        );
        let json = serde_json::to_value(trail.samples().collect::<Vec<_>>()).unwrap();
        assert_eq!(
            json,
            serde_json::json!([{"frame": 0, "time": 0.5, "pos": [1.0, 2.0, 3.0], "state": "top_out", "crouched": true}])
        );
    }
}
