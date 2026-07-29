---
name: vr-device-loop
description: >-
  Build, install, launch, select missions, inspect, capture, troubleshoot, and
  verify shock2quest on an adb-attached Meta Quest. Use for Oculus/OpenXR runtime
  changes, unattended headset testing, device screenshots or recordings,
  mission launch failures, controller/proximity launch gates, and device
  verification of rendering or performance work.
---

# VR device loop

Treat the Quest as the acceptance environment for Oculus runtime changes. Read
`README.md`, `DEVELOPMENT.md`, `runtimes/oculus_runtime/Cargo.toml`, and the
relevant portion of `runtimes/oculus_runtime/src/lib.rs` before changing the
pipeline.

Use `scripts/quest-device.mjs` for repeatable device lifecycle operations:

```sh
node .claude/skills/vr-device-loop/scripts/quest-device.mjs status
node .claude/skills/vr-device-loop/scripts/quest-device.mjs launch medsci1.mis
node .claude/skills/vr-device-loop/scripts/quest-device.mjs capture /tmp/medsci1.png
node .claude/skills/vr-device-loop/scripts/quest-device.mjs record /tmp/medsci1.mp4 8
node .claude/skills/vr-device-loop/scripts/quest-device.mjs restore
node .claude/skills/vr-device-loop/scripts/quest-device.mjs reset-mission
```

Pass `--serial SERIAL` before the command when more than one device is attached.

## Preserve the runtime contract

- Keep game behavior in `shock2vr`; keep OpenXR session, EGL/GLES swapchains,
  tracked poses, compositor submission, and device lifecycle in
  `runtimes/oculus_runtime`.
- Keep mission names data-relative. The device runtime reads
  `/sdcard/shock2quest/vr-mission.txt`; the helper validates and pushes this
  file before launch.
- Use `shock2vr::paths::data_root()` in Rust. Android resolves it to
  `/sdcard/shock2quest`.
- Use release APKs for performance claims and verify the installed package does
  not contain the `DEBUGGABLE` flag.
- Never commit keystore paths, credentials, or device-specific serials.

## Preflight

Resolve the device and confirm required game data:

```sh
adb devices -l
adb -s SERIAL shell ls -l \
  /sdcard/shock2quest/shock2.gam \
  /sdcard/shock2quest/motiondb.bin \
  /sdcard/shock2quest/earth.mis
```

The repository data and device data must describe the same workload. Sync the
retail game data before interpreting visual or performance differences.

## Build and install

Build from the runtime directory so its FFmpeg and signing paths resolve:

```sh
cd runtimes/oculus_runtime
export JAVA_HOME="$(/usr/libexec/java_home -v 1.8)"
source ./set_up_android_sdk.sh
cargo apk build --release
adb -s SERIAL install -r ../../target/release/apk/shock2quest.apk
```

Locate the emitted APK from the final `cargo apk` line instead of assuming the
path when the local Cargo target directory differs. After any Oculus Rust or
manifest change, rebuild and reinstall.

## Establish an unattended XR session

Quest rendering cannot run as a normal background process: the activity must
own a visible, focused OpenXR session. “Headless” here means unattended and
ADB-driven.

`quest-device.mjs launch` performs these steps:

1. Push the selected mission.
2. Pause Guardian dialogs, wake the device, and send the proximity-close
   automation broadcast.
3. Clear logcat, stop the old process, and start `android.app.NativeActivity`.
4. Poll for `SHOCK2QUEST_READY`, which is emitted only after the first submitted
   XR frame, then require `SHOCK2QUEST_XR_STATE ... state=FOCUSED`.

The APK declares optional `oculus.software.handtracking`. Without this,
Horizon OS may intercept an unattended launch with a disabled “Switch to
Controllers” dialog before the app process exists.

Expect these structured records:

```text
SHOCK2QUEST_STARTUP mission=medsci1.mis init_ms=...
SHOCK2QUEST_READY mission=medsci1.mis refresh_hz=... eye_width=... eye_height=...
SHOCK2QUEST_XR_STATE mission=medsci1.mis state=FOCUSED
SHOCK2QUEST_PERF mission=medsci1.mis focused=true samples=... skipped=... fps=...
```

An `am start` success or a live PID does not prove XR is rendering. Require
the first-frame marker, a focused XR state, a top-resumed activity, and
advancing focused performance samples.

## Capture device visuals every iteration

Capture from the Quest for every implementation iteration, including failed
launches:

```sh
node .claude/skills/vr-device-loop/scripts/quest-device.mjs \
  capture /tmp/quest-after.png
node .claude/skills/vr-device-loop/scripts/quest-device.mjs \
  capture-focused medsci1.mis /tmp/quest-focused.png
node .claude/skills/vr-device-loop/scripts/quest-device.mjs \
  record /tmp/quest-after.mp4 8
file /tmp/quest-after.png /tmp/quest-after.mp4
```

Read the still and decode the video before using them as evidence. Require
visible stereo content, the intended mission, plausible disparity, no system
dialog, and no fallback/missing assets. `capture-focused` asserts the named
mission is focused before and after a direct compositor screenshot, rejects
fully black images, and falls back to a short device recording only when the
direct capture fails. A human or image-capable reviewer must still reject Quest
Loft, void, or badly aimed captures. `adb screencap` is compositor evidence; a
future raw-eye endpoint should capture the application swapchains before
distortion for renderer diagnosis.

When opening or updating a PR, use the `pr-visuals` hosting and embedding
workflow, but use these Quest artifacts whenever the claim depends on device
behavior. Include before/after device captures for visual modifications.

## Toward a device debug runtime

Prefer a thin loopback-only HTTP server reached through `adb forward`, sharing
wire types and behavior with `runtimes/debug_runtime`, rather than a separate
automation model. Add it incrementally:

1. `GET /v1/info` and `GET /v1/metrics` for mission, session state, frame
   counter, views, and aggregate timings.
2. `POST /v1/screenshot` for a raw left/right swapchain capture.
3. Existing input actions and control channels.
4. Entity/physics inspection only after the common interface is extracted.

Keep OpenXR rendering focused while serving requests. A server that responds
while the XR session is idle must report that state and reject captures rather
than returning stale pixels.

## Troubleshoot

- **“Switch to Controllers” before PID exists:** inspect
  `ActivityLaunchInterceptorController`; verify the optional hand-tracking
  manifest feature is present in the installed APK.
- **PID exists, no READY marker:** inspect `RustStdoutStderr`, OpenXR session
  transitions, storage permission, and data sentinels.
- **Home remains visible:** repeat proximity-close and start with `-S`; inspect
  the top-resumed activity with `dumpsys activity activities`.
- **Immediate native crash:** use
  `adb logcat RustStdoutStderr:V AndroidRuntime:E DEBUG:E '*:S'`.
- **Bad performance:** confirm release packaging, stop broad logcat consumers,
  settle the mission, then use the `oculus-profiling` skill.

## Finish cleanly

Always restore the device:

```sh
node .claude/skills/vr-device-loop/scripts/quest-device.mjs stop
node .claude/skills/vr-device-loop/scripts/quest-device.mjs restore
```

The restore command re-enables Guardian before disabling proximity automation.
Benchmark sweeps snapshot and restore the previous mission selector. For manual
work, use `reset-mission` to return to the runtime default.
Report the device/model, APK profile, mission, view size, refresh rate, capture
paths, and whether automation was restored.
