---
name: playtest
description: >-
  Play ONE session of a System Shock 2 mission via the headless debug runtime,
  as a QA agent. SEE the world (screenshots you read) and ACT with the runtime's
  tools (move, look, frob, pick up, attack, follow real triggers) toward a goal -
  observe and react, don't run a script. Emit a structured data.json (per-step
  screenshot + what you saw + what you did + any bug, plus the frontier reached
  and the issues found) so the run renders to an HTML timeline and drives the
  play-through loop. This is the atomic unit; the `play-through` manager runs it
  in a fix-and-replay loop. Invoke with a mission (default medsci1), a start
  state (fresh, or resume at a frontier), and a goal.
---

# playtest — play one session, report structured

Play a mission the way a QA tester would: take in the world, interact, try to
make progress toward the goal, and **notice** what's broken. Output a structured
`data.json` + screenshots. You do NOT fix anything here — you observe and report;
the `play-through` manager triages and delegates fixes.

## Before the first action

- Read the mission context at
  `.agents/skills/play-through/walkthroughs/<mission>.md` when it exists. It is
  an intent/decision guide, not a movement script.
- State the next authored objective in plain language (for example, "loot the
  wrench, break the duct, climb out of cryo"). Do not substitute a nearby-looking
  prop or jump straight to a later transition just because its coordinates are
  discoverable.
- If no mission context exists, consult a real walkthrough and `cargo dq`, then
  leave concise mission notes for the manager to validate before treating a
  finding as a blocker.

## Session bound and stuck policy

One invocation is one bounded evidence-gathering session, not an endless search.

- Discovery sessions stop after roughly 12 meaningful action/observation steps
  or 15 minutes, whichever comes first. A manager replay can continue from the
  frontier. A manager-labeled **final validation replay** is exempt from this
  total-session limit and runs from a fresh start to the mission exit or the
  first new blocker; the three-attempt rule below still applies.
- After three materially different, sensible attempts at the same obstacle, stop
  retrying. Capture the best repro, identify the missing gameplay mechanism or
  automation control, emit `data.json`, and return.
- Repeated screenshots from the same place are not progress. Keep the clearest
  one and explain what changed between attempts.
- When the walkthrough reveals that an earlier required action was skipped,
  first check the manager-provided ledger checkpoint records for prior reviewed
  evidence (checkpoint ID plus originating `data.json`). A save/frontier alone
  is not proof. If the checkpoint is not already proven, classify the session as
  shallow/invalid and retry it before filing a later obstacle as the blocker.

## Reach the start state
- **Fresh:** launch the runtime on the mission (SDK `GameServer.launch`, or
  `cargo dbgr --mission <m>.mis --port <p>`), `step 5` to settle.
- **Resume at a frontier** (the manager passes one): warp with
  `POST /v1/control/transition-level {level, loc}` and/or `teleport {x,y,z}` to
  where the last session stalled, so you don't replay solved parts. (`QuickLoad`
  — `POST /v1/input/action {"action":"QuickLoad"}` — only restores a `QuickSave`
  made earlier in the *same* runtime session, so it's not a cross-launch resume.)

## Senses & hands (HTTP; SDK for lifecycle)

| See the world | Act on it |
| --- | --- |
| `POST /v1/screenshot {filename}` → **Read `/tmp/claude/<file>`** (do this often) | `POST /v1/player/move {x,y,z}` — diagnostic collision probe (bounded ≤5u) |
| `GET /v1/entities?filter=&limit=` (name/id/pos/distance) | `POST /v1/control/input {right_hand.thumbstick:[strafe,fwd]}` + step (walk) |
| `GET /v1/entities/:id` (props, links) | `{left_hand.thumbstick:[turn,0]}` + step (look around) |
| `GET /v1/info` (health, pos, wielded, psi) | `head.look` + squeeze press/release (use/pick up) |
| `GET /v1/player/inventory` · `GET /v1/quests` | `right_hand.trigger` press/release (fire/swing) |
| `GET /v1/transitions` (exits: dest + position) | walk into a **tripwire** volume; squeeze a bulkhead **button** |

`/v1/step {frames:N}` after each action so it takes effect (deterministic; no
sleeps). Tripwires fire on entry; bulkhead buttons fire on Frob.

Use stepped thumbstick input for **proof of gameplay traversal**. Aim toward the
next X/Z target with `head.look`, hold `right_hand.thumbstick:[0,1]` for a small
frame batch, release it, then inspect position and screenshot. `/v1/player/move`
is a bounded, shape-cast spatial probe (≤5 units) that is useful for route setup
and collision diagnosis, but it does not enforce ground support and therefore
cannot prove stairs, drops, ladders, lifts, or open shafts are playable. Raw
`/v1/player/teleport` is reserved for manager-driven frontier setup/resume.

Doors block both ordinary locomotion and the diagnostic shape cast. Walk up to
the door, use normal squeeze interaction when needed, step until it opens, then
walk through.

### Route recovery after a blocked move

A blocked straight-line move does not mean the destination is unreachable. The
mission's authored AIPATH database can provide the turn-by-turn walk route:

```bash
cargo bn path show medsci1.mis \
  --from=-34.97,-4.57,17.86 --to=-37.27,-5.51,31.77
```

Use the player's current position and the target's discovered position. Treat
each waypoint as an **X/Z heading only**: preserve the live player Y and advance
with stepped thumbstick locomotion. Never copy AIPATH Y into `/v1/player/move`.
Screenshots and live collisions still decide what to do at doors, enemies,
debris, lifts, and ladders; the path is spatial planning, not permission to
ignore the world.

Do not report "cannot reach target" after merely trying several direct lines.
First either follow a returned AIPATH route or show that AIPATH reports no route.
Any waypoint Y change is diagnostic context only. At a ladder, lift, stair,
drop, or open shaft, stop and exercise the authored mechanism rather than
translating vertically or horizontally across unsupported space.

`/v1/player/give` and direct entity messages are **diagnostic automation
levers**, not proof that an in-world pickup/frob works. Reach the authored object
first and prefer the real flat controls (`head.look`, then a press/release of
`right_hand.squeeze` for use/pickup; `right_hand.trigger` for the wielded
weapon). If an unavailable container/GUI control forces a narrow diagnostic
bypass, record that automation gap and do not mark the bypassed checkpoint as
passing. Never grant an objective item from across the map.

## How to play (toward the goal)
1. Screenshot + **read it**. Describe what you actually see; is it coherent or off?
2. Explore: look around, list nearby entities, and use stepped thumbstick
   locomotion toward interesting ones (door, corpse, item, terminal, monster).
   Screenshot turns and interactions. A blocked diagnostic move identifies
   collision geometry, not necessarily an unreachable destination.
3. Interact toward progress: pick up items, frob doors/terminals/keypads, fight,
   and follow the level's real exit toward the goal.
4. Hunt bugs the whole time: missing/black textures, floating/clipping/z-fighting,
   T-posed or frozen creatures, doors that won't open, frobs that do nothing,
   items that can't be taken, HUD glitches, physics weirdness, implausible
   positions. Anything you'd file as a playtester — especially whatever **blocks
   progress**.

## Output: `data.json` (drop it next to the screenshots)

Write a `data.json` in the run's output dir (same dir as the `pt-*.png`), in this
exact shape — the manager renders it with `render-timeline.mjs` and reads the
frontier/issues to drive the loop:

```json
{
  "mission": "medsci1",
  "goal": "reach the eng1 exit",
  "generated": "iteration N",
  "frontier": "medsci1 @ (x,y,z) — reached the locked Sci door",
  "verdict": "one-line summary of how it went",
  "steps": [
    { "index": 1, "title": "Wake in cryo bay", "screenshot": "pt-01.png",
      "observation": "what you saw", "action": "what you did",
      "bug": { "severity": "High|Med|Low", "class": "[gameplay|visual|functionality|debug_runtime]", "detail": "..." } }
  ],
  "bugs": [
    { "title": "short", "severity": "High", "class": "[gameplay]", "screenshot": "pt-07.png",
      "detail": "what's wrong + how you triggered it", "blocker": true }
  ]
}
```

- `steps[].bug` is optional (null when the step was clean).
- Mark the progress **blocker** (`"blocker": true`) — the manager fixes it first.
- Set `frontier` to the furthest reachable state so the next session resumes there.
- Optional **session video**: also capture `frame-NNNN.png` every 4 sim-frames
  during play, stitch with the `video-capture` skill, and set `"video":
  "session.mp4"` — `render-timeline.mjs` embeds it as a `<video>` in the report.

Return to the caller: the `data.json` path, the frontier, and the blocker (if any).
Leave the runtime running only if asked; otherwise `POST /v1/shutdown`.
