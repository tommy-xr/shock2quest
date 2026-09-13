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

When a `play-through` manager delegated the session and you find a likely issue
that does **not** prevent further play, notify the manager immediately with the
exact repro, expected vs actual behavior, current screenshot, and why it is
bypassable, then keep playing. Do not wait until `data.json` is complete: the
manager validates, files, and delegates its fix in parallel. Continue to record
the finding in the final `data.json`.

## Choose the launch configuration

**Always use the 25th Anniversary assets.** Set
`DARK_ASSET_PATH="$HOME/ss2-25th"` on every debug-runtime launch and verify that
the directory contains a data-root sentinel such as `sshock2.kpf` before
starting. If it is missing, report a setup blocker; never fall back to legacy
assets or the repository's `Data/` directory.

Choose the presentation once per session:

- Use the manager's persisted `presentation` when it supplied one.
- An explicit `--vr` in the skill invocation overrides the manager choice or
  standalone coin flip and forces VR.
- Otherwise, for a standalone session, make a fair 50/50 `flat`/`vr` random
  choice by running `node -e 'console.log(Math.random()<.5?"flat":"vr")'`
  exactly once. Record the result before launch so it cannot be re-rolled after
  a failure.

Launch flatscreen with no presentation flag; launch VR by appending `--vr`.
With the SDK, pass `debugFlags: presentation === "vr" ? ["--vr"] : []` to
`GameServer.launch` (the process inherits `DARK_ASSET_PATH`). With raw Cargo:

```bash
DARK_ASSET_PATH="$HOME/ss2-25th" cargo dbgr --mission <m>.mis --port <p>
DARK_ASSET_PATH="$HOME/ss2-25th" cargo dbgr --mission <m>.mis --port <p> --vr
```

## Reach the start state
- **Fresh:** launch the runtime on the mission (SDK `GameServer.launch`, or
  the matching command above), `step 5` to settle.
- **Resume at a frontier** (the manager passes one): warp with
  `POST /v1/control/transition-level {level, loc}` and/or `teleport {x,y,z}` to
  where the last session stalled, so you don't replay solved parts. (`QuickLoad`
  — `POST /v1/input/action {"action":"QuickLoad"}` — only restores a `QuickSave`
  made earlier in the *same* runtime session, so it's not a cross-launch resume.)

## Senses & hands (HTTP; SDK for lifecycle)

| See the world | Act on it |
| --- | --- |
| `POST /v1/screenshot {filename}` → **Read `/tmp/claude/<file>`** (do this often) | **`POST /v1/player/move {x,y,z}`** — navigate (bounded ≤5u hop, collision-checked) |
| `GET /v1/entities?filter=&limit=` (name/id/pos/distance) | `POST /v1/control/input {right_hand.thumbstick:[strafe,fwd]}` + step (walk) |
| `GET /v1/entities/:id` (props, links) | `{left_hand.thumbstick:[turn,0]}` + step (look around) |
| `GET /v1/info` (health, pos, wielded, psi) | `POST /v1/entities/:id/message {type:"Frob"\|"Damage"\|"TurnOn"}` |
| `GET /v1/player/inventory` · `GET /v1/quests` | `POST /v1/player/give {entity_id}` (pick up) |
| `GET /v1/ui` (open MFD panel + labeled, clickable elements) | **loot a corpse/container**: Frob it → `/v1/ui` panel → `pointer.position` center of the item's `screen_rect`, `pointer.pressed` 1→0 |
| `GET /v1/transitions` (exits: dest + position) | follow an exit: **move** up to a **tripwire** volume; **Frob** a bulkhead **button** |

`/v1/step {frames:N}` after each action so it takes effect (deterministic; no
sleeps). Tripwires fire on entry; bulkhead buttons fire on Frob.

Screenshots come back at **800x600** (the declared size, regardless of a HiDPI
framebuffer) — cheap to read often. Add a large `{"max_width": 4000}` on the rare
shot where you need the native framebuffer detail; it never upscales.

### Player-authentic interaction differs by presentation

`POST /v1/entities/:id/message {type:"Frob"}` is a **debug injection**: it drives
the script directly and is fine for diagnosis, but it does **not** prove a player
could do it.

In **flat mode**, frob under the crosshair with squeeze, not trigger:

1. **Aim** — the target must be under the crosshair. With the preferred SDK,
   call `game.player.aimAt(entity, {hitbox:"torso", visibility:"required"})`
   (`head`, `limb`, `center`, and `nearest` are also available). It selects a
   visible live classified creature damage proxy and throws a structured
   `AimOcclusionError` (including the blocker entity/body) when every matching
   point is blocked. `fallback_used` only says whether hitbox classification
   fell back; it does **not** mean the point is visible. The visibility check is
   explicitly from the flat camera eye (`origin:"view"`), not the offset weapon
   muzzle, so after firing step the simulation and verify ammo plus target HP
   rather than inferring a hit from successful aim alone. The helper reports
   the chosen owner/body/joint/world point and accounts for pawn rotation
   restored from a save while still driving the production camera and
   `FlatInteraction` ray. Raw `head.look` and
   `head.rotation` are **pawn-local**, not world-space; do not use an identity
   quaternion as a world-facing direction. If using raw HTTP, turn with
   `{left_hand.thumbstick:[turn,0]}` and confirm the highlighted entity via
   `GET /v1/info` or a screenshot before squeezing.
