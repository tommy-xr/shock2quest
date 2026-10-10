//! Opt-in, per-world aggregate timings. Rapier resets its counters each step.
use rapier3d::counters::Counters;
use std::time::Duration;

#[derive(Default)]
pub(super) struct PhysicsProfile {
    samples: usize,
    totals: [f64; 11],
}

impl PhysicsProfile {
    pub fn record(&mut self, counters: &Counters, wrapper: [Duration; 4]) {
        let values = [
            wrapper[0].as_secs_f64() * 1000.0,
            wrapper[1].as_secs_f64() * 1000.0,
            wrapper[2].as_secs_f64() * 1000.0,
            wrapper[3].as_secs_f64() * 1000.0,
            counters.stages.user_changes.time_ms(),
            counters.cd.broad_phase_time.time_ms(),
            counters.cd.narrow_phase_time.time_ms(),
            counters.stages.island_construction_time.time_ms(),
            counters.stages.solver_time.time_ms(),
            counters.stages.ccd_time.time_ms(),
            counters.stages.update_time.time_ms(),
        ];
        for (total, value) in self.totals.iter_mut().zip(values) {
            *total += value;
        }
        self.samples += 1;
        if self.samples == 600 {
            // Stage counters are nested inside step_ms, not additive to it.
            let names = [
                "prepare",
                "step",
                "controller",
                "events",
                "user_changes",
                "broad_phase",
                "narrow_phase",
                "islands",
                "solver",
                "ccd",
                "update",
            ];
            let means: serde_json::Map<String, serde_json::Value> = names
                .into_iter()
                .zip(self.totals)
                .map(|(name, total)| (format!("{name}_ms"), serde_json::json!(total / 600.0)))
                .collect();
            eprintln!(
                "SHOCK2QUEST_PHYSICS_PROFILE {}",
                serde_json::json!({"samples": 600, "mean": means})
            );
            *self = Self::default();
        }
    }
}
