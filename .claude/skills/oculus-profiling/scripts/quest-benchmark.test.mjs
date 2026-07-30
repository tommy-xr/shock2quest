import assert from "node:assert/strict";
import test from "node:test";

import {
  assertStableRefreshRate,
  parseArgs,
  parseEngineTelemetry,
  parseReadyInfo,
  parseVrApiTelemetry,
  renderMarkdown,
  summarize,
} from "./quest-benchmark.mjs";

test("summarize reports the distribution used in benchmark tables", () => {
  assert.deepEqual(summarize([3, 1, 2, 4]), {
    mean: 2.5,
    min: 1,
    p5: 1,
    p95: 4,
    max: 4,
  });
});

test("parses VrApi one-second telemetry buckets", () => {
  const telemetry = parseVrApiTelemetry(`
FPS=90,App=5.0,TW=2.0,CPU&GPU=6.0,GPU%=0.5,CPU%=0.4,Stale=0,Tear=0
FPS=89,App=6.0,TW=2.5,CPU&GPU=7.0,GPU%=0.6,CPU%=0.5,Stale=1,Tear=0
FPS=88,App=7.0,TW=3.0,CPU&GPU=8.0,GPU%=0.7,CPU%=0.6,Stale=0,Tear=1
`);

  assert.equal(telemetry.samples, 2);
  assert.equal(telemetry.fps.mean, 88.5);
  assert.equal(telemetry.app_ms.mean, 6.5);
  assert.equal(telemetry.stale_frames, 1);
  assert.equal(telemetry.torn_frames, 1);
});

test("uses one primary-display VrApi record per interval", () => {
  const telemetry = parseVrApiTelemetry(
    `
FPS=90,Fov=0D,App=5.0,CPU&GPU=6.0,Stale=0,Tear=0
FPS=10,Fov=0,App=50.0,CPU&GPU=60.0,Stale=9,Tear=9
FPS=89,Fov=0D,App=6.0,CPU&GPU=7.0,Stale=1,Tear=0
FPS=10,Fov=0,App=60.0,CPU&GPU=70.0,Stale=9,Tear=9
FPS=88,Fov=0D,App=7.0,CPU&GPU=8.0,Stale=0,Tear=1
FPS=10,Fov=0,App=70.0,CPU&GPU=80.0,Stale=9,Tear=9
`,
    2,
  );

  assert.equal(telemetry.samples, 2);
  assert.equal(telemetry.fps.mean, 88.5);
  assert.equal(telemetry.app_ms.mean, 6.5);
  assert.equal(telemetry.cpu_gpu_ms.mean, 7.5);
  assert.equal(telemetry.stale_frames, 1);
  assert.equal(telemetry.torn_frames, 1);
});

test("does not report missing compositor freshness fields as zero", () => {
  const telemetry = parseVrApiTelemetry(`
FPS=90,Fov=0D,App=5.0
FPS=90,Fov=0D,App=5.0
FPS=90,Fov=0D,App=5.0
`);

  assert.equal(telemetry.stale_frames, undefined);
  assert.equal(telemetry.torn_frames, undefined);
});

test("does not report partially observed freshness fields as zero", () => {
  const telemetry = parseVrApiTelemetry(
    `
FPS=90,Fov=0D,App=5.0,Stale=0,Tear=0
FPS=90,Fov=0D,App=5.0
FPS=90,Fov=0D,App=5.0,Stale=0,Tear=0
`,
    3,
  );

  assert.equal(telemetry.stale_frames, undefined);
  assert.equal(telemetry.torn_frames, undefined);
});

test("requires a whole number of one-second samples", () => {
  assert.throws(
    () =>
      parseArgs([
        "--mission",
        "earth.mis",
        "--seconds",
        "3.5",
      ]),
    /whole number of one-second samples/,
  );
});

test("parses Shock2Quest engine timing records", () => {
  const telemetry = parseEngineTelemetry(`
SHOCK2QUEST_PERF mission=earth.mis focused=false samples=40 skipped=2 fps=40 frame_ms=25 update_ms=8 scene_ms=8 left_eye_ms=8 right_eye_ms=8 finish_ms=2 submit_ms=1
SHOCK2QUEST_PERF mission=earth.mis focused=true samples=90 skipped=0 fps=90 frame_ms=11.1 update_ms=1.0 scene_ms=2.0 left_eye_ms=3.0 right_eye_ms=3.1 finish_ms=0.4 submit_ms=0.1
SHOCK2QUEST_PERF mission=earth.mis focused=true samples=89 skipped=1 fps=89 frame_ms=11.2 update_ms=1.2 scene_ms=2.2 left_eye_ms=3.2 right_eye_ms=3.3 finish_ms=0.6 submit_ms=0.2
`);

  assert.equal(telemetry.samples, 2);
  assert.equal(telemetry.focused_samples, 2);
  assert.equal(telemetry.unfocused_samples, 0);
  assert.equal(telemetry.skipped_frames, 1);
  assert.equal(telemetry.fps.mean, 89.5);
  assert.equal(telemetry.update_ms.mean, 1.1);
  assert.equal(telemetry.right_eye_ms.max, 3.3);
  assert.equal(telemetry.finish_ms.mean, 0.5);
});

test("parses requested and active display refresh rates", () => {
  const ready = parseReadyInfo(
    `SHOCK2QUEST_REFRESH_CHANGED from_hz=0 to_hz=90
SHOCK2QUEST_READY mission=earth.mis target_refresh_hz=90 requested_refresh_hz=90 refresh_hz=89.999 eye_width=1680 eye_height=1760
SHOCK2QUEST_REFRESH_CHANGED from_hz=90 to_hz=72`,
  );
  assert.deepEqual(
    ready,
    {
      target_refresh_hz: 90,
      requested_refresh_hz: 90,
      refresh_hz: 72,
      eye_width: 1680,
      eye_height: 1760,
      refresh_rate_changes: [{ from_hz: 90, to_hz: 72 }],
    },
  );
  assert.throws(
    () => assertStableRefreshRate(ready),
    /diverged from requested 90 Hz.*observed 72 Hz/,
  );
});

test("benchmark report exposes compositor freshness failures", () => {
  const report = renderMarkdown(
    [
      {
        status: "ok",
        mission: "earth.mis",
        startup: { game_init_ms: 1000, launch_to_focused_ms: 1500 },
        ready: { refresh_hz: 90 },
        engine: {
          update_ms: { mean: 1 },
          scene_ms: { mean: 2 },
          left_eye_ms: { mean: 3 },
          right_eye_ms: { mean: 4 },
          finish_ms: { mean: 0.5 },
        },
        vrapi: {
          fps: { mean: 90, min: 88 },
          stale_frames: 2,
          torn_frames: 1,
          app_ms: { mean: 5 },
          gpu_load: { mean: 0.5 },
        },
        memory: { total_pss_mib: 300 },
      },
    ],
    {
      model: "Quest",
      serial: "test",
      android: "14",
      apk_sha256: "abc",
      seconds: 5,
      warmup: 3,
    },
  );

  assert.match(report, /\| earth\.mis .* \| 90\/88 \| 2 \| 1 \|/);
  assert.match(report, /\| 7\.000 ms \| 0\.5 ms \|/);
  assert.match(report, /stale\/torn counts identify/);
});