2. **Squeeze** — the use button is **`right_hand.squeeze_value`**, on a *rising
   edge*: set it `>0.5`, `step`, then back to `0.0`, `step`.
   **`trigger_value` fires the wielded weapon — it does NOT frob.**
3. The target must be frobbable (`PropFrobInfo`) and is the *highlighted* entity
   under the reticle; `GET /v1/info` reports what's highlighted.

```bash
curl -X POST .../v1/control/input -d '{"right_hand.squeeze_value": 1.0}'
curl -X POST .../v1/step -d '{"frames": 2}'
curl -X POST .../v1/control/input -d '{"right_hand.squeeze_value": 0.0}'
curl -X POST .../v1/step -d '{"frames": 2}'
```

A session that reports "frob does nothing" **without** having driven the squeeze
edge has not tested frobbing — that finding will be rejected at review.

In **VR mode**, exercise the production two-hand path. Place and rotate a hand
with `{left|right}_hand.position` and `{left|right}_hand.rotation`, aim its ray at
the target, and choose the input from the hand's state:

| Hand state and input | Production result |
| --- | --- |
| Empty hand, `trigger_value` rising edge | Frobs the ray-hit world object once. Use this for fixed controls such as buttons, doors, and terminals. |
| Empty hand, `squeeze_value > 0.5` | Grabs a ray-hit movable `MOVE` / `USE_AMMO` item. Keep squeeze held; dropping below 0.5 releases it. Squeeze does **not** Frob a fixed world object. |
| Held item, `trigger_value` rising edge | Uses the held item's authored inventory action: scripted non-weapons receive `Frob`; weapons and the Psi Amp receive `TriggerPull`. Releasing trigger emits `TriggerRelease`; squeeze remains the hold/release state. |

For example, Frob a fixed world control with an empty-hand trigger edge:

```bash
curl -X POST .../v1/control/input -d '{"right_hand.trigger_value": 0.0}'
curl -X POST .../v1/step -d '{"frames": 1}'
curl -X POST .../v1/control/input -d '{"right_hand.trigger_value": 1.0}'
curl -X POST .../v1/step -d '{"frames": 2}'
curl -X POST .../v1/control/input -d '{"right_hand.trigger_value": 0.0}'
curl -X POST .../v1/step -d '{"frames": 2}'
```

Grab, hold, and release a movable item with squeeze instead:

```bash
curl -X POST .../v1/control/input -d '{"right_hand.squeeze_value": 0.0}'
curl -X POST .../v1/step -d '{"frames": 1}'
curl -X POST .../v1/control/input -d '{"right_hand.squeeze_value": 1.0}'
curl -X POST .../v1/step -d '{"frames": 2}'
# Verify right_hand_entity_id in /v1/info while squeeze stays above 0.5.
curl -X POST .../v1/control/input -d '{"right_hand.squeeze_value": 0.0}'
curl -X POST .../v1/step -d '{"frames": 2}'
```

VR world panels are controller-ray interfaces too: aim an empty hand at the
rendered element and pulse that hand's trigger. Gameplay panels derive their
`Hover` / `GUIHover` press from `trigger_value`; frontend panels use the shared
VR frontend pointer pass. `pointer.position` / `pointer.pressed` drive the flat
canvas path and do not prove that a VR controller can reach or click a world
panel. Do not use the flat crosshair/`aimAt` result as proof that a VR hand can
reach or operate any world target.

A review must reject a VR "frob does nothing" finding unless the tester aimed
an **empty** hand's ray and drove its **trigger** across the rising edge. A
squeeze-only attempt tested grabbing, and a held-hand trigger tested that held
item's use/fire path, not fixed-world Frobbing. Conversely, do not reject a
valid VR Frob because the tester did not squeeze; squeeze is the flat-mode Frob
gesture, not the VR-mode one.

Use the actual VR forearm/two-hand UI and interaction whenever it is available.
If `/v1/ui`, pointer input, inventory access, combat, or another debug-runtime
lever only exposes the flat path, try the closest production VR input first and
record the missing VR access as a `vr` + `tool` finding. Mark it as a blocker
when it prevents the session goal. A debug-injected Frob may diagnose what lies
beyond the gap, but does not clear the finding.

