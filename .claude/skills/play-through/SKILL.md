---
name: play-through
description: >-
  Play a mission end-to-end as an automated game tester, following a per-mission
  checkpoint walkthrough via the headless debug runtime (SDK / HTTP - no window,
  deterministic fixed-timestep). For each checkpoint it sets up state (warp,
  teleport, give item, set quest bit), acts, then VERIFIES the outcome
  (mission name, objective bits, inventory, player health/position) and captures
  a screenshot. Every failure is a surfaced gap - classified [debug_runtime] /
  [gameplay] / [functionality] - filed as a GitHub issue AND handed to a fix
  sub-agent in parallel, with the fix PR linked back to the issue. Use to smoke
  the critical path of a level, surface technical breakage, and march toward a
  fully playable end-to-end game. Invoke with a mission name (default: medsci1).
---

# play-through — automated game-tester harness

Drive a mission through a **checkpoint walkthrough** and verify each stage works,
using the debug runtime headlessly (fixed 60 Hz stepping, no manual clicking).
The goal is not to "win" the game with AI — it is to **exercise the critical path
and surface every technical gap**, then drive those gaps to a fix.

## Context discipline (important)

A full run drives many HTTP calls and reads screenshots (image tokens), and each
gap spawns fix work. Keep the **main context** for the run report and the
checkpoint verdicts. **Delegate the heavy parts to sub-agents:**

- The per-mission **execution** (launch runtime → step through checkpoints →
  collect verdicts + screenshots) — one `general-purpose` sub-agent that returns
  a structured checkpoint result table, not the raw HTTP/image traffic.
- Each **gap fix** — its own sub-agent (see "On a gap"), run in parallel.

## The toolkit (what the harness drives)

All via the debug runtime — raw `curl` for one-offs, the **TypeScript SDK**
(`tools/shock2-sdk`, `GameServer.launch`) for multi-step runs (preferred; it
handles launch/readiness/shutdown). Capabilities the checkpoints rely on:

| Need | Lever |
| --- | --- |
| Load / warp levels | `game.transitionLevel(level, loc?)` · `POST /v1/control/transition-level` |
| Move the player | `game.player.teleport({x,y,z})` (warp) · `POST /v1/control/input` (drive — future) |
| Give an item (pickup) | `game.player.give(entityId)` → lands in inventory |
| Find an item's runtime id | `game.entities.byTemplate(id)` · `game.entities.list({filter})` |
| Verify objective | `game.quests.get(name)` / `.list()` / `.set(name, value)` |
| Verify inventory | `game.player.inventory()` |
| Verify player state | `game.info()` → `player.hit_points`, `wielded_entity_id`, position |
| Trigger a switch/frob | `game.entities.sendMessage(id, {type:"TurnOn"|"Frob"|...})` |
| Discrete actions | `game.input.trigger("CycleWeapon"|...)` |
| Capture | `game.screenshot(file)` |
| Step (deterministic) | `game.step({frames})` — blocks until run; 60 Hz fixed |

## Walkthrough format

Each mission has a checkpoint file at `walkthroughs/<mission>.md` (see
`walkthroughs/medsci1.md`). A checkpoint is: **setup → act → verify**, each with
machine-checkable assertions. Checkpoints **derive from game data** (query the
mission with `cargo dq` for triggers, items, quest bits, positions) and are
**cross-checked against an external SS2 walkthrough** for the intended path.
Prefer resolving runtime ids **at run time** (`entities.list` / `byTemplate`)
over hardcoding — ids are stable per mission but the query is self-documenting.

A checkpoint entry:

```
### CP<n> — <title>
- setup:  <warp/teleport/give/step to reach the state under test>
- act:    <the interaction being tested (frob, give, move, trigger), or none>
- verify: <assertions — each must be machine-checkable and cite the lever>
- capture: screenshot cp<n>.png
- on-fail: <hint for classifying the gap if verify fails>
```

## The loop

For each checkpoint, in order:

1. **Setup** — reach the state under test (teleport to the area / warp to the
   level / give a prerequisite item). Warp+teleport is the current navigation
   model; driving the player (thumbstick) is a planned upgrade — see below.
2. **Act** — perform the interaction the checkpoint tests (or nothing, for a
   pure state check).
3. **Step** — advance enough frames for the effect to land (transitions are
   synchronous; scripts/animation need a few frames).
4. **Verify** — evaluate every assertion. A checkpoint **passes** only if all
   pass. Record PASS/FAIL + the actual values.
5. **Capture** — screenshot for the report (and before/after when useful).
6. On FAIL → **surface the gap** (next section). Then continue to later
   checkpoints where possible (a failed checkpoint doesn't have to abort the
   run — note which later checkpoints it blocks).

## On a gap (failure)

Classify, then **both** record and fix — in parallel:

**Classify** the failure:
- `[debug_runtime]` — the harness can't observe/control something it needs (a
  missing endpoint/field). *Fix = add the capability to the debug runtime.*
- `[gameplay]` — a trigger/script/objective behaves wrong (transition doesn't
  fire, item can't be picked up, objective never completes). *Fix = the
  script/mission logic.*
- `[functionality]` — a crash, an asset that won't load, a mission that won't
  step. *Fix = the underlying bug.*

**Record** — file a GitHub issue (`gh issue create`) with: the classification
label, the mission + checkpoint, the exact repro (lever + args + step count),
expected vs actual, and any log excerpt. Keep issues deduplicated (search open
issues first).

**Fix** — spawn a `general-purpose` sub-agent (run gaps in parallel) to fix the
root cause. Give it the issue number, the repro, and the relevant subsystem
pointer (CLAUDE.md "Iterating on Visual Features" + the file map). It should:
follow the repo's incremental-change + `/xreview` + negative-first-test
discipline, open a PR, and **link the PR to the issue** (`Fixes #<n>`). When it
returns, **re-run the failed checkpoint** to confirm green.

Do not silently truncate: if a gap blocks later checkpoints, say so in the report.

## Navigation: warp+teleport now, driving later

The current model **teleports** to each checkpoint area and **warps** between
levels — fast, deterministic, and focused on verifying that content/objectives/
items work rather than locomotion. Structure walkthroughs so a later upgrade can
**drive the player** (thumbstick `POST /v1/control/input` + step) over the same
routes to additionally exercise navmesh/collision/locomotion: keep each
checkpoint's target as a world position (drivable) rather than an opaque warp, so
"drive to CP" can replace "teleport to CP" without rewriting the walkthrough.
Only pursue driving once warp+teleport runs green end-to-end.

## Report

End with a single **run report**:

- A checkpoint table: `CP | title | PASS/FAIL | actual vs expected`.
- Screenshots (embed the key ones / the failing ones).
- The gap list: each with classification, the issue number, and the fix PR (or
  "fix in progress").
- A one-line verdict: `<n>/<m> checkpoints green; <k> gaps (<filed>/<fixed>)`.

## Running it

```bash
cd tools/shock2-sdk && npm run build   # first time / after SDK changes
```

Author the run as an SDK scenario driven by the walkthrough (launch → loop →
report), or drive `curl` for a quick pass. The debug runtime is deterministic
(fixed-timestep stepping, `/v1/step` blocks until run, `/v1/screenshot` captures
the fully-rendered frame), so a plain `step` then `verify` needs no sleeps or
retries. Always `POST /v1/shutdown` (or let the SDK's `await using` do it) when
done so the runtime never lingers.
```
