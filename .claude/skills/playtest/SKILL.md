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
| `POST /v1/screenshot {filename}` → **Read `/tmp/claude/<file>`** (do this often) | **`POST /v1/player/move {x,y,z}`** — navigate (bounded ≤5u hop, collision-checked) |
| `GET /v1/entities?filter=&limit=` (name/id/pos/distance) | `POST /v1/control/input {right_hand.thumbstick:[strafe,fwd]}` + step (walk) |
| `GET /v1/entities/:id` (props, links) | `{left_hand.thumbstick:[turn,0]}` + step (look around) |
| `GET /v1/info` (health, pos, wielded, psi) | `POST /v1/entities/:id/message {type:"Frob"\|"Damage"\|"TurnOn"}` |
| `GET /v1/player/inventory` · `GET /v1/quests` | `POST /v1/player/give {entity_id}` (pick up) |
| `GET /v1/ui` (open MFD panel + labeled, clickable elements) | **loot a corpse/container**: Frob it → `/v1/ui` panel → `pointer.position` center of the item's `screen_rect`, `pointer.pressed` 1→0 |
| `GET /v1/transitions` (exits: dest + position) | follow an exit: **move** up to a **tripwire** volume; **Frob** a bulkhead **button** |

`/v1/step {frames:N}` after each action so it takes effect (deterministic; no
sleeps). Tripwires fire on entry; bulkhead buttons fire on Frob.

### Frobbing *in world* (flat mode) — squeeze, not trigger

`POST /v1/entities/:id/message {type:"Frob"}` is a **debug injection**: it drives
the script directly and is fine for diagnosis, but it does **not** prove a player
could do it. To frob the way a player does, in flat mode (the default):

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
most ~5 units toward the target and **shapecasts the player collider**, so it
**cannot tunnel through walls or out of bounds** (returns `blocked:true`,
`distance_moved < requested` when it hits geometry). So you walk to a place in
short hops, checking screenshots — this is what makes it a real playtest instead
of warping to arbitrary (often out-of-level) entity coordinates. Raw
`/v1/player/teleport` is reserved for manager-driven setup/frontier-resume.

It **walks** the same way the player does — stairs, ramps and small ledges are
traversed, and it climbs/descends them for you — so `blocked` means a real
obstruction, not a step. Only the horizontal direction of the target is used;
gravity decides the vertical, so aim at where you want to *stand*, not at a
point in the air.

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
- Optional **session video**: also capture `frame-NNNN.png` every 4 sim-frames
  during play, stitch with the `video-capture` skill, and set `"video":
  "session.mp4"` — `render-timeline.mjs` embeds it as a `<video>` in the report.

Return to the caller: the `data.json` path, the frontier, and the blocker (if any).
Leave the runtime running only if asked; otherwise `POST /v1/shutdown`.
