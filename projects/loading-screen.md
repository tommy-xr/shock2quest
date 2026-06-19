# Loading Screen — Design Note

> Status: 🧭 Investigation complete; stacked-PR plan proposed 2026-06-19.
> Goal: show `LOADING.PCX` with a real, animated progress bar while a level loads
> **in the background**, instead of freezing the window on a black frame.

---

## 1. High-Level Overview

### The problem

Today every level load is **synchronous and single-threaded**. All three runtimes
call `Game::init(...)` once on the main thread *before* the render loop starts, and
mid-game level transitions (`GlobalEffect::TransitionLevel`) run `Mission::load(...)`
**inside** `Game::update`. During a load nothing renders — the window shows a frozen /
black frame until the entire mission (file parse + GPU uploads + entity
instantiation) finishes.

This matches the original Dark Engine (Thief 1/2, SS2), which was single-threaded and
blocked on a static `LOADING.PCX` bitmap with no progress bar — confirmed by the
well-known "force the engine onto one core" bug. We want to keep the *look* (the
authentic `LOADING.PCX` from `intrface.crf`) but **improve the behavior**: keep
submitting frames during the load. For VR this is not cosmetic — a frozen compositor
causes discomfort and ANR-style stalls on Quest.

### What we already have (big head start)

- **A complete 2D screen-space rendering stack** — `UiCanvas`
  (`shock2vr/src/ui/mod.rs`) with `.image()`, `.bar()` (a horizontal progress bar
  that clips fill 0..1), `.text()`, and `.opacity()` primitives, rendered via
  `render_screen_space(...)` with letterboxing. A loading screen is *composition*, not
  new rendering infrastructure.
- **A working fullscreen-image precedent** — `MainMenuScene`
  (`shock2vr/src/scenes/main_menu.rs`) draws `MAIN.PCX` fullscreen via `UiCanvas` and
  drives transitions through `GlobalEffect`. A `LoadingScene` is structurally
  near-identical.
- **The asset is already mounted** — `res/intrface.crf` is in the asset path list
  (`shock2vr/src/lib.rs:270`), so `LOADING.PCX` resolves by name today; PCX decoding
  already exists (`engine/src/texture_format.rs:61`).
- **A proven cross-thread marshalling pattern** — `debug_runtime` runs an HTTP server
  on a tokio thread and marshals all `Game` access onto the main game thread via an
  `mpsc` command channel + `oneshot` replies (`runtimes/debug_runtime/src/main.rs:169-467`).
  This is the template for "worker thread does work, results return to the GL thread."
- **A two-phase importer contract** — `AssetImporter { loader, processor }`
  (`engine/src/assets/asset_importer.rs`): `loader` is CPU parse, `processor` is the
  build/GPU-upload phase. Conceptually this is exactly the CPU/GPU seam we want.

### The core constraint

`Game`, `AssetCache`, and every cached `SceneObject` are **`!Send` and `!Sync`**
(pervasive `Rc`/`RefCell`, raw GL handles). You **cannot** move `Game` or `AssetCache`
to a worker thread, and GPU resource creation (`gl::GenTextures`, VAO/VBO uploads) must
happen on the GL/main thread. The loader/processor split is *conceptually* the CPU/GPU
boundary but is **not enforced as a thread boundary today** — loaders take
`&mut AssetCache` and can recursively trigger GPU work (e.g. `load_model` sub-loads a
`.cal` skeleton).

What **is** `Send + Sync` and shareable across threads: `Arc<dyn Storage>`
(`engine/src/file_system/storage.rs:5`) and the `.crf` zip asset paths
(`shock2vr/src/zip_asset_path.rs:13`). **File reads + pure parsing are the part that
can move off-thread.** GPU upload cannot.

### The design strategy

Three independent improvements, deliverable as a stack. Each is independently valuable
and reviewable:

1. **A `LoadingScene` rendered for ≥1 frame before the (still blocking) load.**
   Static `LOADING.PCX`, zero threading. Immediately removes the black-frame; gives a
   place to hang a progress bar later. *Low risk, high visible payoff.*

