#!/usr/bin/env node

import { spawn } from "node:child_process";
import {
  mkdirSync,
  writeFileSync,
} from "node:fs";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";

import {
  PACKAGE,
  adb,
  assertRuntimeFocused,
  captureFocusedFrame,
  launchMission,
  missionSelection,
  resolveSerial,
  restoreMissionSelection,
  restoreProximityAutomation,
  stopApp,
  validateMission,
} from "../../vr-device-loop/scripts/quest-device.mjs";

const delay = (milliseconds) =>
  new Promise((resolveDelay) => setTimeout(resolveDelay, milliseconds));

export function parseArgs(argv) {
  const options = {
    missions: [],
    all: false,
    warmup: 5,
    seconds: 10,
    timeout: 120,
    capture: true,
    keepAwake: false,
    output: resolve(
      "/tmp",
      `shock2quest-quest-benchmark-${new Date().toISOString().replaceAll(":", "-")}`,
    ),
  };

  for (let index = 0; index < argv.length; index += 1) {
    const argument = argv[index];
    if (argument === "--all") options.all = true;
    else if (argument === "--no-capture") options.capture = false;
    else if (argument === "--keep-awake") options.keepAwake = true;
    else {
      const value = argv[++index];
      if (value === undefined) throw new Error(`${argument} requires a value`);
      if (argument === "--mission") options.missions.push(validateMission(value));
      else if (argument === "--warmup") options.warmup = Number(value);
      else if (argument === "--seconds") options.seconds = Number(value);
      else if (argument === "--timeout") options.timeout = Number(value);
      else if (argument === "--output") options.output = resolve(value);
      else if (argument === "--serial") options.serial = value;
      else throw new Error(`unknown argument: ${argument}`);
    }
  }

  if (options.all && options.missions.length > 0) {
    throw new Error("use either --all or --mission, not both");
  }
  if (!options.all && options.missions.length === 0) {
    throw new Error("pass --all or at least one --mission");
  }
  for (const [name, value, minimum] of [
    ["warmup", options.warmup, 0],
    ["seconds", options.seconds, 3],
    ["timeout", options.timeout, 1],
  ]) {
    if (!Number.isFinite(value) || value < minimum) {
      throw new Error(`--${name} must be at least ${minimum}`);
    }
  }
  if (!Number.isInteger(options.seconds)) {
    throw new Error("--seconds must be a whole number of one-second samples");
  }
  return options;
}

