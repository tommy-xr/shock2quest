#!/usr/bin/env node

import { execFileSync } from "node:child_process";
import { mkdtempSync, rmSync, unlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, resolve } from "node:path";
import { pathToFileURL } from "node:url";

export const PACKAGE = "com.tommybuilds.shock2quest";
export const ACTIVITY = `${PACKAGE}/android.app.NativeActivity`;
export const MISSION_CONFIG = "/sdcard/shock2quest/vr-mission.txt";

const delay = (milliseconds) =>
  new Promise((resolveDelay) => setTimeout(resolveDelay, milliseconds));

export function adb(serial, args, options = {}) {
  const output = execFileSync("adb", ["-s", serial, ...args], {
    encoding: "utf8",
    maxBuffer: 64 * 1024 * 1024,
    stdio: ["ignore", "pipe", "pipe"],
    ...options,
  });
  return typeof output === "string" ? output.trim() : output;
}

export function resolveSerial(explicitSerial) {
  const lines = execFileSync("adb", ["devices", "-l"], {
    encoding: "utf8",
  })
    .split("\n")
    .slice(1)
    .map((line) => line.trim())
    .filter(Boolean);
  const devices = lines
    .filter((line) => /\sdevice(?:\s|$)/.test(line))
    .map((line) => line.split(/\s+/)[0]);

  if (explicitSerial) {
    if (!devices.includes(explicitSerial)) {
      throw new Error(
        `requested device ${explicitSerial} is not attached and authorized`,
      );
    }
    return explicitSerial;
  }
  if (devices.length !== 1) {
    throw new Error(
      `expected exactly one attached device, found ${devices.length}; pass --serial`,
    );
  }
  return devices[0];
}

export function validateMission(mission) {
  const trimmed = mission.trim();
  const validCharacters = /^[A-Za-z0-9_.-]+$/.test(trimmed);
  const validName =
    trimmed.startsWith("debug_") ||
    trimmed.endsWith(".mis") ||
    trimmed === "main_menu";
  if (!trimmed || !validCharacters || !validName) {
    throw new Error(`invalid mission name: ${JSON.stringify(mission)}`);
  }
  return trimmed;
}

export function selectMission(serial, mission) {
  const selected = validateMission(mission);
  writeMissionSelection(serial, `${selected}\n`);
  return selected;
}

function writeMissionSelection(serial, contents) {
  const temporaryDirectory = mkdtempSync(
    resolve(tmpdir(), "shock2quest-mission-"),
  );
  const localPath = resolve(temporaryDirectory, "vr-mission.txt");
  try {
    writeFileSync(localPath, contents);
    adb(serial, ["shell", "mkdir", "-p", dirname(MISSION_CONFIG)]);
    adb(serial, ["push", localPath, MISSION_CONFIG]);
  } finally {
    rmSync(temporaryDirectory, { recursive: true, force: true });
  }
}

export function missionSelection(serial) {
  try {
    return {
      exists: true,
      contents: adb(serial, ["exec-out", "cat", MISSION_CONFIG], {
        encoding: null,
      }),
    };
  } catch {
    return { exists: false, contents: Buffer.alloc(0) };
  }
}

export function restoreMissionSelection(serial, selection) {
  if (selection.exists) {
    writeMissionSelection(serial, selection.contents);
  } else {
    adb(serial, ["shell", "rm", "-f", MISSION_CONFIG]);
  }
}

export function wakeDevice(serial) {
  adb(serial, ["shell", "setprop", "debug.oculus.guardian_pause", "1"]);
  adb(serial, ["shell", "input", "keyevent", "KEYCODE_WAKEUP"]);
  adb(serial, [
    "shell",
    "am",
    "broadcast",
    "-a",
    "com.oculus.vrpowermanager.prox_close",
  ]);
}

export function restoreProximityAutomation(serial) {
  adb(serial, ["shell", "setprop", "debug.oculus.guardian_pause", "0"]);
  adb(serial, [
    "shell",
    "am",
    "broadcast",
    "-a",
    "com.oculus.vrpowermanager.automation_disable",
  ]);
}

