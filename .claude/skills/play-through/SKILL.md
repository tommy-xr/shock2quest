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
  and a goal (default: reach the level's exit). Use `--auto` to keep iterating to
  campaign completion, `--auto-once` for one iteration, or `--restart` to forget
  the previous ledger and roll a fresh campaign.
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
  3. TRIAGE     → keep only REAL findings; split progress BLOCKERS from playable findings.
  4. FIX        → file + delegate every finding immediately; fix blockers on the campaign
                  stack, and fix playable findings independently in parallel.
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
mission, repro: exact levers + step count, expected vs actual, the screenshot,
and — in a rolled campaign — the campaign configuration: scenario, tweak,
asset set, seed; same on the fix PR).

**Attach stateful reproduction evidence locally.** When a finding depends on
deep campaign state, create a dedicated game save immediately before the
smallest reproducing action (for example `pt-shodan-issue-725-repro`), then
record its logical save name, SHA-256, mission, position, asset set, and load
steps in `data.json`, the local report, and the fix-agent handoff. Never
overwrite or repurpose the campaign frontier or a user-owned save; the fix agent
copies the reproduction save into its task-owned asset directory before using
it. Skip this artifact when a fresh mission plus concise steps reproduces the
issue just as reliably.

Game saves contain user-generated state and retail-derived data. Do **not**
commit, attach, gist, or otherwise upload them to a public issue/PR by default.
The public issue may mention that a hashed local reproduction save exists, but
must not expose a user-specific absolute path. Publish the binary only with
explicit user authorization and repository-policy approval.

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
discipline, opens a PR `Fixes #n`, and re-validates.

**Playable findings start fixes immediately and do not wait for session end.**
When the playtester discovers a likely issue but can still make progress, it
must send the manager an early notification with the exact repro, expected vs
actual behavior, current screenshot, and whether the issue is safely bypassable.
The manager performs a focused adversarial review while the playtester keeps
playing. As soon as the finding is validated:

1. File its GitHub issue with the same evidence and campaign configuration used
   for blockers.
2. Immediately delegate an independent fix worker in a separate worktree/branch.
   The worker follows the same faithful-feature, negative-first, PR, restack,
   and green-CI requirements as a blocker fix.
3. Keep the playtest moving in parallel. Do not wait for that worker or put its
   commit onto `fix_branch` unless the issue later becomes necessary for campaign
   progress.
4. Add the issue and fix-PR links to the session `data.json`/report. The blocker
   ledger remains reserved for changes the campaign must stack in order to
   advance.

Never merely collect validated non-blockers for a later sweep. If worker slots
are full, preserve discovery order and start the next fix as soon as a slot
opens; lack of an immediately free slot does not stop the playable session.

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

## Campaign randomization (`roll`)

**Every new campaign starts with a roll.** Instead of always marching
earth→shodan, the campaign randomizer picks three independent axes and persists
them in the ledger (tables + per-pick instructions live in `scenarios.mjs`):

1. **Mission-sequence scenario** — one of eight slices of the game, each with
   its own mission order and goal: `earth-to-medsci` (training → station →
   medsci1/2), `engineering` (power to the elevator), `hydroponics` (Toxin-A →
   regulators), `operations` (ops2 elevator → cutscene → sim unit overrides),
   `recreation` (painting codes → transmitter), `command`, `rickenbacker`
   (destroy the eggs), `shodan` (end sequence + boss AI).
2. **Bonus objective / special tweak** — a playstyle or verification constraint
   for every session in the campaign: none, melee-only, loot-everything, hack
   everything hackable, repair everything repairable, buy from every replicator,
   verify OS upgraders, cutscene/research/regen/camera-alarm verification, Navy
   (hack/repair/modify), Marine (standard/electronic/organic/heavy weapons), OSA
   (psi tiers 1–5), AI/pathfinding stress, or a detailed test run.