2. **Progress reporting** — thread a lightweight progress sink through the existing
   synchronous load so the loader reports coarse phases/counts (parse, geometry,
   entities N/total). With the still-synchronous load this drives a 2-stage bar
   (0%→100% across one frame), but it builds the *plumbing* the bar consumes and the
   phase taxonomy is reusable.

3. **Background loading** — move the heavy *parse* (`dark::mission::read` → `Send` CPU
   data) onto a worker thread, keep GPU upload + entity instantiation on the main
   thread, time-sliced across frames. The `LoadingScene` keeps animating because the
   loop keeps presenting frames. This is the real win and the largest change.

We deliberately split the easy, faithful, low-risk part (1+2: a real loading screen
that *renders*) from the hard concurrency refactor (3). Even if we stop after PR 2,
the user-visible result is already "a loading screen with a progress bar" rather than a
frozen window — the bar just fills in fewer steps.

### Architecture sketch (end state)

```
   main thread (owns GL + AssetCache + Game)         worker thread
   ───────────────────────────────────────          ─────────────────────────
   TransitionLevel effect
     └─ spawn LoadingScene (draws LOADING.PCX)        spawn:
     └─ kick off background parse  ───────────────►   dark::mission::read(.mis)
   each frame:                                        (file I/O + CPU parse only,
     update()  → poll progress channel  ◄──────────   reports % over channel)
     render()  → LoadingScene draws bar               produces Send `LevelData`
     (no heavy work on main thread)    ◄──────────    done: send LevelData back
   when LevelData arrives:
     time-sliced GPU upload + entity instantiation
       (a few ms/frame, bar keeps moving)
     swap active_game_scene → Mission
```

---

## 2. Key Code References

| Concern | Location |
| --- | --- |
| `Game` struct / `init` / `update` / `render_per_eye` | `shock2vr/src/lib.rs:127`, `:260`, `:430`, `:646` |
| Synchronous mission switch | `Game::switch_mission` `shock2vr/src/lib.rs:150`; `GlobalEffect::TransitionLevel` handler `:589`–`:623` |
| Mission load entry | `Mission::load` `shock2vr/src/mission/mod.rs:46`; `MissionCore::load` `shock2vr/src/mission/mission_core.rs:213`; entity loop `:330` |
| Level file parse (CPU-heavy) | `dark::mission::read` `dark/src/mission/mod.rs:125`; scene build `dark/src/mission/scene_builder.rs:13` |
| Per-entity asset loads | `entity_creator.rs:350` (model), `:388` (anim), `:441` (bitmap) |
| Asset cache (`!Send`, GL-coupled) | `engine/src/assets/asset_cache.rs:13`, GL work at `:104-110` |
| Two-phase importer seam | `engine/src/assets/asset_importer.rs`; `MODELS_IMPORTER` `dark/src/importers/model_importer.rs:24-58` |
| GL allocation points | `engine/src/texture.rs:115`, `engine/src/scene/mesh.rs:22`, `engine/src/texture_atlas.rs:167` |
| 2D canvas | `UiCanvas` `shock2vr/src/ui/mod.rs:152`; `.image` `:177`, `.bar` `:186`, `.text` `:196`, `.opacity` `:165`, `render_screen_space` `:219` |
| Loading-screen scene template | `shock2vr/src/scenes/main_menu.rs` (esp. fullscreen image `:215`); `GameScene` trait `shock2vr/src/game_scene.rs:30`, `render_per_eye` `:54` |
| Progress-bar precedent | `shock2vr/src/hud/flat_hud.rs:43-44` |
| Loading asset | `LOADING.PCX` in `res/intrface.crf` (mounted `shock2vr/src/lib.rs:270`); PCX decode `engine/src/texture_format.rs:61` |
| Cross-thread marshalling template | `runtimes/debug_runtime/src/main.rs:169-467` |
| Main-loop call sites to interpose | desktop `runtimes/desktop_runtime/src/main.rs:365/375`; debug `…/debug_runtime/src/main.rs:488/561`; oculus `…/oculus_runtime/src/lib.rs:603/767` |