export function runtimeLogs(serial) {
  return adb(serial, [
    "logcat",
    "-d",
    "-v",
    "raw",
    "RustStdoutStderr:V",
    "ActivityLaunchInterceptorController:I",
    "AndroidRuntime:E",
    "DEBUG:E",
    "*:S",
  ]);
}

export function assertRuntimeFocused(serial, mission) {
  const processId = adb(serial, ["shell", "pidof", PACKAGE]);
  if (!processId) throw new Error("runtime process is not running");

  const activities = adb(serial, ["shell", "dumpsys", "activity", "activities"]);
  const resumed = activities
    .split("\n")
    .find((line) => /topResumedActivity|mResumedActivity/.test(line));
  if (!resumed) {
    throw new Error("could not find Android's top resumed activity");
  }
  if (!resumed.includes(PACKAGE)) {
    throw new Error(`runtime is not the top resumed activity: ${resumed.trim()}`);
  }

  const logs = runtimeLogs(serial);
  const states = logs
    .split("\n")
    .filter((line) =>
      line.includes(`SHOCK2QUEST_XR_STATE mission=${mission} state=`),
    );
  const latestState = states.at(-1);
  if (!latestState) {
    throw new Error(`no XR session state found for ${mission}`);
  }
  if (!latestState.endsWith("state=FOCUSED")) {
    throw new Error(`runtime XR session is not focused: ${latestState}`);
  }

  const performance = logs
    .split("\n")
    .filter((line) =>
      line.includes(`SHOCK2QUEST_PERF mission=${mission} `),
    );
  const latestPerformance = performance.at(-1);
  if (!latestPerformance) {
    throw new Error(`no advancing performance sample found for ${mission}`);
  }
  if (!latestPerformance.includes("focused=true")) {
    throw new Error(`runtime is not rendering while focused: ${latestPerformance}`);
  }

  return {
    processId,
    resumed,
    latestState,
    performanceSamples: performance.length,
  };
}

export async function waitForMarker(
  serial,
  marker,
  timeoutMilliseconds = 120_000,
) {
  const deadline = Date.now() + timeoutMilliseconds;
  let logs = "";
  while (Date.now() < deadline) {
    logs = runtimeLogs(serial);
    if (logs.includes(marker)) return logs;
    if (logs.includes("RequiresControllersLaunchInterceptor")) {
      throw new Error(
        "Horizon OS blocked launch for sleeping controllers; verify optional oculus.software.handtracking in the installed manifest",
      );
    }

    let pid = "";
    try {
      pid = adb(serial, ["shell", "pidof", PACKAGE]);
    } catch {
      // pidof exits non-zero until Android has started the process.
    }
    if (!pid && logs.includes("FATAL EXCEPTION")) {
      break;
    }
    await delay(250);
  }
  const tail = logs.split("\n").slice(-30).join("\n");
  throw new Error(`timed out waiting for ${marker}\n${tail}`);
}

export async function launchMission(
  serial,
  mission,
  timeoutMilliseconds = 120_000,
) {
  const selected = selectMission(serial, mission);
  wakeDevice(serial);
  try {
    adb(serial, ["logcat", "-c"]);
    adb(serial, ["shell", "am", "force-stop", PACKAGE]);
    const launchStarted = Date.now();
    adb(serial, ["shell", "am", "start", "-S", "-n", ACTIVITY]);
    const firstFrameLogs = await waitForMarker(
      serial,
      `SHOCK2QUEST_READY mission=${selected}`,
      timeoutMilliseconds,
    );
    const focusedLogs = await waitForMarker(
      serial,
      `SHOCK2QUEST_XR_STATE mission=${selected} state=FOCUSED`,
      timeoutMilliseconds,
    );
    const launchMilliseconds = Date.now() - launchStarted;
    const performanceLogs = await waitForMarker(
      serial,
      `SHOCK2QUEST_PERF mission=${selected} focused=true`,
      timeoutMilliseconds,
    );
    assertRuntimeFocused(serial, selected);
    return {
      mission: selected,
      launchMilliseconds,
      logs: `${firstFrameLogs}\n${focusedLogs}\n${performanceLogs}`,
    };
  } catch (error) {
    try {
      stopApp(serial);
    } finally {
      restoreProximityAutomation(serial);
    }
    throw error;
  }
}

