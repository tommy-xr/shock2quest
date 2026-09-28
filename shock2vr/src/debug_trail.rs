//! Player trail: a debug overlay of where the player's body has been over the
//! last few seconds, coloured by what the character controller was doing
//! (walking, climbing, topping out, airborne). Built to read ladder climbs -
//! including ones that leave the level - from a side view with the free
//! camera, and to let e2e tests dump or assert on the path
//! (`GET /v1/player/trail`).
//!
//! Debug instrumentation only: it lives on `MissionCore`, not in the ECS or
//! the save, so a level load starts it empty. Gated by the `player_trail`
//! dev param; turning it off clears it.

use std::collections::VecDeque;

use cgmath::{InnerSpace, Vector3, vec3};
use engine::scene::{RenderLayer, SceneObject, VertexPosition, color_material, lines_mesh};
use serde::Serialize;

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

#[derive(Default)]
pub struct DebugTrail {
    samples: VecDeque<TrailSample>,
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
    ) {
        if self
            .samples
            .back()
            .is_some_and(|last| (position - last.position).magnitude() > TELEPORT_DISTANCE)
        {
            self.clear();
        }
        self.elapsed += dt;
        self.samples.push_back(TrailSample {
            frame: self.next_frame,
            time: self.elapsed,
            position,
            motion,
            crouched,
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
    }

    pub fn clear(&mut self) {
        self.samples.clear();
        self.next_frame = 0;
        self.elapsed = 0.0;
    }

    pub fn samples(&self) -> impl Iterator<Item = &TrailSample> {
        self.samples.iter()
    }

    /// Line meshes for the trail, drawn over the world (depth cleared) so it
    /// shows through walls and outside the level. One mesh per colour; each
    /// segment takes the colour of the frame it ends on, darkened while
    /// crouched. Ticks mark every half second, bigger crosses a state change.
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
                    for axis in [Vector3::unit_x(), Vector3::unit_y(), Vector3::unit_z()] {
                        push(
                            color,
                            p - axis * TRANSITION_SIZE,
                            p + axis * TRANSITION_SIZE,
                        );
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

    /// One-second frames, so ages read as frame counts.
    fn record(trail: &mut DebugTrail, keep_seconds: f32, y: f32, motion: TrailMotion) {
        trail.record(1.0, keep_seconds, vec3(0.0, y, 0.0), motion, false);
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
    fn samples_serialize_with_their_state() {
        let mut trail = DebugTrail::default();
        trail.record(0.5, 8.0, vec3(1.0, 2.0, 3.0), TrailMotion::TopOut, true);
        let json = serde_json::to_value(trail.samples().collect::<Vec<_>>()).unwrap();
        assert_eq!(
            json,
            serde_json::json!([{"frame": 0, "time": 0.5, "pos": [1.0, 2.0, 3.0], "state": "top_out", "crouched": true}])
        );
    }
}
