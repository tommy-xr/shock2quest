---
name: play-through
description: >-
  Manager loop that hardens the game toward fully-playable end-to-end. It drives
  the `playtest` primitive in a PLAYTEST to REVIEW to FIX to REPLAY loop: play one
  session (agent plays a mission via the debug runtime, emits a data.json),
  adversarially REVIEW that session against a real walkthrough (did it genuinely
  progress, were the actions sensible, are the findings real - not artifacts of
  poking the wrong thing), triage the validated blocker/bugs, file + delegate a
  fix (PR "Fixes #n"), then REPLAY from the advancing frontier - until a mission
  (then the game) plays through with no new blocker. Aggregates every session into
  a self-contained HTML timeline report. Invoke with a mission (default medsci1)
  and a goal (default: reach the level's exit).
---

# play-through — the playtest → review → fix → replay loop

Owns the *loop* that hardens the game to end-to-end playable. The atomic unit is
the **`playtest`** skill (play one session, emit `data.json`); this manager runs
it repeatedly, **reviews** each session for validity, fixes what blocks progress,
and replays a little further — tracking a **frontier** so it never re-plays solved
ground. When the host supports delegation, assign playtest, review, and fix work
to subagents to keep image-heavy work out of the manager's context. Otherwise,
run those phases inline. The manager always owns loop state and the report.

## The loop

```
resume at the frontier (warp/teleport/QuickLoad) — or launch fresh at iteration 0
  1. PLAYTEST   → invoke `playtest` toward the goal → data.json + screenshots + frontier
  2. REVIEW     → adversarially judge the session (below). Shallow/invalid → re-play with guidance.
  3. TRIAGE     → keep only REAL findings; identify the progress BLOCKER.
  4. FIX        → file issue + delegate fix sub-agent (PR "Fixes #n"); re-validate.
  5. REPLAY     → resume at the (now advanced) frontier; confirm it gets further.
repeat until the mission plays through with no new blocker, then advance to the next level
```

Each iteration must get **further**. Stop a mission when a full session reaches its
exit with no new blocker; then chain to the next level and continue. "End-to-end
with confidence" = every mission in sequence plays through clean on a fresh replay.

## 2. REVIEW — the quality gate (do not skip)

A playtest can *look* busy without actually **playing** (teleport-and-poke,
frobbing decorative props, "bugs" that are just the agent doing the wrong thing).
Before acting on a session, review its `data.json` **adversarially**. Use a
separate reviewer subagent when available, or perform a distinct inline review
pass. The reviewer may read screenshots and consult a real System Shock 2
walkthrough for the mission:

- **Did it genuinely progress** toward the goal, or just survey? (moved along the
  intended path, pursued the objective, tried the real exit — vs jumping between
  random nearby entities).
- **Were the actions sensible?** (frobbed *doors / keypads / objective items /
  usable terminals* — not decorative set-dressing; engaged enemies as a player
  would, not just debug-damaged them). Flag nonsensical actions.
- **Coverage vs the walkthrough:** did it do what a player *should* here (get the
  wrench, find the keycard, deal with the objective, reach the elevator)? Note
  what it skipped.
- **Are the findings real** or artifacts? (e.g. "console frob does nothing" may be
  correct-by-design if that console isn't interactive — not a bug). Keep only
  validated findings; downgrade or drop the rest.

Verdict: was this a valid playtest? If **no** (too shallow, wrong actions), re-run
`playtest` with the review's specific guidance (and the walkthrough context)
before triaging. Feed the walkthrough into the *next* playtest so it plays with
intent, not at random.

## 3-4. Triage & fix

Classify each validated finding two ways:
- **Domain:** `[gameplay]` (broken interaction/objective) · `[visual]` (rendering)
  · `[functionality]` (crash / won't load).
- **Kind — this matters as much as the domain, because this is a *partial* port:**
  - **bug** — broken logic in something that's implemented (an off-by-one, a wrong
    comparator, a crash). Targeted fix.
  - **feature gap** — a system that is **stubbed / unimplemented** (a script wired
    to nothing, a `// TODO: Fully implement`, a `UnimplementedScript`, an
    unparsed property). **Most blockers here are this kind.** The "fix" is a real
    feature, and it must be **faithful**, not a shim.

**Blockers first** (they gate progress). File a GitHub issue (domain + kind,
mission, repro: exact levers + step count, expected vs actual, the screenshot).

**Fixing a feature gap — faithfulness is required:**
- The fix agent must **investigate the original System Shock 2 behavior AND the
  engine's existing partial wiring** (the relevant scripts, properties, links,
  `TODO`s, the actual mission entities via `cargo dq`) *before* implementing.
- Implement it **through the game's own entities/scripts/flow** — do NOT bolt on
  a debug-only shim (a new keybinding, a hardcoded shortcut, a fake trigger) as
  the real mechanism. Debug levers are for *testing* the fix, never the fix.
- Expect a **larger PR**; that's fine. A faithful partial implementation with a
  clear "deferred" note beats a shim that looks done.
- **The review checks faithfulness explicitly:** on the fix PR, use the available
  review workflow and ask "does this use the real in-world flow, or does it fake
  the mechanism?" — reject shims. (This is how #426 was caught: it made careers
  differ via F-key debug actions instead of wiring the station career choice.)

