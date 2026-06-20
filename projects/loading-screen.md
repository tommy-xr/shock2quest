# Loading Screen — Design Note

> Status: 🧭 Investigation + measurement spike complete; plan updated 2026-06-19.
> Goal: show `LOADING.PCX` with a real, animated progress bar while a level loads
> **in the background**, instead of freezing the window on a black frame.
>
> **PR 0 (the feasibility spike) is complete** — §2 has the numbers across all 23
> missions. All three gates resolved: **Gate A ✅** (GL-free texture-dimension read
> validated on 1863 textures / 0 mismatches; `Send` blockers reduced to two mechanical
> `Rc → Arc` conversions), **Gate B ✅** (≈45–63% of load is off-threadable CPU work,
> once `physics_spatial` is counted alongside `parse`), **Gate C ⚠️** (mostly passes;
> named exceptions: the lightmap-atlas upload and a handful of fat models exceed one
> frame). The background-load design is de-risked; the plan below (§4) is revised to
> match, and the stack is ready to start at PR 1.

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

> **Spike refinement (§2):** the off-threadable set is bigger than just `parse`. The
> physics-collider + spatial-index build (`create_physics_collider` +
> `LevelSpatialData::from_level`, `mission/mod.rs:78-79`) is also pure CPU/no-GL, and is
> a substantial phase (~13–38% of load). Counting it with `parse` is what lifts the
> off-thread fraction to ~45–63%. So "off-thread work" = **parse + physics_spatial**,
> not parse alone.

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

3. **Background loading** — move the off-threadable CPU work (`parse` +
   `physics_spatial` → `Send` CPU data) onto a worker thread, keep GPU upload + entity
   instantiation on the main thread, time-sliced across frames. The `LoadingScene` keeps
   animating because the loop keeps presenting frames. This is the real win and the
   largest change.

We deliberately split the easy, faithful, low-risk part (1+2: a real loading screen
that *renders*) from the hard concurrency refactor (3). Even if we stop after PR 2,
the user-visible result is already "a loading screen with a progress bar" rather than a
frozen window — the bar just fills in fewer steps.

**Two design constraints the spike (§2) locked in:**

- **PR 4's time-slicer must be time-budget-based, not entity-count-based.** Per-entity
  cost is extremely skewed — thousands of ~0ms `AssetCache` hits sprinkled with a few
  10–30ms cold model loads (the cost is per-*unique-model* cold load, not per-entity).
  A count-based slicer is lumpy; a "instantiate until ~8ms elapsed this frame, then
  yield" slicer self-corrects regardless of where the fat models fall.
- **`to_scene` needs two slicing strategies, because its two halves differ.** The
  geometry/texture loop is a sliceable per-group loop (4–48ms, scales with texture
  groups); the lightmap-atlas upload is one **indivisible ~30ms blob** that can't be
  per-group sliced — it's a single ~2-frame hitch to hide behind the loading screen (or
  to attack with the perf work in §4, PR 6).

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

## 2. Spike Results (PR 0, measured 2026-06-19)

All numbers are **desktop** (a fast dev machine), captured by instrumenting the load
path with `[load-timing]`-prefixed logs and driving every mission through the debug
runtime (`cargo dbgr --mission X`). Quest hardware is ~2–4× slower, so treat absolute ms
as desktop-relative; the **ratios** (off-thread fraction, per-entity skew, phase shares)
transfer. Instrumentation lives on branch `spike/loading-screen-load-timing`.

> Harness note: the debug runtime is the right driver (it loads a mission at startup and
> logs the phase timings; `tools/shock2-sdk/test/missions.e2e.test.ts` already enumerates
> all 23). `dark_viewer` is per-model, not per-mission — not useful here.

### 2.1 Per-mission load breakdown (all 23 missions)

Phases: `parse` = `dark::mission::read`; `phys` = `create_physics_collider` +
`LevelSpatialData`; `to_scene` = geometry+lightmap GPU build; `entities` = the
instantiation loop. **off-thread%** = `(parse + phys) / total` — the CPU work movable to
a worker thread. `max_ent` = slowest single entity (cold model load).