3. **Asset set** — legacy assets or the 25th Anniversary assets (loaded
   directly from the stock `.kpf` install since #557); the pick is the
   `DARK_ASSET_PATH` every runtime in the campaign must be launched with. Only
   sets where a data-root sentinel exists (`shock2.gam` / `sshock2.kpf` / ...,
   same list as `paths::data_root()`) enter the random draw; a missing install
   can still be forced with `assets=<id>` and the roll prints a warning.

```
node .agents/skills/play-through/playthrough-state.mjs roll              # random campaign (idempotent)
node .agents/skills/play-through/playthrough-state.mjs roll seed=42      # reproducible roll
node .agents/skills/play-through/playthrough-state.mjs roll --restart    # forget ledger, roll fresh
node .agents/skills/play-through/playthrough-state.mjs roll --force scenario=hydroponics tweak=melee-only assets=legacy
node .agents/skills/play-through/scenarios.mjs list                      # browse all ids
```

**The roll happens once per campaign and then sticks**: like `init`, `roll` is
idempotent — re-invoking the skill mid-campaign keeps the existing roll, so the
frontier, scenario, tweak, and assets stay consistent across iterations
(`--force` starts a fresh campaign with a new roll). `--restart` is different:
it is the autonomous-mode spelling for forgetting the previous ledger and
starting a newly rolled campaign; it does not retain the old seed, scenario,
tweak, assets, mission order, fix branch, frontier, blockers, or history. `show`
re-surfaces the goal, the tweak instructions, and the
`DARK_ASSET_PATH` in its NEXT line every iteration — **feed the tweak
instructions and campaign goal into every `playtest` prompt**, and treat a
tweak's verifications as first-class findings (a broken psi power under an OSA
tweak is a real finding even if the mission could be finished without it).
Every roll prints its `seed`, so any campaign can be reproduced exactly (the
RNG stream is identical whether or not picks were forced alongside the seed).

**Tweak setup gaps.** Most scenarios start mid-game with a fresh character, so
a class tweak (Marine/Navy/OSA) may require gear, skills, or psi tiers the start
state doesn't have. Bonus objectives may similarly require hacking/repair skill,
tools, or nanites. Provisioning those prerequisites is then the **first job of
iteration 0** — use the debug runtime's **provisioning levers** to establish them:

```ts
await game.player.spawnItem("Shotgun");        // by template name...
await game.player.spawnItem(-18);              // ...or stable template id (Assault Rifle)
await game.player.setStats({                   // free "training" (no module cost)
  skills: { standard_weapons: 4, hack: 4 },
  strength: 3,
  psi_tier: 2,
  cyber_modules: 20,
});
```

(`POST /v1/player/spawn-item` / `POST /v1/player/stats`; also
`game.player.give(entityId)` for an item the level already contains.) Spawned
items land in the backpack like any pickup — wield a weapon by double-clicking
it in the use-mode inventory strip. Provisioning only *raises* the sheet, only
accepts genuine pickup items, and only takes **gamesys** templates (negative
ids), so it can't duplicate a level's unique quest items: it is a starting
state, not a way to skip gameplay. Note the sheet is storage for most fields —
only Hack + cyber_affinity currently drive anything (hacking difficulty), so
"trained standard_weapons" does not yet change how a gun behaves; don't read
that as a bug. If the tooling still can't provision what a tweak or bonus
objective needs, record that as a **tooling/setup gap** (and satisfy as much of
it as is reachable) — do NOT file "X is broken" game bugs for capabilities or
resources the character was never given.

**Record the roll everywhere it matters:** every issue filed and every fix PR
opened during a campaign must state the rolled configuration — scenario, tweak,
asset set, and seed (e.g. `campaign: hydroponics · melee-only · legacy · seed
42`) — so a reader can tell whether a finding is specific to a playstyle or
asset set, and can reproduce the campaign that surfaced it.

## Autonomous modes (`--auto`, `--auto-once`)

Choose the mode from the user's invocation:

- **`--auto` — persistent:** keep running complete playtest → review → fix →
  replay iterations in the same invocation. Continue across iteration and
  mission boundaries until the campaign finishes, the same blocker's fix fails
  three times, or progress genuinely requires human input.
- **`--auto-once` — bounded:** run exactly one complete iteration, persist its
  result, report, and return. A later invocation resumes from the ledger.

Both modes **auto-create the campaign ledger on first run** — no setup step.
State persists in that ledger so work survives restarts and context compaction,
resumes at the frontier, and never re-treads solved ground:

```
node .agents/skills/play-through/playthrough-state.mjs roll      # idempotent; both autonomous modes call it
node .agents/skills/play-through/playthrough-state.mjs show      # ledger + the NEXT action
#   blocker add <level> <bug|feature-gap> <issue#> <desc...>   ·  blocker set <issue#> <status> [pr#]
#   blocker fail <issue#> [pr#]                                (counts failed fixes; pauses on failure 3)
#   advance <level> <saveName> <x,y,z> [note...]               (sets frontier, bumps iteration)
#   complete <finalLevel> [note...]                             (marks the campaign finished)
```

The ledger holds: the campaign **roll** (`scenario` + `tweak` + `assets`, with
its `seed`), `frontier` (the game **save** to `/v1/load` from), the
`blockers` ledger (issue → PR → status + failed-fix count), terminal campaign
completion, and `fix_branch` — the running branch that **stacks each fix** so
the campaign plays *past* an already-fixed-but-unmerged blocker.

