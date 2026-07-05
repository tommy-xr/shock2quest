---
name: play-through
description: >-
  Harden the game toward fully-playable end-to-end with a PLAYTEST -> FIX ->
  REPLAY loop. An agent actually PLAYS a mission via the headless debug runtime -
  it SEES the world (screenshots it reads) and ACTS with the runtime's tools
  (move, look, frob, pick up, attack, follow triggers) - and reports what it
  observes and where it gets stuck. Each blocker/bug is classified
  ([gameplay]/[visual]/[functionality]), filed as a GitHub issue, and delegated
  to a fix sub-agent (PR linked to the issue); then the agent REPLAYS from the
  frontier to get a little further and find the next issue. Repeat. A run builds
  an HTML timeline report from the playtest (each step: screenshot + note +
  action + any bug). This is a playtest, NOT a scripted checkpoint runner.
  Invoke with a mission (default medsci1) and optionally a progress goal.
---

# play-through — playtest → fix → replay loop

Iteratively drive the game toward **playable end-to-end** by *playing* it. An
agent plays a mission like a QA tester, surfaces the thing that blocks or breaks,
we fix it, and the agent replays a little further to find the next thing. The
value is the agent **noticing** problems organically — not a script asserting
predefined checks.

## The loop

```
launch/resume at the frontier
   → PLAYTEST (agent plays toward the goal, observing + acting)
   → it surfaces an issue (a blocker, or a bug it noticed)
   → classify + file a GitHub issue + delegate a FIX sub-agent (PR "Fixes #n")
   → re-validate the fix, then REPLAY from the frontier
   → gets a little further → finds the next issue
repeat until the mission (then the game) plays through cleanly
```

Each iteration should get **further** than the last. Track a **frontier** (the
furthest level + position reached, plus quest/inventory state) and resume there
each replay — via `transitionLevel(level, loc)` to the frontier level and a
teleport/`QuickLoad`, so you don't replay solved sections every time.

## The playtest agent (how it plays)

Delegate each playtest to a **`general-purpose` sub-agent** (keeps the image-heavy
observe loop out of the main context; it returns a playtest log + a frontier +
the issue to fix). Its brief:

> You are a QA playtester for a System Shock 2 port. A debug runtime of
> `<mission>` runs at `<url>`. **Play toward `<goal>`** (default: reach the
> level's exit / next level). SEE the world and REACT to it — don't run a script.
> Loop: screenshot → **Read the PNG** → decide → act → step → observe. Explore,
> interact (frob doors/terminals, pick up items, fight), and follow the level's
> real exit. **Report where you get stuck and every bug you notice.** Return: a
> step-by-step log (each: what you saw in which screenshot, what you did), the
> **frontier** you reached, and the **blocking issue** (if any) + other bugs,
> each with a screenshot, severity, and classification.

**Senses & hands** (HTTP; SDK `GameServer` for lifecycle):

| Sense the world | Act on it |
| --- | --- |
| `POST /v1/screenshot {filename}` → **Read `/tmp/claude/<file>`** | `POST /v1/player/teleport {x,y,z}` (jump to inspect) |
| `GET /v1/entities?filter=&limit=` (name/id/pos/distance) | `POST /v1/control/input {right_hand.thumbstick:[strafe,fwd]}` + step (walk) |
| `GET /v1/entities/:id` (props, links) | `left_hand.thumbstick:[turn,0]` + step (look around) |
| `GET /v1/info` (health, pos, wielded, psi) | `POST /v1/entities/:id/message {type:"Frob"\|"Damage"\|"TurnOn"}` |
| `GET /v1/player/inventory` · `GET /v1/quests` | `POST /v1/player/give {entity_id}` (pick up) |
| `GET /v1/transitions` (where exits lead + position) | follow an exit: teleport into a **tripwire** volume; **Frob** a bulkhead **button** |

Note: **tripwires** fire on entry (teleport into the volume); **bulkhead buttons**
fire on **Frob** (teleporting into a button does nothing). `/v1/transitions`
gives each trigger's destination + position.

## On an issue (blocker or bug)

**Classify:** `[gameplay]` (a broken interaction/objective — door won't open,
frob does nothing, item can't be taken, trigger won't fire) · `[visual]` (missing
texture, floating/clipping/z-fighting, T-posed creature, HUD glitch) ·
`[functionality]` (crash, level won't load/step).

**File + fix (parallel).** File a GitHub issue (classification, mission, repro:
exact levers + step count, expected vs actual, the screenshot). Spawn a
`general-purpose` fix sub-agent with the issue # + repro + subsystem pointer; it
follows the repo's incremental + `/xreview` + negative-first-test discipline,
opens a PR (`Fixes #n`), and re-validates. Blockers first (they gate progress);
log non-blocking bugs and keep playing past them when possible.

**Replay.** After the fix merges (or on the fix branch), replay from the frontier
and confirm the agent gets further. Note if a fix unblocks new territory.

## The report (built from the playtest)

Assemble an **HTML timeline** from the playtest log — one entry per step with the
screenshot, a note on what the agent saw, the action it took, and any bug — plus
the issues filed and their fix PRs, and how far the frontier advanced this
iteration. Render locally (self-contained HTML, images by relative path) so it's
viewable without hosting. See `render-timeline.mjs` (reusable across runs); keep
the per-mission game-data notes in `walkthroughs/<mission>.md` as context for the
playtest agent (spawn point, exits, notable items — NOT a script to follow).

## Optional fast first pass

Before the agent digs into a level, a quick load+transition smoke can pre-flag
dead levels (won't load, no forward trigger). That's a coarse net; the agent
playtest is where real issues surface. Don't let the first-pass script substitute
for actually playing.

## Notes

- Deterministic: `/v1/step` blocks until frames run; `/v1/screenshot` captures the
  fully-rendered frame — no sleeps/retries.
- Always `POST /v1/shutdown` (or SDK `await using`) when done.
- Navigation today is teleport + short thumbstick drives; richer real-movement
  playtesting (full navmesh traversal) is a planned upgrade — keep frontier
  positions as world coordinates so it drops in later.
