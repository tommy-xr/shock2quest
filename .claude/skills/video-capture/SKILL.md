---
name: video-capture
description: >-
  Record an mp4 video of the headless debug runtime by capturing periodic
  screenshots and stitching them with ffmpeg. Deterministic fixed-timestep: the
  sim runs at 60Hz, so capturing a frame every 4 sim-frames yields real-time
  15fps. Drive movement (turn/walk via /v1/control/input) while capturing for
  smooth footage - a pan of a scene, a walk down a corridor, or a record of a
  playtest session. Local artifact (not hosted), so file size is fine. Used
  standalone to grab a clip, or by the `playtest` / `play-through` skills to
  record a session.
---

# video-capture — record an mp4 from the debug runtime

Turn the deterministic step+screenshot loop into a video. The sim is fixed 60Hz,
so **capture every 4 sim-frames → real-time 15fps** (every 6 → 10fps). Screenshots
are deterministic (each captures the fully-rendered frame), so no sleeps/retries.

## Capture loop

Against a running runtime (launch it, or reuse a `playtest` session). Make a
subdir under `/tmp/claude/` (where the runtime writes shots) and capture
sequential frames while something is moving:

```bash
BASE=http://127.0.0.1:8080   # the runtime's port
mkdir -p /tmp/claude/rec
# optional: drive smooth motion (a slow turn-in-place pan)
curl -s -X POST $BASE/v1/control/input -d '{"left_hand.thumbstick":[0.35,0.0]}'
for n in $(seq -w 1 60); do
  curl -s -X POST $BASE/v1/step -d '{"frames":4}'                              # 4 -> 15fps
  curl -s -X POST $BASE/v1/screenshot -d "{\"filename\":\"rec/frame-${n}.png\"}"
done
curl -s -X POST $BASE/v1/control/input -d '{"left_hand.thumbstick":[0.0,0.0]}'  # stop
```

Motion options via `/v1/control/input` (set channel, then step): `left_hand.thumbstick:[turn,0]`
to look around, `right_hand.thumbstick:[strafe,forward]` to walk. A discrete
teleport-and-poke session records as a choppier slideshow; continuous motion is
smooth.

## Assemble

```bash
node .claude/skills/video-capture/record-video.mjs /tmp/claude/rec        # -> rec/video.mp4 @ 15fps
node .claude/skills/video-capture/record-video.mjs /tmp/claude/rec out.mp4 15
```

`record-video.mjs` globs `frame-*.png` and runs ffmpeg (`libx264`, `yuv420p`,
even-dimension scale). Requires `ffmpeg` on PATH.

## Notes
- Delegate a long capture to a sub-agent (many screenshots — keep the image
  traffic out of the main context); it returns the mp4 path.
- Frame count / fps: N frames at F fps = N/F seconds. 60 frames @ 15fps = 4s.
- Local-only artifact; embed/host separately only if a PR needs it (see
  `pr-visuals` for GIF+gist hosting).