**Restart with a fresh ledger and roll:** invoke the skill with `--restart`.
Before the normal iteration, run `playthrough-state.mjs roll --restart` exactly
once, then `show`. This forgets the entire previous ledger and creates a newly
randomized campaign. Consume `--restart` once; every later iteration in the same
`--auto` invocation uses plain idempotent `roll`. A missing ledger is rolled
normally.

**Clear & start a new campaign:** `playthrough-state.mjs roll --force` (wipes
the ledger back to iteration 0 with a fresh scenario/tweak/assets roll; plain
`init --force` still exists for a fixed, non-randomized order). For a *truly*
clean slate also recreate the `fix_branch` off current `main` and delete stale
frontier saves (`<data_root>/saves/frontier*.sav`) — otherwise the fresh
campaign just launches mission 0 with no frontier to load, which is harmless.

**Each autonomous iteration:**

1. `playthrough-state.mjs roll` (idempotent — rolls scenario + tweak + assets
   and creates the ledger on the first iteration, keeps the roll after), then
   `show` → read the frontier + NEXT action (goal, tweak, `DARK_ASSET_PATH`).
2. **Resume:** build the runtime from the **`fix_branch`** (so accrued fixes are
   in), launch it **with the campaign's `DARK_ASSET_PATH`**, and `POST /v1/load
   {file: frontier.save}` — or launch the first mission fresh at iteration 0.
3. **Playtest** from here toward the goal (the `playtest` primitive), passing the
   campaign goal + the tweak instructions into the playtest prompt → `data.json`.
4. **Review** the session (§2). Shallow/invalid → re-playtest with guidance.
5. **Triage** every validated finding (bug vs **feature-gap** — §3-4;
   feature-gaps get a *faithful*, non-shim fix). While play continues, file and
   delegate each playable finding immediately on an independent branch. Do not
   batch non-blockers at the end of the session.
6. **Fix blockers:** assign a fix worker, using a subagent when available, that
   commits on **`fix_branch`** (stacked) and opens a PR `Fixes #n`. `blocker add`
   / `blocker set` in the ledger. Parallel playable-finding workers continue on
   their own branches and do not gate replay.
7. **Re-validate:** rebuild `fix_branch`, reload the frontier save, confirm the
   playtest now gets **past** the blocker. If it does not, run `blocker fail
   <issue#> [pr#]`. Failures one and two return to triage/fix with the new
   evidence; failure three is terminal and requires a human.
8. **Advance:** `POST /v1/save {file:"frontier"}` at the new furthest point and
   run `playthrough-state.mjs advance <level> frontier <x,y,z>`. If a valid
   session instead clears the final mission and campaign goal, run
   `playthrough-state.mjs complete <finalLevel> <note...>`.
9. Render the session report (+ optional video).
10. **Continue or return:** with `--auto-once`, report this iteration and return.
    With `--auto`, run `show` and immediately begin the next iteration unless a
    terminal condition below has been reached.

**Guardrails (don't skip):**

- **Merge gate is the human.** Fixes open PRs; *you* merge (or CI+`/xreview` gate).
  The `fix_branch` stack lets the campaign progress while PRs await review — the
  goal is a reviewable stack, not auto-merge. An open PR awaiting human merge
  does not stop `--auto`; continue from the stacked fix branch.
- **Three failed fixes = pause.** Count every distinct fix or revision that
  fails step 7 with `blocker fail`. Re-triage and revise after failures one and
  two. On failure three, the ledger marks the blocker failed; stop and give the
  human the issue, PR, attempts, and evidence.
- **Human input = pause.** Stop only when progress requires new authority,
  credentials, external coordination, or a material choice that cannot safely
  be inferred. Ask the smallest blocking question and preserve the frontier.
- **Do not stop `--auto` at routine boundaries.** An iteration ending, a report
  being ready, context compaction, a long-running CI check, or an unmerged green
  PR is not a terminal condition. Continue until campaign completion, three
  failed fixes on one blocker, or required human input.

## Notes
- Delegate playtest, review, and each fix when supported; tell the playtest
  worker to notify the manager as soon as it encounters a bypassable likely
  issue so validation and fixing can begin while play continues. Otherwise keep
  the phases distinct inline. The manager keeps the ledger (frontier,
  issues→fixes, iteration report).
- Navigation today is teleport + short thumbstick drives; real-movement
  playtesting (navmesh traversal) is a planned upgrade — keep frontier positions
  as world coordinates so it drops in.
