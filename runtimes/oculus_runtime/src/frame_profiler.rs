use std::time::Duration;

#[derive(Clone, Copy, Debug, Default)]
pub struct FrameTimings {
    pub frame: Duration,
    pub update: Duration,
    pub scene: Duration,
    pub left_eye: Duration,
    pub right_eye: Duration,
    pub finish: Duration,
    pub submit: Duration,
}

impl FrameTimings {
    #[cfg(test)]
    fn from_frame_duration(frame: Duration) -> Self {
        Self {
            frame,
            ..Self::default()
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FrameReport {
    pub frames: u64,
    pub skipped_frames: u64,
    pub fps: f64,
    pub frame_ms: f64,
    pub update_ms: f64,
    pub scene_ms: f64,
    pub left_eye_ms: f64,
    pub right_eye_ms: f64,
    pub finish_ms: f64,
    pub submit_ms: f64,
}

#[derive(Debug)]
pub struct FrameProfiler {
    report_interval: Duration,
    elapsed: Duration,
    frames: u64,
    skipped_frames: u64,
    update: Duration,
    scene: Duration,
    left_eye: Duration,
    right_eye: Duration,
    finish: Duration,
    submit: Duration,
}

impl FrameProfiler {
    pub fn new(report_interval: Duration) -> Self {
        assert!(!report_interval.is_zero());
        Self {
            report_interval,
            elapsed: Duration::ZERO,
            frames: 0,
            skipped_frames: 0,
            update: Duration::ZERO,
            scene: Duration::ZERO,
            left_eye: Duration::ZERO,
            right_eye: Duration::ZERO,
            finish: Duration::ZERO,
            submit: Duration::ZERO,
        }
    }

    pub fn record(&mut self, timings: FrameTimings) -> Option<FrameReport> {
        self.frames += 1;
        self.record_sample(timings)
    }

    pub fn record_skipped(&mut self, frame: Duration, update: Duration) -> Option<FrameReport> {
        self.skipped_frames += 1;
        self.record_sample(FrameTimings {
            frame,
            update,
            ..FrameTimings::default()
        })
    }

    pub fn reset(&mut self) {
        self.elapsed = Duration::ZERO;
        self.frames = 0;
        self.skipped_frames = 0;
        self.update = Duration::ZERO;
        self.scene = Duration::ZERO;
        self.left_eye = Duration::ZERO;
        self.right_eye = Duration::ZERO;
        self.finish = Duration::ZERO;
        self.submit = Duration::ZERO;
    }

    fn record_sample(&mut self, timings: FrameTimings) -> Option<FrameReport> {
        self.elapsed += timings.frame;
        self.update += timings.update;
        self.scene += timings.scene;
        self.left_eye += timings.left_eye;
        self.right_eye += timings.right_eye;
        self.finish += timings.finish;
        self.submit += timings.submit;

        if self.elapsed < self.report_interval {
            return None;
        }

        let elapsed_seconds = self.elapsed.as_secs_f64();
        let frames = self.frames;
        let attempts = frames + self.skipped_frames;
        let rendered_divisor = frames.max(1) as f64;
        let update_divisor = attempts as f64;
        let report = FrameReport {
            frames,
            skipped_frames: self.skipped_frames,
            fps: frames as f64 / elapsed_seconds,
            frame_ms: self.elapsed.as_secs_f64() * 1_000.0 / rendered_divisor,
            update_ms: self.update.as_secs_f64() * 1_000.0 / update_divisor,
            scene_ms: self.scene.as_secs_f64() * 1_000.0 / rendered_divisor,
            left_eye_ms: self.left_eye.as_secs_f64() * 1_000.0 / rendered_divisor,
            right_eye_ms: self.right_eye.as_secs_f64() * 1_000.0 / rendered_divisor,
            finish_ms: self.finish.as_secs_f64() * 1_000.0 / rendered_divisor,
            submit_ms: self.submit.as_secs_f64() * 1_000.0 / rendered_divisor,
        };

        self.reset();

        Some(report)
    }
}

#[cfg(test)]
mod tests {
    use super::{FrameProfiler, FrameTimings};
    use std::time::Duration;

    #[test]
    fn reports_mean_timings_when_the_window_elapses() {
        let mut profiler = FrameProfiler::new(Duration::from_millis(20));
        let timings = FrameTimings {
            frame: Duration::from_millis(10),
            update: Duration::from_millis(1),
            scene: Duration::from_millis(2),
            left_eye: Duration::from_millis(3),
            right_eye: Duration::from_millis(3),
            finish: Duration::from_millis(1),
            submit: Duration::from_millis(1),
        };

        assert!(profiler.record(timings).is_none());
        let report = profiler.record(timings).expect("window should report");

        assert_eq!(report.frames, 2);
        assert_eq!(report.skipped_frames, 0);
        assert_eq!(report.fps, 100.0);
        assert_eq!(report.frame_ms, 10.0);
        assert_eq!(report.update_ms, 1.0);
        assert_eq!(report.scene_ms, 2.0);
        assert_eq!(report.left_eye_ms, 3.0);
        assert_eq!(report.right_eye_ms, 3.0);
        assert_eq!(report.finish_ms, 1.0);
        assert_eq!(report.submit_ms, 1.0);
    }

    #[test]
    fn resets_after_emitting_a_report() {
        let mut profiler = FrameProfiler::new(Duration::from_millis(10));
        let first = FrameTimings::from_frame_duration(Duration::from_millis(10));
        let second = FrameTimings::from_frame_duration(Duration::from_millis(20));

        assert!(profiler.record(first).is_some());
        let report = profiler
            .record(second)
            .expect("second window should report");

        assert_eq!(report.frames, 1);
        assert_eq!(report.fps, 50.0);
        assert_eq!(report.frame_ms, 20.0);
    }

    #[test]
    fn skipped_frames_reduce_rendered_fps_and_are_reported() {
        let mut profiler = FrameProfiler::new(Duration::from_millis(30));
        let rendered = FrameTimings {
            frame: Duration::from_millis(10),
            update: Duration::from_millis(2),
            ..FrameTimings::default()
        };

        assert!(profiler.record(rendered).is_none());
        assert!(
            profiler
                .record_skipped(Duration::from_millis(10), Duration::from_millis(1))
                .is_none()
        );
        let report = profiler.record(rendered).expect("window should report");

        assert_eq!(report.frames, 2);
        assert_eq!(report.skipped_frames, 1);
        assert!((report.fps - 66.666_666).abs() < 0.001);
        assert_eq!(report.frame_ms, 15.0);
        assert!((report.update_ms - 1.666_666).abs() < 0.001);
    }

    #[test]
    fn reset_discards_an_incomplete_window() {
        let mut profiler = FrameProfiler::new(Duration::from_millis(20));
        let timing = FrameTimings::from_frame_duration(Duration::from_millis(10));

        assert!(profiler.record(timing).is_none());
        profiler.reset();
        assert!(profiler.record(timing).is_none());
    }
}