For a plain **bug**, a targeted fix + negative-first test is enough. Either way the
fix agent follows the repo's incremental, review, and negative-first-test
discipline, opens a PR `Fixes #n`, and re-validates. Non-blockers: log them,
keep playing past them.

**Opening the PR is not the finish line — the fix agent watches it land green:**
- `cargo fmt --check --all`. `format` is a separate, fast-failing CI job, and it
  fails on changes that compile and test perfectly.
- **Restack before finishing.** Fix agents branch off whatever `main` was when
  they spawned; on a long run `main` moves underneath them (it may even have
  refactored the very function being fixed). `git fetch origin && git rebase
  origin/main`, resolve, then **re-run the tests** — a conflict resolution can
  silently drop a test or revert half a hunk.
- **Read `gh pr checks <N>`** after pushing, and fix what's red. A PR is done
  when CI is green, not when the push succeeds.
- **Widen the build check when touching shared types.** AGENTS.md's
  `-p shock2vr -p desktop_runtime -p debug_runtime` skips `tools/`, which CI
  builds — so a new variant on a `dark` enum breaks `dark_query`'s exhaustive
  matches with a clean local check. For enum/trait changes, check every
  non-Android package (`dark_viewer`, `dark_query`, `debug_command`,
  `hitbox_analyzer`, `bench` too).

## 5. Replay & frontier

Track the **frontier**: the furthest state reached. The robust, faithful way to
persist and resume it is the game's **own save format** (it stores exactly this:
active mission, player position/rotation, quest bits, held items) — at the edge
of known-good, `POST /v1/save {file:"frontier"}`; each replay (a fresh runtime
launch, since fixes rebuild the binary) resumes with `POST /v1/load
{file:"frontier"}`. That survives relaunches, which `QuickLoad` (same-session
only) and manual `transitionLevel`+`teleport` do not. Each iteration starts at
the frontier and pushes further.