---

## 3. Stacked PR Breakdown

The stack is ordered so each PR compiles, passes CI (`RUSTFLAGS="-D warnings" cargo
check -p shock2vr -p desktop_runtime -p debug_runtime`), and delivers standalone value.
**PR 0 is a front-loaded feasibility spike that retires PR 3's risk before we commit
to the stack;** PRs 1–2 are low-risk and faithful; PR 3 is the concurrency refactor;
PRs 4–5 polish.

### PR 0 — Feasibility spike: measure & de-risk (gates PR 3)

**Why this exists:** PR 3 (background parse) is simultaneously the highest-risk change,
the crux of the whole feature, *and* carries open unknowns. That's the wrong thing to
discover mid-implementation. This spike answers three questions **with numbers**, on a
throwaway branch, before we build the real stack:

1. **Is `dark::mission::read` actually off-threadable?** (the `Send` / GL-leak question)
2. **Is it worth it?** — i.e. what fraction of load time is off-threadable *parse* vs.
   stuck-on-main-thread *GPU build*. If build dominates, background parse alone won't
   keep the Quest compositor alive and the plan must change.
3. **Can the main-thread remainder be sliced under a VR frame budget?** — the actual
   "will the compositor stay alive" test.

**The crux is smaller than it looked.** The only GPU call inside `dark::mission::read`
is `texture_dimensions` (`dark/src/mission/mod.rs:480`), which calls
`asset_cache.get(&TEXTURE_IMPORTER, …)` solely to read `.width()/.height()` — it decodes
*and uploads* a whole texture to get two integers that live in the **PCX header**
(`engine/src/texture_format.rs:61`). Replacing it with a GL-free header read is a small,
self-contained change with no threading, and it removes the sole GL dependency from
parse. The spike validates exactly this.

> ⚠️ Note: the existing `"loading level took {}s"` log
> (`mission_core.rs:229-233`) is misleading — it brackets only a field move
> (`let scene = abstract_mission.scene_objects;`), **not** the heavy parse or the
> entity loop. We have no usable timing today; real instrumentation is step 1.

**Spike steps:**

1. **Instrument the real phases** (cheap, no refactor, no risk — land this for real):
   add timers around the four genuine cost centers and log per-phase ms:
   - `parse` → `dark::mission::read` (`dark/src/mission/mod.rs:125`) — the off-threadable candidate.
   - `geometry/lightmap` → `to_scene` (`scene_builder.rs:13`) — main-thread GPU.
   - `entities` → the `entities_to_instantiate` loop (`mission_core.rs:330`), plus a
     per-entity max — main-thread GPU (per-entity model/texture uploads, `entity_creator.rs:350`).
   - `finalize` → player/audio init.
2. **Prove the crux:** prototype a GL-free PCX-dimension read (parse the PCX header for
   width/height) and swap it into `texture_dimensions`. Confirm `dark::mission::read`
   no longer touches `AssetCache`/GL. *(This is safe enough to land standalone and feed PR 3.)*
3. **Prove `Send`:** add a `fn assert_send<T: Send>() {}` against the proposed
   `LevelData` boundary (the `Send` output of parse). Let the compiler enumerate any
   remaining `Rc`/GL leakage. Record what, if anything, has to move into `build`.
4. **Throwaway threaded prototype** on 1–2 representative missions (dense `medsci1.mis`,
   plus a large one like `eng1.mis`/`command1.mis`): actually move `parse` to a
   `std::thread`, keep the `LoadingScene` (PR 1) animating, and run `build` time-sliced
   on the main thread. Measure max main-thread slice and frames pumped during load.

**Deliverable — a before/after table across all 23 missions** (steps 1–3 give every
row; step 4 measures the slicing columns on the sampled missions and we model the rest):

| Mission | Total sync (ms) | Parse, off-thread (ms) | Main-thread build (ms) | Off-thread % | Slices @ ~10ms | Max single-asset upload (ms) | Frames pumped (72Hz) | Verdict |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| medsci1 | … | … | … | … | … | … | … | ✅ / ⚠️ |
| … (all 23) | | | | | | | | |