export function stopApp(serial) {
  adb(serial, ["shell", "am", "force-stop", PACKAGE]);
}

export function captureScreenshot(
  serial,
  outputPath,
  { focusedMission } = {},
) {
  if (focusedMission) assertRuntimeFocused(serial, focusedMission);
  const absolutePath = resolve(outputPath);
  const bytes = adb(serial, ["exec-out", "screencap", "-p"], {
    encoding: null,
  });
  writeFileSync(absolutePath, bytes);
  if (focusedMission) {
    assertRuntimeFocused(serial, focusedMission);
    validateScreenshot(absolutePath);
  }
  return absolutePath;
}

export function captureFocusedFrame(serial, outputPath, mission) {
  const absolutePath = resolve(outputPath);
  try {
    return captureScreenshot(serial, absolutePath, {
      focusedMission: mission,
    });
  } catch (screenshotError) {
    assertRuntimeFocused(serial, mission);
    return captureFocusedFrameFromRecording(
      serial,
      absolutePath,
      mission,
      screenshotError,
    );
  }
}

function captureFocusedFrameFromRecording(
  serial,
  absolutePath,
  mission,
  screenshotError,
) {
  const focusedBefore = assertRuntimeFocused(serial, mission);
  const temporaryDirectory = mkdtempSync(
    resolve(tmpdir(), "shock2quest-focused-capture-"),
  );
  const videoPath = resolve(temporaryDirectory, "capture.mp4");
  try {
    recordVideo(serial, videoPath, 2);
    execFileSync(
      "ffmpeg",
      [
        "-y",
        "-v",
        "error",
        "-ss",
        "1",
        "-i",
        videoPath,
        "-frames:v",
        "1",
        absolutePath,
      ],
      { stdio: ["ignore", "ignore", "pipe"] },
    );
    const focusedAfter = assertRuntimeFocused(serial, mission);
    if (focusedAfter.performanceSamples <= focusedBefore.performanceSamples) {
      throw new Error("performance samples did not advance during recording");
    }
    validateScreenshot(absolutePath);
    return absolutePath;
  } catch (recordingError) {
    throw new Error(
      `focused screencap failed (${
        screenshotError instanceof Error ? screenshotError.message : screenshotError
      }); recording fallback failed (${
        recordingError instanceof Error ? recordingError.message : recordingError
      })`,
    );
  } finally {
    rmSync(temporaryDirectory, { recursive: true, force: true });
  }
}

function validateScreenshot(path) {
  try {
    const pixels = execFileSync(
      "ffmpeg",
      [
        "-v",
        "error",
        "-i",
        path,
        "-vf",
        "scale=64:32",
        "-f",
        "rawvideo",
        "-pix_fmt",
        "rgb24",
        "pipe:1",
      ],
      { encoding: null, maxBuffer: 1024 * 1024 },
    );
    let sum = 0;
    let sumSquares = 0;
    for (const value of pixels) {
      sum += value;
      sumSquares += value * value;
    }
    const mean = sum / pixels.length;
    const variance = sumSquares / pixels.length - mean * mean;
    if (mean < 1 && variance < 1) {
      unlinkSync(path);
      throw new Error("captured compositor frame is effectively black");
    }
  } catch (error) {
    if (error instanceof Error && error.message.includes("effectively black")) {
      throw error;
    }
    throw new Error(
      `could not validate captured compositor frame with ffmpeg: ${
        error instanceof Error ? error.message : error
      }`,
    );
  }
}

