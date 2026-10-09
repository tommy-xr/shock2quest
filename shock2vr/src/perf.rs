//! Opt-in, render-thread aggregate diagnostics. No logging in hot paths.
//! Phase timings are inclusive: nested phases must not be summed together.
use std::{
    cell::RefCell,
    time::{Duration, Instant},
};

#[derive(Clone, Copy)]
#[repr(usize)]
pub enum Phase {
    GameUpdate,
    MissionUpdate,
    Physics,
    Solver,
    Animation,
    Hitboxes,
    Interaction,
    Scripts,
    Effects,
    Visibility,
    PhysicsSync,
    Audio,
}
const NAMES: [&str; 12] = [
    "game_update",
    "mission_update",
    "physics",
    "solver",
    "animation",
    "hitboxes",
    "interaction",
    "scripts",
    "effects",
    "visibility",
    "physics_sync",
    "audio",
];

#[derive(Clone, Copy, Default)]
struct Aggregate {
    calls: u64,
    total: Duration,
    max: Duration,
}
impl Aggregate {
    fn record(&mut self, elapsed: Duration) {
        self.calls += 1;
        self.total += elapsed;
        self.max = self.max.max(elapsed);
    }
}

struct State {
    enabled: bool,
    phases: [Aggregate; 12],
    scene_elapsed: Duration,
    active_elapsed: Duration,
    scene_updates: u64,
    physics_steps: u64,
    physics_seconds: f64,
    solver_dt: Option<f32>,
    cpu_frames: Aggregate,
    over_budget: u64,
    // 100 us bins, including one overflow bucket at index 500 (>= 50 ms).
    cpu_histogram: [u64; 501],
}
impl Default for State {
    fn default() -> Self {
        Self {
            enabled: false,
            phases: [Aggregate::default(); 12],
            scene_elapsed: Duration::ZERO,
            active_elapsed: Duration::ZERO,
            scene_updates: 0,
            physics_steps: 0,
            physics_seconds: 0.0,
            solver_dt: None,
            cpu_frames: Aggregate::default(),
            over_budget: 0,
            cpu_histogram: [0; 501],
        }
    }
}
thread_local! { static STATE: RefCell<State> = RefCell::new(State::default()); }

pub fn set_enabled(enabled: bool) {
    STATE.with(|state| {
        *state.borrow_mut() = State {
            enabled,
            ..State::default()
        }
    });
}

pub struct Scope(Option<(Phase, Instant)>);
pub fn scope(phase: Phase) -> Scope {
    Scope(STATE.with(|state| state.borrow().enabled.then(|| (phase, Instant::now()))))
}
impl Drop for Scope {
    fn drop(&mut self) {
        if let Some((phase, start)) = self.0 {
            let elapsed = start.elapsed();
            STATE.with(|state| state.borrow_mut().phases[phase as usize].record(elapsed));
        }
    }
}

/// Active render elapsed time, recorded once per unpaused frame, independently
/// of how many scene/solver ticks are due.
pub fn active_elapsed(elapsed: Duration) {
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        if state.enabled {
            state.active_elapsed += elapsed;
        }
    });
}

pub fn scene_update(elapsed: Duration) {
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        if state.enabled {
            state.scene_updates += 1;
            state.scene_elapsed += elapsed;
        }
    });
}

pub fn physics_step(dt: f32) {
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        if state.enabled {
            state.physics_steps += 1;
            state.physics_seconds += f64::from(dt);
            state.solver_dt = Some(dt);
        }
    });
}

/// CPU wall time after wait_frame through end_frame, including synchronization
/// inside that interval. This is not pure CPU execution time or GPU duration.
pub fn cpu_frame(elapsed: Duration) {
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        if !state.enabled {
            return;
        }
        state.cpu_frames.record(elapsed);
        state.over_budget += u64::from(elapsed > Duration::from_nanos(8_333_333));
        let bin = (elapsed.as_micros() / 100).min(500) as usize;
        state.cpu_histogram[bin] += 1;
    });
}

