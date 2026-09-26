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

Resolve the device and confirm required game data. A 25th Anniversary Remaster
install - what the game targets - keeps everything inside `sshock2.kpf` and has
*no* loose gamesys or missions. Probe for the legacy loose layout too, since a
device provisioned before the remaster still carries it and that is worth
identifying rather than reading as "no data".

```sh
adb devices -l
adb -s SERIAL shell '
  ls -l /sdcard/shock2quest/sshock2.kpf /sdcard/shock2quest/mods/ 2>/dev/null \
    || ls -l /sdcard/shock2quest/shock2.gam \
             /sdcard/shock2quest/motiondb.bin \
             /sdcard/shock2quest/earth.mis
'
```

Statting `shock2.gam` alone reports "missing data" on a correctly-provisioned
remaster install. Note which layout answered: the remaster's upgraded models and
textures are what the VR hands and weapons target, so a visual difference
between two runs can simply be a device still on the legacy layout.

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
SHOCK2QUEST_READY mission=medsci1.mis target_refresh_hz=90 requested_refresh_hz=90 refresh_hz=90 eye_width=... eye_height=...
SHOCK2QUEST_XR_STATE mission=medsci1.mis state=FOCUSED
SHOCK2QUEST_PERF mission=medsci1.mis focused=true samples=... skipped=... fps=...
```

An `am start` success or a live PID does not prove XR is rendering. Require
the first-frame marker, a focused XR state, a top-resumed activity, and
advancing focused performance samples.

## Passthrough glove fit experiment

`debug_gloves` requests `XR_FB_passthrough` only while it is the active scene
and `glove_fit_passthrough` is enabled. It submits a reconstruction underlay
before the alpha-enabled projection layer, with environment blend mode
`OPAQUE`, as required by [Meta's native passthrough documentation](https://developers.meta.com/horizon/documentation/native/android/mobile-passthrough/).
The manifest declares `com.oculus.feature.PASSTHROUGH` optional, and extension
and system capability checks permit a black-background fallback.

The eye target clears to RGBA zero. Straight-alpha shader colors accumulate
premultiplied RGB in that target; preserve coverage alpha with separate alpha
factors `ONE, ONE_MINUS_SRC_ALPHA`. Applying `SRC_ALPHA` to alpha itself
squares coverage and blends translucent UI incorrectly over the room. Rebuild
passthrough resources after `STOPPING`, and destroy them explicitly before
Android's `process::exit`, which skips Rust destructors.
The debug runtime encodes screenshots as RGB, so matching those PNGs verifies
color output but cannot establish that framebuffer alpha is correct.

Expect `SHOCK2QUEST_PASSTHROUGH state=running|stopped|unsupported|failed`.
`running` proves resource creation, not visible camera content. This integration
must be checked on Quest: room visible, both gloves tracked, hide/show working,
exit to a normal scene, re-entry, and suspend/resume. A local black-background
PNG cannot verify compositor passthrough or physical fit. If the user defers
device work (for example while charging), complete local checks and report the
device checks as pending; do not wake/install/launch for that pass.

The compositor owns the passthrough image; application swapchain captures do
not contain it. Inspect device captures for actual room content before claiming
mixed-reality evidence, and use the wearer’s observation if capture omits it.
For physical fit, keep controllers held and record `glove_fit_grip_pose`,
`glove_forward_cm`, `glove_side_cm`, `glove_up_cm` and `glove_fit_size` together (see `DEVELOPMENT.md`). These
use a global forward default of −15 cm; the other fit controls remain scene-only
previews. Passthrough does not add optical hand tracking.

Dependency trap: the checked-in `openxr` 0.21.1 wrapper's `Passthrough::start`
calls the pause function. The experiment uses `IS_RUNNING_AT_CREATION` and
destroys layer before feature on exit; do not replace this with pause/start
until the dependency implementation is verified or fixed.

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

## Drive input on the device

The device runtime can accept remote input, so menu/aim/trigger interaction is
verifiable without a human in the headset. It is gated on a port file (env vars
do not reach an Android app) and binds loopback only:

```bash
adb shell "echo 8171 > /sdcard/shock2quest/debug-port.txt"   # then (re)launch
adb logcat -d RustStdoutStderr:V '*:S' | grep SHOCK2QUEST_DEBUG_SERVER
adb forward tcp:8171 tcp:8171

curl -s http://127.0.0.1:8171/v1/status
curl -s -X POST http://127.0.0.1:8171/v1/control/input \
  -d '{"right_hand.rotation":[0,0,0,1],"right_hand.trigger":1.0}'
curl -s -X POST http://127.0.0.1:8171/v1/control/input -d '{"right_hand.trigger":null}'
curl -s -X POST http://127.0.0.1:8171/v1/input/action -d '{"action":"QuickSave"}'
adb shell rm /sdcard/shock2quest/debug-port.txt              # disable again
```

Patched channels are an **override**, not a replacement: the frame loop still
builds its `InputContext` from OpenXR and only the claimed channels are
overwritten, so a human can wear the headset while an agent nudges one channel.
`null` (or `POST /v1/control/input/clear`) releases a channel back to the
controller; releasing a channel that was never claimed is a 400, so a typo'd
release cannot silently leave the real override latched. Claims persist until
released - **clear before disconnecting**, or the wearer is left holding
whatever the agent set. The channel vocabulary is `shock2vr::input::remote`,
shared verbatim with `runtimes/debug_runtime`
(`runtimes/oculus_runtime/src/debug_input.rs`).

Caveat: `head.rotation` / `head.look` drive aim and locomotion direction only -
the rendered view still comes from the OpenXR views, so claiming them
desynchronizes what a wearer sees from where the game thinks they are aiming
(and `head.look`'s yaw/pitch is the flat camera convention, not the VR head
frame). Prefer the hand channels for VR interaction.

When testing scene reloads locally, include a run with `--defer-transitions`.
The debug runtime's usual immediate-transition shortcut can bypass the shipping
`Game` dispatch: Quest testing exposed `DebugReloadLevel` sending generated
scene names to the `.mis` parser. It now reuses the debug-scene launcher; the
SDK's `debug-reload.e2e.test.ts` covers repeated resets with shipping behavior.

## Toward a device debug runtime

Prefer a thin loopback-only HTTP server reached through `adb forward`, sharing
wire types and behavior with `runtimes/debug_runtime`, rather than a separate
automation model. Add it incrementally:

1. `GET /v1/info` and `GET /v1/metrics` for mission, session state, frame
   counter, views, and aggregate timings. (`GET /v1/status` covers mission +
   frame counter today.)
2. `POST /v1/screenshot` for a raw left/right swapchain capture.
3. ~~Existing input actions and control channels.~~ Done - see above.
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
- **Loading dots after repeated Home/reopen:** record the latest XR state and
  whether the frame counter advances. Quest 3 testing reproduced a persistent
  `IDLE` state in both `debug_gloves` and `debug_minimal`, even with Android's
  activity resumed; a fresh `launch` recovers. One successful resume does not
  establish repeated-cycle reliability. Compare a scene without passthrough
  before attributing this symptom to the passthrough lifecycle.
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