**Rebase the campaign branch on `main` every iteration**, before the replay
build. A campaign runs for many hours while `main` keeps moving, and a stale
`fix_branch` makes the loop hunt ghosts: in the Engineering campaign a session
hit a hard crash entering eng1 (`asset_cache` unwrapping `None` on a missing
model, via an alert security camera's model swap), reported it as a Critical
blocker, and burned the rest of the session on it — the fix had already landed
upstream in #576. A stale branch also silently re-tests bugs that are already
gone, and makes every fix PR conflict later. So each iteration:

```
git fetch origin && git merge origin/main   # or rebase; resolve, then re-run tests
RUSTFLAGS="-D warnings" cargo check -p shock2vr -p desktop_runtime -p debug_runtime
cargo test -p shock2vr
```

If a session reports a crash or a hard blocker, **check whether it reproduces on
current `main` before filing** — "already fixed upstream" is a real and common
outcome, and filing it anyway wastes a reviewer's time.

## Report (aggregate, self-contained)

Each `playtest` emits a `data.json` next to its screenshots. Render a session (or
the aggregate) to a **self-contained HTML timeline** — images inlined as base64,
so it's one portable file:

```
node .agents/skills/play-through/render-timeline.mjs <run-dir>/data.json
# -> <run-dir>/report.html   (open file://... ; renders anywhere)
```

`data.json` schema is documented in `render-timeline.mjs` (steps: screenshot +
what the agent saw + what it did + any bug; plus frontier, bugs with issue/fix
links, verdict). Keep per-mission game-data notes in `walkthroughs/<mission>.md`
as context for the playtest + review (spawn, exits, notable items — NOT a script).

Optionally record a **video** of a session — capture frames at a cadence during
play and stitch with the **`video-capture`** skill (60Hz sim / 4 = real-time
15fps). Local artifact.

## Autonomous mode (`--auto`)

Invoke the `play-through` skill with `--auto` using the host agent's skill syntax.
Run exactly one iteration per invocation; a host automation or loop facility may
schedule subsequent invocations. It **auto-creates the campaign ledger on first
run** — no setup step. State persists in that ledger so it resumes at the
frontier and marches forward instead of re-treading:

```
node .agents/skills/play-through/playthrough-state.mjs init      # idempotent; --auto calls it (--force resets)
node .agents/skills/play-through/playthrough-state.mjs show      # ledger + the NEXT action
#   blocker add <level> <bug|feature-gap> <issue#> <desc...>   ·  blocker set <issue#> <status> [pr#]
#   advance <level> <saveName> <x,y,z> [note...]               (sets frontier, bumps iteration)
```

The ledger holds: `frontier` (the game **save** to `/v1/load` from), the
`blockers` ledger (issue → PR → status), and `fix_branch` — the running branch
that **stacks each fix** so the campaign plays *past* an already-fixed-but-
unmerged blocker.

**Clear & start a new campaign:** `playthrough-state.mjs init --force` (wipes the
ledger back to iteration 0). For a *truly* clean slate also recreate the
`fix_branch` off current `main` and delete stale frontier saves (`<data_root>/
saves/frontier*.sav`) — otherwise the fresh campaign just launches mission 0 with
no frontier to load, which is harmless.

**Each `--auto` iteration** (do exactly one; a host loop may repeat it):
1. `playthrough-state.mjs init` (idempotent — creates the ledger on the first
   iteration, keeps it after), then `show` → read the frontier + NEXT action.
2. **Resume:** build the runtime from the **`fix_branch`** (so accrued fixes are
   in), launch it, and `POST /v1/load {file: frontier.save}` — or launch the first
   mission fresh at iteration 0.
3. **Playtest** from here toward the goal (the `playtest` primitive) → `data.json`.
4. **Review** the session (§2). Shallow/invalid → re-playtest with guidance.
5. **Triage** the blocker (bug vs **feature-gap** — §3-4; feature-gaps get a
   *faithful*, non-shim fix).
6. **Fix:** assign a fix worker, using a subagent when available, that commits on
   **`fix_branch`** (stacked) and opens a PR `Fixes #n`. `blocker add` /
   `blocker set` in the ledger.
7. **Re-validate:** rebuild `fix_branch`, reload the frontier save, confirm the
   playtest now gets **past** the blocker.
8. **Advance:** `POST /v1/save {file:"frontier"}` at the new furthest point;
   `playthrough-state.mjs advance <level> frontier <x,y,z>`.
9. Render the session report (+ optional video).

**Guardrails (don't skip):**
- **Merge gate is the human.** Fixes open PRs; *you* merge (or CI+`/xreview` gate).
  The `fix_branch` stack lets the campaign progress while PRs await review — the
  goal is a reviewable stack, not auto-merge.
- **Max iterations / budget** — stop after N iterations (default ~10) or the turn's
  token target, and report.
- **Recurrence = pause.** If a blocker isn't cleared after its fix (step 7 fails),
  `blocker set <n> failed` and **stop for a human** — don't loop on a bad fix.

## Notes
- Delegate playtest, review, and each fix when supported; otherwise keep the
  phases distinct inline. The manager keeps the ledger (frontier, issues→fixes,
  iteration report).
- Navigation today is teleport + short thumbstick drives; real-movement
  playtesting (navmesh traversal) is a planned upgrade — keep frontier positions
  as world coordinates so it drops in.
