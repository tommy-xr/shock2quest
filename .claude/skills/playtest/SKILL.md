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
| `POST /v1/screenshot {filename}` → **Read `/tmp/claude/<file>`** (do this often) | `POST /v1/player/teleport {x,y,z}` (jump to inspect) |
| `GET /v1/entities?filter=&limit=` (name/id/pos/distance) | `POST /v1/control/input {right_hand.thumbstick:[strafe,fwd]}` + step (walk) |
| `GET /v1/entities/:id` (props, links) | `{left_hand.thumbstick:[turn,0]}` + step (look around) |
| `GET /v1/info` (health, pos, wielded, psi) | `POST /v1/entities/:id/message {type:"Frob"\|"Damage"\|"TurnOn"}` |
| `GET /v1/player/inventory` · `GET /v1/quests` | `POST /v1/player/give {entity_id}` (pick up) |
| `GET /v1/transitions` (exits: dest + position) | follow an exit: teleport into a **tripwire** volume; **Frob** a bulkhead **button** |

`/v1/step {frames:N}` after each action so it takes effect (deterministic; no
sleeps). Tripwires fire on entry; bulkhead buttons fire on Frob.

## How to play (toward the goal)
1. Screenshot + **read it**. Describe what you actually see; is it coherent or off?
2. Explore: look around, list nearby entities, teleport near interesting ones
   (door, corpse, item, terminal, monster) and screenshot each.
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
      "bug": { "severity": "High|Med|Low", "class": "[gameplay|visual|functionality]", "detail": "..." } }
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

Return to the caller: the `data.json` path, the frontier, and the blocker (if any).
Leave the runtime running only if asked; otherwise `POST /v1/shutdown`.
