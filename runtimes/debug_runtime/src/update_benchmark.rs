//! One-shot host simulation benchmark. Asset loading still needs a GL context;
//! measured frames call only Game::update, never any render entry point.
use std::{
    num::NonZeroU32,
    path::PathBuf,
    time::{Duration, Instant},
};

use anyhow::{Context, ensure};
use serde_json::{Value, json};
use shock2vr::{
    Game, benchmark_scene::BenchmarkRun, input::InputActionState, input_context::InputContext,
    perf, time::Time,
};

#[derive(clap::Args)]
pub struct Options {
    /// Run N full simulation updates without rendering, write JSON, and exit.
    /// Forces the existing opt-in 60 Hz clock and CPU phase instrumentation.
    #[arg(long, requires = "benchmark_output")]
    pub benchmark_updates: Option<NonZeroU32>,
    /// Warmup updates, excluded from measurements (at least three seconds).
    #[arg(long, default_value = "600", value_parser = clap::value_parser!(u32).range(180..))]
    benchmark_warmup: u32,
    /// Result path; required for the one-shot update benchmark.
    #[arg(long, requires = "benchmark_updates")]
    benchmark_output: Option<PathBuf>,
    /// Run without 60 Hz wall-clock pacing. Async AI gets different scheduling;
    /// use only as a throughput microbenchmark, not a gameplay/FPS comparison.
    #[arg(long, requires = "benchmark_updates")]
    benchmark_unpaced: bool,
}

// Exact rational boundaries match FixedStepClock: rounded 1/60 durations would
// occasionally yield zero/two updates and contaminate the latency distribution.
fn tick_time(tick: u64) -> Time {
    let boundary = |n: u64| Duration::from_nanos((u128::from(n) * 1_000_000_000 / 60) as u64);
    Time {
        total: boundary(tick),
        elapsed: boundary(tick) - boundary(tick - 1),
    }
}

fn distribution(samples: &[Duration]) -> Value {
    let mut ms: Vec<_> = samples.iter().map(|d| d.as_secs_f64() * 1000.0).collect();
    ms.sort_by(f64::total_cmp);
    let percentile = |p: usize| ms[(ms.len() * p).div_ceil(100) - 1];
    json!({"samples": ms.len(), "mean_ms": ms.iter().sum::<f64>() / ms.len() as f64,
        "p50_ms": percentile(50), "p95_ms": percentile(95), "p99_ms": percentile(99),
        "max_ms": ms.last(), "over_8_33ms": ms.iter().filter(|&&t| t > 1000.0 / 120.0).count()})
}

/// Fixture observations without scene generation. Mesh/lighting counts require
/// rendering and are deliberately absent; animations and lamp state remain useful.
fn observation(game: &Game, fixture: &Option<BenchmarkRun>) -> Value {
    let Some(fixture) = fixture else {
        return Value::Null;
    };
    let mut value = fixture.observation(game, &[]);
    for key in [
        "subject_meshes",
        "expected_subject_meshes",
        "lit_subject_meshes",
        "scene_objects",
        "additional_subjects",
    ] {
        value.as_object_mut().unwrap().remove(key);
    }
    value
}