Measurement harness: the debug runtime / SDK
(`tools/shock2-sdk/test/missions.e2e.test.ts` already loads every mission) is the
natural driver — extend it to record the per-phase timings emitted by step 1.

**Decision gates the spike must answer (these reshape the stack if they fail):**

- **Gate A — feasibility:** after step 2, is `dark::mission::read` GL-free and is
  `LevelData` `Send`? *If no:* per-asset streaming or a deeper `AssetCache` refactor is
  required — PR 3 grows substantially; reassess before starting the stack.
- **Gate B — worth it:** is `Parse / Total` a meaningful fraction (target ≳ 40–50%) on
  most missions? *If build dominates instead,* background parse alone won't keep the
  compositor alive — promote PR 4 (time-slicing) ahead of / merge it into PR 3, since
  the main-thread work is the real bottleneck.
- **Gate C — smoothness:** is the **max single-asset upload** (one texture atlas / one
  big model) below the Quest frame budget (13.9 ms @ 72 Hz, 11.1 ms @ 90 Hz)? *If a
  single indivisible upload blows the budget,* note it — slicing can't go finer than one
  asset without sub-asset streaming, which becomes a PR 4 scope decision.

**Compositor-alive criterion (what "enough" means):** the Quest compositor reprojects
the last submitted frame, so it survives short gaps but a multi-second main-thread block
freezes the headset / trips the guardian. Success = the main thread returns to submit a
fresh loading-bar frame at least once per frame budget — i.e. **max slice < ~13 ms**,
with the off-thread parse covering the bulk of the wall-clock. The table makes this
pass/fail per mission instead of a guess.

**Outcome:** PR 0 lands the two safe, independently-useful pieces (real instrumentation +
GL-free PCX dimensions) and produces the numbers that confirm the PR 3→4 design — or
tell us to reshape it — *before* we write the concurrency code.

### PR 1 — `LoadingScene` rendering `LOADING.PCX` (static, no threading)

**Goal:** kill the black/frozen frame. Show the authentic loading bitmap during a
transition, even though the load itself is still blocking.

**Changes:**
- New `shock2vr/src/scenes/loading.rs` — a `GameScene` modeled on `MainMenuScene`.
  `render_per_eye` builds a `UiCanvas`, draws `canvas.image(full_rect, "LOADING.PCX")`,
  emits via `render_screen_space(..., ScaleMode::PreserveAspect)`. `update` is a no-op
  for now (or returns the effect that kicks the real load).
- Wire it into `switch_mission` / the `TransitionLevel` handler (`lib.rs:150`, `:589`):
  set `active_game_scene = LoadingScene` and force at least one rendered frame before
  the blocking `Mission::load` runs. (The cleanest expression: split the transition
  into "show loading scene → next frame, perform load"; a small state field on `Game`
  like `pending_transition: Option<MissionTransition>` consumed in `update`.)

**Why first:** zero concurrency risk, reuses existing UI stack entirely, and
immediately upgrades the worst symptom (frozen window). Establishes the scene the later
PRs animate.

**Test:** SDK e2e (`tools/shock2-sdk/test/*.e2e.test.ts`, gated `SHOCK2_E2E=1`): trigger
a level transition, step one frame, screenshot, assert the loading scene is active /
`LOADING.PCX` is on screen. Negative test first: confirm pre-change the transition
frame is black.

**Caveat to flag in PR:** with a blocking load this shows `LOADING.PCX` for a single
frame then the load stalls the loop until done — a static screen, not yet animated. PR
3 makes it animate.

---

### PR 2 — Progress reporting plumbing

**Goal:** introduce a `LoadProgress` reporting channel and a phase taxonomy, consumed
by `LoadingScene` to draw a `UiCanvas::bar`. Still synchronous load.