| Mission | total | parse | phys | to_scene | entities | off-thread% | max_ent |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| eng1 | 2309 | 546 | 874 | 80 | 520 | **61%** | 10.8 |
| medsci1 | 2050 | 437 | 580 | 73 | 655 | 50% | 11.0 |
| rick1 | 2000 | 442 | 531 | 70 | 641 | 49% | 10.6 |
| eng2 | 1980 | 445 | 621 | 59 | 568 | 54% | **29.9** |
| hydro2 | 1964 | 443 | 494 | 34 | 685 | 48% | 10.9 |
| command2 | 1942 | 436 | 585 | 70 | 567 | 53% | 10.9 |
| rec1 | 1848 | 435 | 533 | 56 | 555 | 52% | 10.8 |
| medsci2 | 1847 | 399 | 482 | 66 | 609 | 48% | 11.0 |
| many | 1734 | 403 | 593 | 36 | 457 | 57% | 10.7 |
| rec2 | 1626 | 409 | 375 | 55 | 534 | 48% | 10.8 |
| ops2 | 1602 | 418 | 390 | 58 | 476 | 51% | **15.3** |
| rec3 | 1489 | 387 | 333 | 52 | 482 | 48% | **14.6** |
| command1 | 1431 | 398 | 327 | 60 | 390 | 51% | 11.0 |
| ops4 | 1427 | 372 | 312 | 62 | 430 | 48% | 11.0 |
| ops3 | 1417 | 383 | 255 | 60 | 454 | 45% | 10.8 |
| earth | 1237 | 412 | 309 | 49 | 232 | 58% | 11.3 |
| hydro1 | 1151 | 328 | 252 | 29 | 300 | 50% | 10.7 |
| station | 1150 | 353 | 252 | 62 | 265 | 53% | 8.6 |
| rick3 | 1105 | 343 | 299 | 52 | 210 | 58% | 10.9 |
| shodan¹ | 1045 | 417 | 237 | 30 | 137 | 63% | 7.3 |
| hydro3 | 842 | 290 | 128 | 29 | 190 | 50% | 10.9 |
| rick2 | 745 | 268 | 64 | 41 | 173 | 45% | 10.6 |
| ops1 | 609 | 253 | 44 | 30 | 89 | 49% | 10.6 |

