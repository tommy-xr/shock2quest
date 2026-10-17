# Roomscale VR

## Goal and collision policy

Physical walking moves the player controller through the same collision path as
stick locomotion. Head, hands, gameplay and stereo rendering share the resulting
tracking-to-world mapping. Free travel is applied once; blocked physical travel
cannot accumulate a deferred movement request.

Start with **strict correction**, as selected in the design discussion. Cancel
rejected physical displacement immediately through the rig transform. Keep head
rotation live and allow immediate retreat. Do not spring or ease the camera back.
Evaluate comfort in-headset before enabling this by default. Fade-assisted
recovery is an alternative if strict contact feels uncomfortable.

The correction persists: after pushing into a wall and returning to the same
physical spot, the player ends up farther from the virtual wall. The user
confirmed this matches their roomscale experience. Do not automatically undo
that offset on retreat; reserve rebasing for explicit lifecycle/origin changes.

## Initial spike

Enable **Pause → Developer → Camera & view → Roomscale spike**, or use
`POST /v1/dev-params` with `{"key":"vr_roomscale","value":1}`. It defaults off,
is VR-only, and resets on process restart. Harness:

```sh
cargo dbgr --mission debug_ladder --vr --port 0
```

Inject incremental `head.position` and both hand positions through the SDK with
both sticks zero. These channels use pawn-local world units, not metres.
Physical tracking converts through `METERS_PER_WORLD_UNIT`.

The first valid pose anchors horizontal tracking over the capsule. Subsequent
horizontal head deltas are rotated by pawn yaw and added to the ordinary walk
request. The rig subtracts the full delta; the capsule contributes only the
collision-resolved result. This realizes `allowed - requested` correction without
a second physics step or attributing combined stick/physical travel back to each
source. The existing controller owns gravity, slopes, steps, jumping and moving
support. Its next-kinematic-position timing also applies to roomscale movement.

Transient `vr_tracking::RoomscaleState` starts fresh with each mission. The mission
corrects head/hand input and the shared camera resolver applies the same offset
to every rendered eye. Crouch remains a separate shared vertical stance conversion.
The debug renderer uses injected VR head positions to exercise actual view motion.

The spike centers the head over the capsule. It does **not** yet provide independent
leaning or a head sweep: ceilings, overhangs or poses outside the capsule can still
clip. This is a body-following experiment, not complete head protection.

Invalid tracking and explicit OpenXR origin changes reset delta history. A
single-sample horizontal jump over 0.5 metres is also treated as a discontinuity.
This fallback does not implement full reference-space continuity. Physical motion
is not added while dead, control-locked, free-camera enabled, gripping or topping
out. Pause discards delta history so it cannot replay after resuming. Seamless
leaning, climbing and pause/recenter transitions remain follow-up work.

## Work tracking

- [x] Trace tracking, controller and rendering paths.
- [x] Record roomscale principles in the VR design skill.
- [x] Choose strict correction for the initial experiment.
- [x] Add an opt-in horizontal body-following spike.
- [x] Verify free travel, blocked travel/retreat and simultaneous stick movement.
- [x] Capture deterministic disabled/enabled GIF and still evidence.
- [x] Compile-check the Quest target.
- [ ] Verify physical contact comfort in-headset (no Quest attached during spike).
- [ ] Add head-volume sweeps, initial overlap and moving geometry handling.
- [ ] Choose head/body separation for leaning.
- [ ] Complete pause, death, teleport, recenter and tracking-recovery continuity.
- [ ] Integrate physical walking with hand climbing and throw velocity history.
- [ ] Verify moving platforms, stairs, low ceilings and off-origin turning.
- [ ] Decide whether strict correction is comfortable enough to enable by default.

## Spike validation — September 21, 2026

- `cargo test -p shock2vr vr_tracking --lib`: six tests passed.
- SDK `roomscale.e2e.test.ts` and `lean.e2e.test.ts`: six tests passed, covering
  wall contact/retreat, simultaneous stick movement, off-origin turning,
  disabled/flat isolation and existing flat lean/free-camera behavior.
- `cargo build -p debug_runtime`: passed.
- Oculus runtime `cargo check --target aarch64-linux-android`: passed with the
  repository Android environment and target `AR_aarch64_linux_android` pointing
  to the NDK's `llvm-ar`. No APK installation or headset acceptance was possible.
- Formatting, diff whitespace and skill validation passed. Duplication review
  against `HEAD` found no actionable duplication in the new tracking code.

Local visual evidence is in `/tmp/roomscale-spike-media/`: comparison GIF,
maximum-excursion PNG, retreat PNG, capture script and `verification.json`.
Both sides use the same rebuilt binary and identical 61-sample pose sequence,
with both sticks zero. The preserved old binary differed from current source,
so the comparison isolates the option on/off within the new build.

| Capsule world X | Roomscale off | Roomscale on |
| --- | ---: | ---: |
| Start | -5.80 | -5.80 |
| Maximum physical excursion | -5.80 | -6.48 (wall contact) |
| Return to original physical pose | -5.80 | -4.68 |

The retained offset on retreat is intended. These captures establish horizontal
collision behavior, not headset comfort or complete head-volume protection.

## Acceptance criteria

Automated tests cover physical travel with zero stick input, no double movement,
wall stopping, no stored blocked movement, immediate retreat, preserved head/hand
separation and head height, and flat-mode isolation. Interpret movement in pawn
orientation. A tracking-origin change must not send the character across the map.
Images complement state assertions; they cannot establish comfort.

On Quest, walk slowly and quickly toward a wall, turn and retreat, strafe along it,
then try corners and low ceilings. Test seated/standing, away from the tracking
origin, and with simultaneous stick input. Record visible world motion, discomfort,
tracking latency and post-contact drift. Use vr-device-loop and verify the installed
build before acceptance.

## References

- [Meta character controllers](https://developers.meta.com/horizon/documentation/unity/unity-isdk-character-controller/): capsule following, rig reconciliation and obstruction options.
- [OpenXR space changes](https://registry.khronos.org/OpenXR/specs/1.1/man/html/XrEventDataReferenceSpaceChangePending.html): timing and previous-space transform.
- [VR design skill](../.claude/skills/vr-ui-design/SKILL.md): shared-rig and verification principles.