**Tab opens the inventory UI in flat mode.** `POST /v1/input/action
{action:"ToggleUseMode"}` enters the original's metagame "use" mode: `/v1/ui`
flips to `mode:"use"` and its `strip` lists the carried items as labeled,
clickable elements (top-docked INVBACK grid). Drive it headlessly like any
panel: `pointer.position` at an element's `screen_rect` center, `pointer.pressed`
1→0 (click a carried weapon to wield it). Movement keys still work in use mode;
trigger the action again to return to shooter mode.

**Container loot lives in containers, not in the world.** Items with an
incoming `Contains` link (corpse/crate loot) have no world presence — they
never lie on the floor at their editor coordinates, and `/v1/player/give`
can't honestly reach them. Loot them the way a player does: get close, Frob
the container, then click the item by its `label` in the `/v1/ui` panel — it
lands in `/v1/player/inventory` (a living AI's container stays closed until
it's dead).

**Navigate with `/v1/player/move`, not raw teleport.** It advances the player at
most ~5 units toward the target through the real character controller, so it
**cannot tunnel through walls or out of bounds**. Walk in short hops and check
screenshots — this is what makes it a real playtest instead of warping to
arbitrary (often out-of-level) entity coordinates. Raw `/v1/player/teleport` is
reserved for manager-driven setup/frontier-resume.

It **walks** the same way the player does — stairs, ramps and small ledges are
traversed, and it climbs/descends them for you; an exposed walkable step should
not block the hop. Only the horizontal direction of the target is used; gravity
decides the vertical, so aim at where you want to *stand*, not at a point in the
air.

`/v1/player/move` is a **local fixed-heading walk, not a pathfinder or a
reachability oracle**. `blocked:true` means that one bounded hop did not reach
its requested endpoint; it does not mean there is no route around the obstacle.
When a hop makes partial progress or stops at a visible corner, inspect the
screenshot, try short lateral/waypoint hops, and use sustained thumbstick
locomotion for steering-sensitive passages. Before filing an unreachable-area
bug, corroborate the conclusion with plausible alternate lanes and available
AIPATH data. Treat AIPATH as evidence rather than proof: it can be partitioned
and does not account for every live door or mission object.

**Ladders are not walked, they're climbed** — `/v1/player/move` never grips one.
Aim the head with `{"head.look":[yaw_deg,pitch_deg]}` and hold
`right_hand.thumbstick:[0,1]` (push *into* the ladder). **Pitch is positive
DOWN**: `+60` looks at the floor and descends, `-60` looks at the ceiling and
ascends — the same rotation drives the camera and the movement, so a negative
pitch climbs *up* no matter what you meant. To enter a descent from the top of a
shaft, step off the lip and then push back *toward* the ladder while looking
down; the grip catches within a step's worth of falling.

**Doors block the shapecast** — you can't move through a closed one. The pattern:
move up to the door → it trips the tripwire (or `Frob` it) → `step` and wait for
it to open (re-screenshot) → then `move` through. That's genuine door-by-door
traversal.

## How to play (toward the goal)
1. Screenshot + **read it**. Describe what you actually see; is it coherent or off?
2. Explore: look around, list nearby entities, **move** (bounded hops) toward
   interesting ones (door, corpse, item, terminal, monster) and screenshot each.
   If a `move` reports `blocked`, something's in the way — that's real geometry.
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
  "asset_set": "25th",
  "presentation": "flat|vr",
  "frontier": "medsci1 @ (x,y,z) — reached the locked Sci door",
  "verdict": "one-line summary of how it went",
  "steps": [
    { "index": 1, "title": "Wake in cryo bay", "screenshot": "pt-01.png",
      "observation": "what you saw", "action": "what you did",
      "bug": { "severity": "High|Med|Low", "class": "[gameplay|visual|physics|functionality|tool|test]", "detail": "..." } }
  ],
  "bugs": [
    { "title": "short", "severity": "High", "class": "[gameplay]", "kind": "bug|feature-gap",
      "labels": ["bug", "gameplay", "blocker"], "screenshot": "pt-07.png",
      "detail": "what's wrong + how you triggered it", "blocker": true }
  ]
}
```

- `steps[].bug` is optional (null when the step was clean).
- Mark the progress **blocker** (`"blocker": true`) — the manager fixes it first.
- Suggest only applicable issue `labels` from `bug`, `enhancement`, `gameplay`,
  `visual`, `physics`, `vr`, `test`, `tool`, and `blocker`; never attach every
  label mechanically.
- Set `frontier` to the furthest reachable state so the next session resumes there.
- Optional **session video**: also capture `frame-NNNN.png` every 4 sim-frames
  during play, stitch with the `video-capture` skill, and set `"video":
  "session.mp4"` — `render-timeline.mjs` embeds it as a `<video>` in the report.

Return to the caller: the `data.json` path, the frontier, and the blocker (if any).
Leave the runtime running only if asked; otherwise `POST /v1/shutdown`.