¹ `shodan.mis` **loaded fine** in the spike — its known crash ([#267]) is later, in
rendering, not load.

### 2.2 Gate verdicts

- **Gate A — feasibility (off-threadable + `Send`): ✅ PASS (tested).** Both halves
  proven on branch `spike/loading-screen-load-timing`:
  - *GL-free read* — the sole GL call in `dark::mission::read` (`texture_dimensions`,
    which decodes+uploads a texture for its w/h) is replaceable by a `pcx::Reader` header
    read fed from the `AbstractAssetPath` layer (already `Send + Sync`). A dual-path
    `assert`-style validation across **all 23 missions — 1863 textures, 0 mismatches, 0
    missing** — confirms identical dimensions. The header read needs only `Send + Sync`
    layers, so removing `&mut AssetCache` from `read()` is now mechanical PR 3 work, not a
    feasibility unknown. *(Caveat: this is a relocation — the texture decode+upload moves
    into `to_scene`, total work unchanged.)*
  - *`Send` output* — a compile-time `assert_send::<SystemShock2Level>()` names exactly
    two blockers, **both `Rc`, neither GL**: `Rc<BspNode>` (BspTree) and
    `Rc<Box<dyn Property>>` (entity_info). Fix = mechanical `Rc → Arc` (the property one
    needs `dyn Property: Send + Sync` and ripples through the gamesys/property layer).
    Feasible, moderate refactor, no fundamental blocker.
- **Gate B — worth it: ✅ PASS, comfortably.** Off-threadable work (parse + physics) is
  **45–63% of load on every mission** — well above the ~40–50% bar. Background loading is
  clearly worthwhile. (Bonus finding: counting `physics_spatial`, not just `parse`,
  is what got us here — `parse` alone is only ~20–35%.)
- **Gate C — slicing smoothness: ⚠️ mostly pass, two named exceptions.**
  - The **entity loop slices cleanly**: ~1000–2000 entities averaging <0.5ms each, so a
    time-budget slicer stays under frame budget easily.
  - **Exception 1 — fat single models.** 4 missions have one entity exceeding the 72Hz
    budget (13.9ms): `eng2` **29.9ms** (`sarclose` / "Closed Protocol Box"), `ops2`
    15.3ms (`lightr`), `rec3` 14.6ms (`malseat`). One model's GPU upload can't be split
    without sub-asset streaming.
  - **Exception 2 — the lightmap atlas.** `to_scene_lightmap` is a fixed ~26–36ms single
    upload regardless of mission (see §2.4) — one indivisible ~2-frame hitch.
  - Neither is fatal for a *loading screen*: a dropped frame behind a static reprojected
    image is invisible. They only matter if we want a perfectly smooth bar → §4 PR 6.

### 2.3 Why "max_ent" ≈ 11ms on nearly every mission (per-model cold-load reframing)

The "heavy entities" are mundane props (a box, a light, a bench). The cost isn't the
entity — it's being the **first** entity to reference a given **unique model**.
`AssetCache` keys on model name, so the first reference pays the full parse + mesh +
texture GPU upload; every later reference is a ~0ms cache hit. Consequences:

- The cache is **already optimal** (each model loaded exactly once) — there is nothing to
  dedupe or "fix" about per-model loading.
- The recurring ~11ms `max_ent` floor across almost all missions is suspicious — likely a
  **one-time first-GPU-upload / pipeline warmup** attributed to whoever loads first, not
  an intrinsic per-entity cost. *Optional* cheap win: pay it once explicitly at
  loading-screen start (pipeline pre-warm) — needs confirming first.
- True outliers above the floor (`sarclose` 30ms) are specific heavier models → Exception
  1 above. So "per-entity slicing" is really "per-unique-model cold-load slicing."
- Caveat: `max_entity_id` is a runtime `EntityId`; it usually maps to a `.mis` object but
  not always (e.g. `earth`'s winner was a runtime/room entity with no `.mis` row).

### 2.4 `to_scene` split — geometry vs lightmap

`to_scene` has two halves with **opposite profiles** (sampled, ms):

| Mission | lightmap upload | geometry loop | texture groups |
| --- | ---: | ---: | ---: |
| eng1 | 35.9 | 48.3 | 107 |
| medsci1 | 35.6 | 45.8 | 99 |
| command2 | 29.9 | 42.2 | 76 |
| rick1 | 28.6 | 45.1 | 79 |
| hydro2 | 30.3 | 7.1 | 79 |
| ops1 | 26.4 | 4.4 | 23 |

- **Lightmap = fixed ~26–36ms, always 1 atlas, mission-independent.** A single indivisible
  GL upload. Plus it's partly wasteful: `generate_texture()` does
  `self.img.clone().into_raw()` (`engine/src/texture_atlas.rs:49`) — a full CPU copy of
  the atlas before upload. The assembly/clone is off-threadable; only the final
  `TexImage2D` must stay on the main thread.
- **Geometry = 4–48ms, scales with texture-group count.** A `for`-loop over groups, each
  doing a texture + mesh upload — naturally sliceable. Its `geometry.verts.clone()`
  grouping is CPU/off-threadable too.

### 2.5 Parse breakdown — where the parse time goes (and can it be optimized?)

Parse (~250–550ms) decomposes as (sampled):

| Mission | cells | bsp | entity_info | texlist | create_geometry | parse |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| eng1 | 270 | 6 | 24 | 23 | 123 | 480 |
| medsci1 | 248 | 5 | 28 | 23 | 115 | 445 |
| eng2 | 264 | 4 | 22 | 23 | 85 | 426 |
| command2 | 272 | 8 | 23 | 23 | 98 | 447 |
| ops1 | 189 | 0 | 10 | 22 | 31 | 257 |

- **`cells` dominates parse (~55–60%).** And — surprise — the lightmap pixel decode +
  atlas packing is only **12–21% of `cells`** (2% on ops1). The other ~80% is
  **byte-by-byte stream deserialization**: `Cell::read` pulls every vertex/poly/index/
  plane one `read_u8()`/`read_u32()`/`read_vec3()` call at a time, across hundreds of
  cells (`dark/src/mission/cell.rs:39`).
- **`create_geometry` is second (~25%)** — CPU vertex-buffer building plus the
  `texture_dimensions` GPU work (the relocation candidate; see §4 PR 0/PR 3).
- `bsp`/`entity_info`/`texlist` are minor (~10% combined).

**Can parse be optimized?** Yes — the highest-value lever is **bulk-read + slice-parse**
the cell data (read the WR chunk into a `Vec<u8>` once, parse fields from an in-memory
`&[u8]` cursor instead of per-call reads off the `BufReader`). Typically 2–3× on
deserialization-bound code → plausibly `cells` ~250ms → ~120–150ms.

**But it's decoupled from the loading screen.** Once parse runs off-thread (PR 3), its
speed no longer gates the compositor — it only shortens total wall-clock (the coarse
design is parse→build sequential) and speeds the flat/desktop baseline (which has no
loading screen). So parse optimization is a **separate, optional perf PR** (§4 PR 6),
sequenced after PR 3 and gated on whether re-measured Quest totals are too long — not
part of the core loading-screen stack.

---

## 3. Key Code References

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

## 4. Stacked PR Breakdown

The stack is ordered so each PR compiles, passes CI (`RUSTFLAGS="-D warnings" cargo
check -p shock2vr -p desktop_runtime -p debug_runtime`), and delivers standalone value.
**PR 0 (the feasibility spike) is complete** — measurement + all three gates resolved
(§2). PRs 1–2 are low-risk and faithful; PR 3 is the concurrency refactor; PRs 4–5
polish; **PR 6 is an optional, decoupled parse perf pass** gated on Quest numbers.

### PR 0 — Feasibility spike: measure & de-risk (gates PR 3) ✅ DONE

**Status: complete.** The spike de-risked PR 3 (the highest-risk change) by answering,
with numbers on branch `spike/loading-screen-load-timing`, whether the parse/build split
is feasible (Gate A ✅), worth it (Gate B ✅), and sliceable under a frame budget
(Gate C ⚠️) — *before* writing the concurrency code. Full results in §2.

**Landable instrumentation produced:** per-phase `[load-timing]` timers in `Mission::load`,
the entity loop, `to_scene` (lightmap vs geometry), and `dark::mission::read` sub-phases —
producing the §2 tables across all 23 missions. (The old `"loading level took {}s"` log at
`mission_core.rs:229-233` was useless — it bracketed only a field move, not the real work.)

**Gate A artifacts (the feasibility proof):**
1. **GL-free PCX dimension read — validated.** `engine::texture_format::read_pcx_dimensions`
   (reuses `pcx::Reader`'s header parse, no GPU) + `AssetCache::get_raw_reader` (raw bytes
   via the `Send + Sync` `AbstractAssetPath` layer). A dual-path validation across all 23
   missions returned **1863 textures, 0 mismatches, 0 missing** — the GL-free dims are
   byte-identical to the decode+upload path. *Honest nuance:* this is a **relocation, not a
   speedup** — the texture decode+upload moves into `to_scene` as a cold load (total work
   unchanged); its value is enabling off-threading. Removing `&mut AssetCache` from
   `read()` is now mechanical PR 3 work.
2. **`Send` proof — ran.** `assert_send::<SystemShock2Level>()` named two blockers, both
   `Rc` (not GL): `Rc<BspNode>` (BspTree) and `Rc<Box<dyn Property>>` (entity_info). Fix =
   `Rc → Arc` (+ `dyn Property: Send + Sync`), mechanical but ripples through the
   gamesys/property layer.
3. *(Not done — optional)* **Threaded prototype** to measure real max main-thread slice +
   frames-pumped. §2 (per-entity distribution + to_scene split) already implies it passes;
   PR 3 itself will confirm directly. Skipped as redundant for the go/no-go decision.

**Gate verdicts (detail in §2.2):** Gate A — **✅ pass** (GL-free dims validated; `Send` =
2 mechanical `Rc → Arc`). Gate B — **✅ pass** (45–63% off-threadable). Gate C —
**⚠️ mostly pass**, two named exceptions (lightmap atlas ~30ms; fat models up to 30ms),
invisible behind a loading screen, relevant only to §4 PR 6.

**Compositor-alive criterion:** the Quest compositor reprojects the last submitted frame,
surviving short gaps but freezing on a multi-second block. Success = main thread returns
to submit a fresh loading-bar frame at least ~once per frame budget (max slice ≲ 13ms),
with off-thread parse+physics covering the wall-clock bulk.

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
   - `parse(storage, mission_name, progress_sender) -> LevelData` — pure CPU work, **no
     GL, no `AssetCache`**: `dark::mission::read` **plus the physics-collider + spatial-
     index build** (`create_physics_collider` + `LevelSpatialData::from_level`), which §2
     showed are also CPU-only and together ~45–63% of load. Operates only on
     `Arc<dyn Storage>` + the `Send + Sync` `ZipAssetPath` layer. Returns a `Send`
     `LevelData`. (Gate A in PR 0 is what makes `dark::mission::read` GL-free.)
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

**Goal:** when `LevelData` arrives, the main-thread `build` step (textures, meshes,
per-entity models) is still ~500–1000ms. Spread it across frames so the bar keeps moving
instead of one final hitch.

**Changes — shaped by §2's measurements:**
- Make `build` resumable: a `MissionBuilder` that does a bounded chunk of work per
  `update`, reporting `InstantiatingEntities { current, total }` progress, until complete
  then swaps the scene. Drive it from `Game::update`'s poll loop.
- **Slice by time budget, not entity count.** Per-entity cost is extremely skewed
  (§2.3): thousands of ~0ms cache hits + a few 10–30ms cold model loads. "Instantiate
  until ~8ms elapsed this frame, then yield" self-corrects regardless of where the fat
  models fall; a fixed "N per frame" is lumpy.
- **Handle `to_scene`'s two halves differently** (§2.4): the geometry/texture loop slices
  per texture-group; the **lightmap-atlas upload is one indivisible ~30ms blob** — accept
  it as a single ~2-frame hitch hidden behind the loading screen (or shrink it via PR 6).

**Known residual hitches (accept for now; invisible behind a static screen):**
- 4 missions have one entity whose model upload exceeds the 72Hz budget (`eng2`
  `sarclose` ~30ms; `ops2`, `rec3` ~15ms) — can't be split without sub-asset streaming.
- The lightmap atlas (~30ms). Both only matter if we want a perfectly smooth bar → PR 6.

**Why fourth:** refinement of PR 3; only worth doing once background parse exists. Don't
guess the budget — the §2 tables (and re-measured Quest numbers) tell you the real
per-phase costs.

**Test:** assert no single frame during `build` exceeds the chosen budget on a heavy
level (e.g. `medsci1.mis`), *except* the known indivisible uploads above.

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

### PR 6 — (Optional, decoupled) Parse perf pass

**Goal:** shorten total load wall-clock by speeding up the CPU parse. **Not part of the
core loading-screen stack** — once parse runs off-thread (PR 3) its speed no longer gates
the compositor; this only shortens how long the bar shows and speeds the flat/desktop
baseline (which has no loading screen).

**Highest-value change (from §2.5): bulk-read + slice-parse the cell data.** ~55–60% of
parse is `cells`, and ~80% of *that* is byte-by-byte stream deserialization (`Cell::read`,
`dark/src/mission/cell.rs:39`) — not the lightmap pixel work (only 12–21%). Read the WR
chunk into a `Vec<u8>` once and parse fields from an in-memory `&[u8]` cursor instead of
per-call reads off the `BufReader`. Typically 2–3× → plausibly `cells` ~250ms → ~120–150ms.

**Secondary:** avoid the lightmap atlas `self.img.clone()` (`texture_atlas.rs:49`); move
atlas/vertex CPU assembly off-thread (folds into PR 3's `parse`).

**Why optional / last:** decoupled from the feature; **gate on whether re-measured Quest
load totals are actually too long** after PR 3. The `Cell::read` rewrite carries
parsing-bug risk, so only spend it if the numbers justify it. Run
`tools/shock2-sdk/test/missions.e2e.test.ts` (all missions load) as the regression net.

**Test:** negative test first — a checksum/equivalence test that the rewritten parser
produces byte-identical `LevelData` to the old one on every mission, *then* confirm the
speedup.

---

## 5. Open Questions / Decisions to Make

- **Progress weighting:** how to map phases to a 0..1 bar so it feels roughly linear in
  wall-clock. **§2.1 answers this** — weight by measured ms share (roughly: parse ~25%,
  physics ~20%, to_scene ~5%, entities ~35%, remainder ~15%; re-weight from Quest numbers).
- **Re-measure on Quest.** All §2 numbers are desktop. Quest is ~2–4× slower, so totals of
  0.6–2.3s become ~2–8s — which makes the compositor-alive payoff bigger and may change
  whether PR 6 (parse perf) is worth it. Re-run the §2 harness on-device after PR 3.
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

## 6. Comparison to the Original Dark Engine

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