**Changes:**
- New small type, e.g. `LoadProgress { phase: LoadPhase, current: u32, total: u32 }`
  with `LoadPhase` ∈ { `ParsingLevel`, `BuildingGeometry`, `InstantiatingEntities`,
  `FinalizingPlayer` }. Phases map to the natural stages already in the loader:
  - parse → `dark::mission::read` (`dark/src/mission/mod.rs:125`); denominator
    `wr_num_cells` (`:171`).
  - geometry/lightmap → `to_scene` (`scene_builder.rs:13`).
  - entities → the `entities_to_instantiate` loop (`mission_core.rs:330`); `total` =
    `entities_to_instantiate.len()` (`:299`) — an exact denominator already in hand.
- Thread a `&mut dyn FnMut(LoadProgress)` (or an `mpsc::Sender<LoadProgress>` to be
  reused as-is in PR 3) through `Mission::load` → `MissionCore::load`. The loader calls
  it at phase boundaries and inside the entity loop (e.g. every N entities).
- `LoadingScene` holds the latest `LoadProgress`, computes a 0..1 fraction (weight
  phases), and draws `canvas.bar(rect, fill_tex, fraction)` + a status `canvas.text`
  (`"Loading… {phase}"`). Reuse the `flat_hud.rs:43` bar pattern.

**Why second:** builds the data plumbing the bar consumes and forces us to choose the
phase taxonomy / weighting *before* the threading change, where it's cheaper to get
right. Using `mpsc::Sender` as the sink even in the synchronous case means PR 3 only
changes *who* runs the producer, not the wire format.

**Test:** unit-test the phase→fraction weighting (pure function, no game). SDK e2e:
during a transition, poll a new debug endpoint (or log scrape) and assert progress
phases advance monotonically 0→1.

---

### PR 3 — Background mission parse (the real async win)

**Goal:** move the heavy CPU parse off the main thread so the `LoadingScene` actually
animates. Coarse-grained: parse the whole level off-thread into `Send` CPU data, then
do GPU upload + entity instantiation on the main thread.

**Approach (coarse, lowest-risk first — *not* per-asset streaming):**
1. Split `Mission::load` into:
   - `parse(storage, mission_name, progress_sender) -> LevelData` — pure file I/O +
     CPU parse (`dark::mission::read` + raw texture/vertex bytes), **no GL, no
     `AssetCache`**. Operates only on `Arc<dyn Storage>` + the `Send + Sync`
     `ZipAssetPath` layer. Returns a `Send` `LevelData` struct.
   - `build(level_data, &mut AssetCache, …) -> Mission` — the existing GPU-upload +
     entity-instantiation work, on the main thread.
2. `switch_mission` spawns `parse` on a `std::thread` with an `mpsc::Sender` for
   progress + a `oneshot`/`mpsc` for the finished `LevelData` (mirror the
   `debug_runtime` marshalling pattern, `main.rs:169-467`).
3. `Game::update` polls: while parsing, feed progress to `LoadingScene`; when
   `LevelData` arrives, run `build` (optionally time-sliced — see PR 4), then swap
   `active_game_scene` → `Mission`.

**Risks — already retired by PR 0:** the GL-leak crux (`texture_dimensions` →
GL-free PCX header read) and the `LevelData` `Send` proof land in PR 0, and Gate B/C
numbers confirm the parse/build split is worth it before this PR starts. By the time we
write PR 3, it is "wire up the threading the spike already proved," not "discover whether
it's possible." If any PR 0 gate failed, the design is reshaped *first* (see Gate
fallbacks above) rather than absorbed here.