/// Drain outside measured work once per runtime's timing window. Histograms
/// can be merged across windows; percentiles of per-window means cannot.
pub fn take_report() -> Option<serde_json::Value> {
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        if !state.enabled { return None; }
        let sample = std::mem::replace(&mut *state, State { enabled: true, ..State::default() });
        let phases: serde_json::Map<String, serde_json::Value> = NAMES.iter().zip(sample.phases)
            .filter(|(_, aggregate)| aggregate.calls > 0)
            .map(|(name, aggregate)| ((*name).into(), serde_json::json!({
                "calls": aggregate.calls, "total_ms": aggregate.total.as_secs_f64() * 1000.0,
                "mean_ms": aggregate.total.as_secs_f64() * 1000.0 / aggregate.calls as f64,
                "max_ms": aggregate.max.as_secs_f64() * 1000.0,
            }))).collect();
        let histogram: Vec<_> = sample.cpu_histogram.into_iter().enumerate()
            .filter(|(_, count)| *count > 0).collect();
        Some(serde_json::json!({"phases": phases, "scene_updates": sample.scene_updates,
            "active_elapsed_s": sample.active_elapsed.as_secs_f64(),
            "scene_elapsed_s": sample.scene_elapsed.as_secs_f64(), "physics_steps": sample.physics_steps,
            "physics_elapsed_s": sample.physics_seconds, "solver_dt_s": sample.solver_dt,
            "cpu_frames": sample.cpu_frames.calls, "cpu_total_ms": sample.cpu_frames.total.as_secs_f64() * 1000.0,
            "cpu_max_ms": sample.cpu_frames.max.as_secs_f64() * 1000.0,
            "cpu_over_8_33ms": sample.over_budget, "cpu_histogram_100us": histogram,
            "cpu_histogram_overflow_bin": 500}))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn aggregates_and_drains_clock_and_frame_evidence_without_changing_time() {
        set_enabled(true);
        for _ in 0..90 {
            scene_update(Duration::from_secs_f64(1.0 / 90.0));
            physics_step(1.0 / 60.0);
        }
        for micros in [7500, 8333, 8334, 60000] {
            cpu_frame(Duration::from_micros(micros));
        }
        let sample = take_report().unwrap();
        assert_eq!(sample["physics_steps"], 90);
        assert!((sample["scene_elapsed_s"].as_f64().unwrap() - 1.0).abs() < 1e-6);
        assert!((sample["physics_elapsed_s"].as_f64().unwrap() - 1.5).abs() < 1e-6);
        assert_eq!(sample["cpu_frames"], 4);
        assert_eq!(sample["cpu_over_8_33ms"], 2);
        assert_eq!(
            sample["cpu_histogram_100us"],
            serde_json::json!([[75, 1], [83, 2], [500, 1]])
        );
        assert_eq!(take_report().unwrap()["physics_steps"], 0);
        set_enabled(false);
        physics_step(1.0);
        cpu_frame(Duration::from_secs(1));
        assert!(take_report().is_none());
    }
    #[test]
    fn clock_audit_counts_actual_solver_steps_under_synthetic_render_cadences() {
        use crate::physics::{CollisionGroup, DynamicPhysicsOptions, PhysicsShape, PhysicsWorld};
        use cgmath::{Quaternion, vec3};
        use shipyard::EntityId;
        for hz in [60, 72, 90, 120] {
            set_enabled(true);
            let mut physics = PhysicsWorld::new();
            let entity = EntityId::from_inner(10).unwrap();
            let mut player =
                physics.create_player(vec3(0.0, 1.0, 0.0), EntityId::from_inner(11).unwrap());
            let body = physics.add_dynamic(
                entity,
                vec3(10.0, 100.0, 0.0),
                Quaternion::new(1.0, 0.0, 0.0, 0.0),
                vec3(0.0, 0.0, 0.0),
                PhysicsShape::Sphere(0.1),
                CollisionGroup::entity(),
                false,
                DynamicPhysicsOptions::default(),
            );
            for frame in 0..hz {
                // Exact one-second input partition, avoiding accumulated rounding.
                let end = Duration::from_secs_f64((frame + 1) as f64 / hz as f64);
                let start = Duration::from_secs_f64(frame as f64 / hz as f64);
                scene_update(end - start);
                physics.update(vec3(0.0, 0.0, 0.0), &mut player);
            }
            let sample = take_report().unwrap();
            let simulated = sample["physics_elapsed_s"].as_f64().unwrap();
            let velocity = physics.get_velocity(entity).unwrap().y;
            assert_eq!(sample["scene_elapsed_s"], 1.0);
            assert_eq!(sample["physics_steps"], hz);
            assert!((f64::from(velocity) + 9.81 * simulated).abs() < 0.01);
            println!(
                "CLOCK_AUDIT render_hz={hz} wall_s=1 solver_s={simulated:.6} velocity_y={velocity:.6} position_y={:.6}",
                physics.get_position(body).unwrap().y
            );
            set_enabled(false);
        }
    }

    #[test]
    fn phase_totals_keep_call_count_and_max_instead_of_averaging_averages() {
        let mut aggregate = Aggregate::default();
        aggregate.record(Duration::from_millis(2));
        aggregate.record(Duration::from_millis(4));
        assert_eq!(aggregate.calls, 2);
        assert_eq!(aggregate.total, Duration::from_millis(6));
        assert_eq!(aggregate.max, Duration::from_millis(4));
    }
}