function escapeRegularExpression(value) {
  return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

function numericField(line, name) {
  const escapedName = escapeRegularExpression(name);
  const match = line.match(
    new RegExp(`(?:^|[ ,])${escapedName}=(-?[0-9.]+)`),
  );
  return match ? Number(match[1]) : undefined;
}

export function summarize(values) {
  if (values.length === 0) return undefined;
  const sorted = values.toSorted((left, right) => left - right);
  const total = values.reduce((sum, value) => sum + value, 0);
  return {
    mean: Number((total / values.length).toFixed(3)),
    min: sorted[0],
    p5: sorted[Math.floor((sorted.length - 1) * 0.05)],
    p95: sorted[Math.ceil(sorted.length * 0.95) - 1],
    max: sorted.at(-1),
  };
}

function valuesFor(lines, name) {
  return lines
    .map((line) => numericField(line, name))
    .filter((value) => value !== undefined);
}

function sumObserved(lines, name) {
  const values = valuesFor(lines, name);
  return values.length !== lines.length
    ? undefined
    : values.reduce((sum, value) => sum + value, 0);
}

export function parseVrApiTelemetry(text, expectedSamples) {
  const allLines = text.split("\n").filter((line) => line.includes("FPS="));
  // Current Horizon OS emits a primary-display (`Fov=0D`) record and a second
  // `Fov=0` record for each interval. Treating both as independent samples
  // double-counts the window and mixes distinct CPU&GPU values. Prefer the
  // primary-display series when present, while retaining compatibility with
  // OS versions that emit only one record class.
  const primaryDisplayLines = allLines.filter((line) =>
    line.includes("Fov=0D"),
  );
  const sampleLines =
    primaryDisplayLines.length > 0 ? primaryDisplayLines : allLines;
  const lines =
    expectedSamples === undefined
      ? sampleLines.length > 2
        ? sampleLines.slice(1)
        : sampleLines
      : sampleLines.slice(-expectedSamples);
  if (lines.length === 0) return undefined;
  return {
    samples: lines.length,
    fps: summarize(valuesFor(lines, "FPS")),
    app_ms: summarize(valuesFor(lines, "App")),
    compositor_ms: summarize(valuesFor(lines, "TW")),
    cpu_gpu_ms: summarize(valuesFor(lines, "CPU&GPU")),
    gpu_load: summarize(valuesFor(lines, "GPU%")),
    cpu_load: summarize(valuesFor(lines, "CPU%")),
    temperature_c: summarize(valuesFor(lines, "Temp")),
    stale_frames: sumObserved(lines, "Stale"),
    torn_frames: sumObserved(lines, "Tear"),
  };
}

export function parseEngineTelemetry(text, expectedSamples) {
  const allLines = text
    .split("\n")
    .filter((line) => line.includes("SHOCK2QUEST_PERF"));
  const lines =
    expectedSamples === undefined
      ? allLines.length > 2
        ? allLines.slice(1)
        : allLines
      : allLines.slice(-expectedSamples);
  if (lines.length === 0) return undefined;
  return {
    samples: lines.length,
    focused_samples: lines.filter((line) => line.includes("focused=true"))
      .length,
    unfocused_samples: lines.filter((line) => line.includes("focused=false"))
      .length,
    skipped_frames: valuesFor(lines, "skipped").reduce(
      (sum, value) => sum + value,
      0,
    ),
    fps: summarize(valuesFor(lines, "fps")),
    frame_ms: summarize(valuesFor(lines, "frame_ms")),
    update_ms: summarize(valuesFor(lines, "update_ms")),
    scene_ms: summarize(valuesFor(lines, "scene_ms")),
    left_eye_ms: summarize(valuesFor(lines, "left_eye_ms")),
    right_eye_ms: summarize(valuesFor(lines, "right_eye_ms")),
    submit_ms: summarize(valuesFor(lines, "submit_ms")),
  };
}

function parseMemory(text) {
  const number = (pattern) => {
    const match = text.match(pattern);
    return match ? Number(match[1]) : undefined;
  };
  const mebibytes = (kilobytes) =>
    kilobytes === undefined
      ? undefined
      : Number((kilobytes / 1024).toFixed(1));
  return {
    total_pss_mib: mebibytes(number(/TOTAL PSS:\s+(\d+)/)),
    total_rss_mib: mebibytes(number(/TOTAL RSS:\s+(\d+)/)),
    graphics_pss_mib: mebibytes(number(/Graphics:\s+(\d+)/)),
  };
}

function markerField(text, marker, name) {
  const line = text.split("\n").find((entry) => entry.includes(marker));
  return line ? numericField(line, name) : undefined;
}

function readyInfo(text) {
  const line = text
    .split("\n")
    .find((entry) => entry.includes("SHOCK2QUEST_READY"));
  if (!line) return undefined;
  return {
    refresh_hz: numericField(line, "refresh_hz"),
    eye_width: numericField(line, "eye_width"),
    eye_height: numericField(line, "eye_height"),
  };
}

async function collectTelemetry(serial, seconds) {
  const child = spawn(
    "adb",
    [
      "-s",
      serial,
      "logcat",
      "-v",
      "raw",
      "VrApi:I",
      "RustStdoutStderr:V",
      "*:S",
    ],
    { stdio: ["ignore", "pipe", "pipe"] },
  );
  let output = "";
  let errors = "";
  child.stdout.setEncoding("utf8");
  child.stderr.setEncoding("utf8");
  child.stdout.on("data", (chunk) => {
    output += chunk;
  });
  child.stderr.on("data", (chunk) => {
    errors += chunk;
  });

  // The first one-second record may straddle the sampling boundary. Collect an
  // extra second and let both parsers discard that first bucket.
  await delay((seconds + 1) * 1_000);
  child.kill("SIGTERM");
  await Promise.race([
    new Promise((resolveClose) => child.once("close", resolveClose)),
    delay(2_000),
  ]);
  if (errors) output += `\nADB_LOGCAT_STDERR ${errors}`;
  return output;
}

function installedMissions(serial) {
  return adb(serial, ["shell", "ls", "/sdcard/shock2quest"])
    .split("\n")
    .map((entry) => entry.trim())
    .filter((entry) => entry.endsWith(".mis"))
    .sort();
}

async function benchmarkMission(serial, mission, options, device) {
  const launched = await launchMission(
    serial,
    mission,
    options.timeout * 1_000,
  );
  await delay(options.warmup * 1_000);
  assertRuntimeFocused(serial, mission);
  const rawTelemetry = await collectTelemetry(serial, options.seconds);
  const slug = mission.replace(/[^A-Za-z0-9_-]/g, "_");
  writeFileSync(
    resolve(options.output, `${slug}.telemetry.log`),
    rawTelemetry,
  );
  const engineTelemetry = parseEngineTelemetry(rawTelemetry, options.seconds);
  const compositorTelemetry = parseVrApiTelemetry(
    rawTelemetry,
    options.seconds,
  );
  if (!engineTelemetry || engineTelemetry.samples !== options.seconds) {
    throw new Error(
      `expected ${options.seconds} advancing SHOCK2QUEST_PERF samples, got ${engineTelemetry?.samples ?? 0}`,
    );
  }
  if (!compositorTelemetry || compositorTelemetry.samples !== options.seconds) {
    throw new Error(
      `expected ${options.seconds} primary-display VrApi samples, got ${compositorTelemetry?.samples ?? 0}`,
    );
  }
  if (
    engineTelemetry.focused_samples !== engineTelemetry.samples ||
    engineTelemetry.unfocused_samples > 0
  ) {
    throw new Error(
      `XR session was not focused for the full interval (${engineTelemetry.focused_samples}/${engineTelemetry.samples} focused samples)`,
    );
  }
  assertRuntimeFocused(serial, mission);
  const runtimeLogs = adb(serial, [
    "logcat",
    "-d",
    "-v",
    "raw",
    "RustStdoutStderr:V",
    "*:S",
  ]);
  let screenshot;
  let visual_error;
  if (options.capture) {
    try {
      screenshot = captureFocusedFrame(
        serial,
        resolve(options.output, `${slug}.png`),
        mission,
      );
    } catch (error) {
      visual_error = error instanceof Error ? error.message : String(error);
    }
  }
  const result = {
    status: visual_error ? "visual_failed" : "ok",
    mission,
    device,
    sample: {
      warmup_seconds: options.warmup,
      duration_seconds: options.seconds,
    },
    startup: {
      game_init_ms: markerField(
        launched.logs,
        "SHOCK2QUEST_STARTUP",
        "init_ms",
      ),
      launch_to_focused_ms: launched.launchMilliseconds,
    },
    ready: readyInfo(launched.logs),
    engine: engineTelemetry,
    vrapi: compositorTelemetry,
    memory: parseMemory(
      adb(serial, ["shell", "dumpsys", "meminfo", PACKAGE]),
    ),
    screenshot,
    visual_error,
  };
  writeFileSync(resolve(options.output, `${slug}.log`), runtimeLogs);
  writeFileSync(
    resolve(options.output, `${slug}.json`),
    `${JSON.stringify(result, null, 2)}\n`,
  );
  return result;
}

function metricMean(metric, suffix = "") {
  return metric?.mean === undefined ? "n/a" : `${metric.mean}${suffix}`;
}

export function renderMarkdown(results, metadata) {
  const lines = [
    "# Quest mission baseline",
    "",
    `- Device: ${metadata.model} (${metadata.serial})`,
    `- Android: ${metadata.android}`,
    `- APK: release, ${PACKAGE}`,
    `- APK SHA-256: ${metadata.apk_sha256}`,
    `- Sample: ${metadata.seconds}s after ${metadata.warmup}s warmup per mission`,
    "",
    "| Mission | Init | Focused | FPS mean/min | Stale | Torn | App | Update | Scene | Eyes | GPU load | PSS | Visual |",
    "| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- |",
  ];
  for (const result of results) {
    if (result.status === "failed") {
      lines.push(
        `| ${result.mission} | n/a | n/a | failed | n/a | n/a | n/a | n/a | n/a | n/a | n/a | n/a | ${result.error} |`,
      );
      continue;
    }
    const eyes =
      result.engine?.left_eye_ms && result.engine?.right_eye_ms
        ? `${(
            result.engine.left_eye_ms.mean + result.engine.right_eye_ms.mean
          ).toFixed(3)} ms`
        : "n/a";
    const visual =
      result.status === "visual_failed"
        ? `failed: ${result.visual_error}`
        : result.screenshot
          ? `[PNG](${result.screenshot.replaceAll(" ", "%20")})`
          : "not captured";
    const fps = result.vrapi?.fps ?? result.engine?.fps;
    const fpsSummary = fps
      ? `${fps.mean}/${fps.min}`
      : "n/a";
    lines.push(
      `| ${result.mission} | ${result.startup.game_init_ms?.toFixed(1) ?? "n/a"} ms | ${result.startup.launch_to_focused_ms} ms | ${fpsSummary} | ${result.vrapi?.stale_frames ?? "n/a"} | ${result.vrapi?.torn_frames ?? "n/a"} | ${metricMean(result.vrapi?.app_ms, " ms")} | ${metricMean(result.engine?.update_ms, " ms")} | ${metricMean(result.engine?.scene_ms, " ms")} | ${eyes} | ${metricMean(result.vrapi?.gpu_load)} | ${result.memory.total_pss_mib ?? "n/a"} MiB | ${visual} |`,
    );
  }
  lines.push("");
  lines.push(
    "Engine timings are one-second in-app means. VrApi values use the primary-display (`Fov=0D`) one-second record when Horizon OS emits paired records. VrApi FPS is compositor presentation rate; stale/torn counts identify intervals that were not fresh application frames. Visual paths have passed an automated non-black check only and still require human review. `n/a` means the current OS did not emit that field.",
  );
  return `${lines.join("\n")}\n`;
}

async function main(argv) {
  const options = parseArgs(argv);
  const serial = resolveSerial(options.serial);
  const packageInfo = adb(serial, ["shell", "dumpsys", "package", PACKAGE]);
  if (/\bDEBUGGABLE\b/.test(packageInfo)) {
    throw new Error(
      "installed APK is debuggable; install a cargo apk --release build",
    );
  }

  mkdirSync(options.output, { recursive: true });
  const missions = options.all ? installedMissions(serial) : options.missions;
  const previousMissionSelection = missionSelection(serial);
  const apkPath = adb(serial, ["shell", "pm", "path", PACKAGE]).replace(
    /^package:/,
    "",
  );
  const device = {
    serial,
    model: adb(serial, ["shell", "getprop", "ro.product.model"]),
    android: adb(serial, ["shell", "getprop", "ro.build.version.release"]),
    apk_sha256: adb(serial, ["shell", "sha256sum", apkPath]).split(/\s+/)[0],
  };
  const results = [];
  try {
    for (const mission of missions) {
      process.stdout.write(`Benchmarking ${mission}...\n`);
      try {
        const result = await benchmarkMission(
          serial,
          mission,
          options,
          device,
        );
        results.push(result);
        process.stdout.write(
          `  ${metricMean(result.vrapi?.fps ?? result.engine?.fps)} FPS, ${metricMean(result.engine?.update_ms, " ms")} update, ${result.screenshot ?? `visual failed: ${result.visual_error}`}\n`,
        );
      } catch (error) {
        const failure = {
          status: "failed",
          mission,
          error: error instanceof Error ? error.message : String(error),
        };
        results.push(failure);
        process.stderr.write(`  failed: ${failure.error}\n`);
      }
    }
  } finally {
    try {
      restoreMissionSelection(serial, previousMissionSelection);
    } finally {
      if (!options.keepAwake) {
        try {
          stopApp(serial);
        } finally {
          restoreProximityAutomation(serial);
        }
      }
    }
  }

  const metadata = {
    ...device,
    warmup: options.warmup,
    seconds: options.seconds,
  };
  const report = renderMarkdown(results, metadata);
  writeFileSync(
    resolve(options.output, "results.json"),
    `${JSON.stringify({ metadata, results }, null, 2)}\n`,
  );
  writeFileSync(resolve(options.output, "report.md"), report);
  process.stdout.write(`\n${report}\nArtifacts: ${options.output}\n`);
}

const isMain =
  process.argv[1] &&
  import.meta.url === pathToFileURL(resolve(process.argv[1])).href;
if (isMain) {
  main(process.argv.slice(2)).catch((error) => {
    process.stderr.write(
      `quest-benchmark: ${error instanceof Error ? error.message : error}\n`,
    );
    process.exitCode = 1;
  });
}