Remaining work specific to this PR:
- Land the production `parse`/`build` split (PR 0's prototype was throwaway).
- The per-entity model/texture loads (`entity_creator.rs:350`) stay on the main thread
  in `build` — they're GPU work and dominate on entity-dense levels, which is why PR 4's
  time-slicing matters for keeping the bar smooth (PR 0's table quantifies how much).

**Why third:** depends on PR 1 (scene to animate), PR 2 (progress wire), and PR 0
(crux retired + numbers in hand). Isolating it keeps the earlier wins shippable if this
stalls.

**Test:** SDK e2e: trigger a transition, assert frames keep advancing (e.g. a frame
counter / screenshot diff) *during* the load — the negative test is that pre-PR-3 the
loop is blocked for the full load. Run `tools/shock2-sdk/test/missions.e2e.test.ts`
(all missions load) to catch regressions in the parse/build split.

---

### PR 4 — Time-sliced GPU upload + entity instantiation

**Goal:** when `LevelData` arrives, the main-thread `build` step can still be heavy
(textures, meshes, per-entity models). Spread it across frames so the bar keeps moving
instead of one final hitch.

**Changes:**
- Make `build` resumable: a `MissionBuilder` that does a bounded chunk of work per
  `update` (e.g. ~N entities or a time budget of a few ms), reporting
  `InstantiatingEntities { current, total }` progress, until complete then swaps the
  scene.
- Drive it from `Game::update`'s poll loop.

**Why fourth:** pure refinement of PR 3; only worth doing once background parse exists.
Can be deferred if PR 3's main-thread `build` is already fast enough on target levels —
**measure first** (the loader already logs `"loading level took {}s"` at
`mission_core.rs:233`).

**Test:** assert no single frame during `build` exceeds a dt threshold on a heavy level
(e.g. `medsci1.mis`).

---

### PR 5 — Polish: fade transitions + per-mission art / Quest validation

**Goal:** finish the feel.

**Changes:**
- Fade-in/out using the existing `UiCanvas::opacity` (`ui/mod.rs:165`) — draw a black
  full-rect at animated opacity to cross-fade loading↔world. No new primitive needed;
  there is no dedicated screen-fade system today, so this also gives us a reusable
  transition.
- Optional: VR-appropriate presentation — instead of a flat overlay, a floating
  world-space panel (cf. `cutscene_player.rs:130`) so the loading screen sits
  comfortably in the headset. Decide flat-overlay vs. world-panel per `PresentationMode`.
- Validate on Oculus runtime (`oculus_runtime/src/lib.rs:603/767`): confirm the
  compositor keeps receiving frames during a load (the core VR payoff).

**Why last:** cosmetic; depends on everything above being in place.

---

## 4. Open Questions / Decisions to Make

- **Progress weighting:** how to map phases to a 0..1 bar so it feels roughly linear in
  wall-clock. **PR 0's per-phase table answers this directly** — weight phases by their
  measured ms share.
- **`Game::init` first-load:** the *very first* mission load happens in `Game::init`
  before any loop. Do we also want a loading screen there (requires the runtime loop to
  start before `init` completes), or is that acceptable to leave blocking initially?
  Recommendation: scope PRs 1–4 to **mid-game transitions** first; handle first-boot in
  a follow-up since it touches all three runtime entry points
  (`desktop:279`, `debug:346`, `oculus:439`).
- **Texture-dimension-during-parse** (`dark/src/mission/mod.rs:324`, `:480`):
  *resolved direction* — the PCX header carries width/height, so a GL-free read
  replaces the `asset_cache.get`. PR 0 validates and lands this; it is no longer an open
  question, only a task.
- **Scope of "background":** coarse whole-level parse (this plan) vs. per-asset
  streaming (much larger refactor of the `AssetImporter` loader/processor thread
  boundary). Recommend coarse first; revisit streaming only if load times demand it.

---

## 5. Comparison to the Original Dark Engine

The original SS2 / Dark Engine loaded **synchronously on a single thread** behind a
**static `LOADING.PCX`** with no progress bar (the engine is so single-threaded it has
the infamous multi-core bug requiring affinity to one core). NewDark and the
OpenDarkEngine reimplementation both kept the **blocking** resource-manager model.

**We emulate** the authentic asset (`LOADING.PCX` from `intrface.crf`, drawn on the
existing 2D path with the `SHOCKPAL` palette / `MAINFONT` text) and the per-transition
timing. **We improve** on it by *not blocking*: background parse + a real animated
progress bar, which is both a UX win and, for VR, a requirement (the compositor must
keep getting frames). The port's own `projects/ffmpeg-anr-fix.md` already establishes
the "move I/O to a background thread + show loading state" pattern we reuse here.