export function recordVideo(serial, outputPath, seconds = 8) {
  if (!Number.isInteger(seconds) || seconds < 1 || seconds > 180) {
    throw new Error("recording duration must be an integer from 1 to 180 seconds");
  }
  const absolutePath = resolve(outputPath);
  const remotePath = `/sdcard/shock2quest-profile-${process.pid}.mp4`;
  try {
    adb(
      serial,
      [
        "shell",
        "screenrecord",
        "--time-limit",
        String(seconds),
        remotePath,
      ],
      { timeout: (seconds + 15) * 1_000 },
    );
    adb(serial, ["pull", remotePath, absolutePath]);
  } finally {
    adb(serial, ["shell", "rm", "-f", remotePath]);
  }
  return absolutePath;
}

export function deviceStatus(serial) {
  let pid = "";
  try {
    pid = adb(serial, ["shell", "pidof", PACKAGE]);
  } catch {
    // An absent process is a valid status.
  }
  let mission = "";
  try {
    mission = adb(serial, ["shell", "cat", MISSION_CONFIG]);
  } catch {
    // A missing selector means the runtime will use its default.
  }
  return {
    serial,
    model: adb(serial, ["shell", "getprop", "ro.product.model"]),
    package: PACKAGE,
    pid: pid || null,
    mission: mission || null,
  };
}

function usage() {
  return `Usage:
  quest-device.mjs [--serial SERIAL] status
  quest-device.mjs [--serial SERIAL] select MISSION
  quest-device.mjs [--serial SERIAL] launch MISSION
  quest-device.mjs [--serial SERIAL] capture OUTPUT.png
  quest-device.mjs [--serial SERIAL] capture-focused MISSION OUTPUT.png
  quest-device.mjs [--serial SERIAL] record OUTPUT.mp4 [SECONDS]
  quest-device.mjs [--serial SERIAL] stop
  quest-device.mjs [--serial SERIAL] restore
  quest-device.mjs [--serial SERIAL] reset-mission`;
}

async function main(argv) {
  const args = [...argv];
  let explicitSerial;
  if (args[0] === "--serial") {
    explicitSerial = args[1];
    args.splice(0, 2);
  }
  const serial = resolveSerial(explicitSerial);
  const [command, ...values] = args;

  if (command === "status") {
    process.stdout.write(`${JSON.stringify(deviceStatus(serial), null, 2)}\n`);
  } else if (command === "select" && values[0]) {
    process.stdout.write(`${selectMission(serial, values[0])}\n`);
  } else if (command === "launch" && values[0]) {
    const result = await launchMission(serial, values[0]);
    const { logs: _logs, ...summary } = result;
    process.stdout.write(`${JSON.stringify(summary, null, 2)}\n`);
  } else if (command === "capture" && values[0]) {
    process.stdout.write(`${captureScreenshot(serial, values[0])}\n`);
  } else if (
    command === "capture-focused" &&
    values[0] &&
    values[1]
  ) {
    process.stdout.write(
      `${captureFocusedFrame(serial, values[1], values[0])}\n`,
    );
  } else if (command === "record" && values[0]) {
    const seconds = values[1] === undefined ? 8 : Number(values[1]);
    process.stdout.write(`${recordVideo(serial, values[0], seconds)}\n`);
  } else if (command === "stop") {
    stopApp(serial);
  } else if (command === "restore") {
    restoreProximityAutomation(serial);
  } else if (command === "reset-mission") {
    restoreMissionSelection(serial, { exists: false, contents: "" });
  } else {
    throw new Error(usage());
  }
}

const isMain =
  process.argv[1] &&
  import.meta.url === pathToFileURL(resolve(process.argv[1])).href;
if (isMain) {
  main(process.argv.slice(2)).catch((error) => {
    process.stderr.write(
      `quest-device: ${error instanceof Error ? error.message : error}\n`,
    );
    process.exitCode = 1;
  });
}