pub fn run(
    game: &mut Game,
    fixture: &mut Option<BenchmarkRun>,
    input: &InputContext,
    options: &Options,
    mission: &str,
    features: &[String],
    vr: bool,
) -> anyhow::Result<()> {
    let count = options.benchmark_updates.unwrap().get();
    let output = options
        .benchmark_output
        .as_ref()
        .context("--benchmark-updates requires --benchmark-output")?;
    ensure!(
        game.debug_scene().is_some(),
        "update benchmark requires a mission or debug scene"
    );
    let mut actions = InputActionState::new();
    let mut samples = Vec::with_capacity(count as usize);
    let mut before = Value::Null;
    let mut measured_start = Instant::now();
    let mut deadline = Instant::now();
    let total = u64::from(options.benchmark_warmup) + u64::from(count);
    for tick in 1..=total {
        let measured = tick > u64::from(options.benchmark_warmup);
        if tick == u64::from(options.benchmark_warmup) + 1 {
            before = observation(game, fixture);
            ensure!(
                fixture.is_none() || before["setup_complete"] == true,
                "fixture setup incomplete after warmup"
            );
            // Drop all warmup timings, including initialization/lazy loading.
            perf::take_report();
            measured_start = Instant::now();
            deadline = measured_start;
        }
        let time = tick_time(tick);
        let start = Instant::now();
        game.update(&time, input, &mut actions);
        let elapsed = start.elapsed();
        if measured {
            samples.push(elapsed);
        }
        if let Some(fixture) = fixture {
            fixture.advance_setup(game, time.elapsed);
        }
        ensure!(
            !game.has_pending_transition(),
            "benchmark crossed a level transition; select a stationary workload"
        );
        // Pace outside measured work. On an overrun, resume from now instead
        // of a burst that starves the asynchronous pathfinding worker.
        if !options.benchmark_unpaced {
            deadline += time.elapsed;
            let now = Instant::now();
            if let Some(remaining) = deadline.checked_duration_since(now) {
                std::thread::sleep(remaining);
            } else {
                deadline = now;
            }
        }
    }
    let wall = measured_start.elapsed();
    let profile = perf::take_report().context("CPU profiling unexpectedly disabled")?;
    ensure!(
        profile["scene_updates"] == u64::from(count),
        "expected one scene update per benchmark tick: {profile}"
    );
    ensure!(
        profile["physics_steps"] == u64::from(count),
        "expected one unchanged solver step per update: {profile}"
    );
    ensure!(
        profile["phases"].get("visibility").is_none(),
        "render work entered the measured interval"
    );
    let report = json!({"schema_version": 1, "kind": "host_update_only", "mission": mission,
        "presentation": if vr { "Vr" } else { "Flat" }, "experimental_features": features,
        "debug_assertions": cfg!(debug_assertions), "arch": std::env::consts::ARCH, "os": std::env::consts::OS,
        "paced": !options.benchmark_unpaced, "simulation_hz": 60, "warmup_updates": options.benchmark_warmup,
        "measured_wall_s": wall.as_secs_f64(), "update": distribution(&samples), "profile": profile,
        "fixture": fixture.as_ref().map(|f| &f.scene), "before": before, "after": observation(game, fixture),
        "update_samples_ms": samples.iter().map(|d| d.as_secs_f64() * 1000.0).collect::<Vec<_>>()});
    std::fs::write(output, serde_json::to_vec_pretty(&report)?)
        .with_context(|| format!("writing {}", output.display()))?;
    println!(
        "SHOCK2QUEST_UPDATE_BENCHMARK output={} update={}",
        output.display(),
        report["update"]
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cli_rejects_missing_output_zero_samples_and_short_warmup() {
        use clap::Parser;
        for args in [
            vec!["dbgr", "--benchmark-updates", "60"],
            vec![
                "dbgr",
                "--benchmark-updates",
                "0",
                "--benchmark-output",
                "out.json",
            ],
            vec![
                "dbgr",
                "--benchmark-updates",
                "60",
                "--benchmark-output",
                "out.json",
                "--benchmark-warmup",
                "1",
            ],
            vec!["dbgr", "--benchmark-unpaced"],
        ] {
            assert!(crate::Args::try_parse_from(args).is_err());
        }
        assert!(
            crate::Args::try_parse_from([
                "dbgr",
                "--benchmark-updates",
                "60",
                "--benchmark-output",
                "out.json"
            ])
            .is_ok()
        );
        assert!(crate::Args::try_parse_from(["dbgr", "--mission", "medsci1.mis"]).is_ok());
    }
    #[test]
    fn exact_tick_boundaries_produce_one_scene_step_each() {
        let mut clock = shock2vr::fixed_step::FixedStepClock::new(
            NonZeroU32::new(60).unwrap(),
            NonZeroU32::new(8).unwrap(),
        );
        for tick in 1..=6000 {
            let time = tick_time(tick);
            assert_eq!(clock.advance(time.elapsed).count(), 1);
        }
        assert_eq!(tick_time(6000).total, Duration::from_secs(100));
    }
    #[test]
    fn percentiles_use_individual_updates_and_nearest_rank() {
        let samples: Vec<_> = (1..=100).rev().map(Duration::from_millis).collect();
        let stats = distribution(&samples);
        assert_eq!(stats["p50_ms"], 50.0);
        assert_eq!(stats["p95_ms"], 95.0);
        assert_eq!(stats["p99_ms"], 99.0);
        assert_eq!(stats["over_8_33ms"], 92);
    }
}
