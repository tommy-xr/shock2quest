# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

For a concise contributor checklist, see `AGENTS.md`. This document serves as the extended reference for workflows and engine internals.

## Core Principles

### 1. Small, Incremental Changes

- **Always break larger changes into smaller, manageable steps**
- Make one logical change per commit
- Test each increment before moving to the next
- Prefer multiple small PRs over large, complex ones
- Each change should be independently reviewable and rollback-able

### 2. Development Workflow

- Read and understand existing code before making changes
- Follow existing patterns and conventions in the codebase
- Run tests after each change (if available)
- Use descriptive commit messages that explain the "why"

### 3. Visual Changes

Whenever a change adds or alters something visible (a viewmodel/HUD/rendering/
material/lighting feature, a debug scene, an animation, a shader), capture a
short looping GIF and a still PNG of it and embed them in the PR — this is part
of the definition of done for visual work, so reviewers (human and LLM) can see
the result. When the change modifies an existing visual, include a before/after
too (capture the base ref with the same deterministic request sequence). Use the
**pr-visuals skill** (`.claude/skills/pr-visuals/`): it drives the headless
debug-runtime capture path (`/v1/step` + `/v1/screenshot` — no window
interaction, deterministic fixed-timestep), assembles the GIF, hosts the
binaries in a gist, and embeds them in the PR body — and it runs the capture in
a subagent so the image-heavy work stays out of the main context.

## Project Documentation

### Essential Reading

Before making any changes, review these documents:

- **`DEVELOPMENT.md`** - Setup and build instructions
- **`README.md`** - Project overview and goals

### Reference Materials

- **`references/`** - Technical specifications and data formats
  - `entities.md` - Comprehensive entity system documentation (templates, properties, links)
  - `cutscene_formats.md` - Cutscene format documentation
  - `pathfinding.md` - AI pathfinding and navigation mesh documentation
  - `dark_engine_climbing_info.md` - Climbing mechanics reference
  - Various `.spew` files with animation and sound data
- **`projects/`** - In-progress feature documentation and design notes

## Project Structure

This is a Rust-based VR port of System Shock 2. Key components:

- `dark/` - Dark engine file format readers (bin, mis, cal, gam, etc)
- `engine/` - Core OpenGL rendering engine
- `shock2vr/` - Core gameplay logic
  - `scripts/` - Object script implementations
  - `mission/` - Mission running logic
  - `save_load/` - Game state serialization
  - `creature/` - Creature definitions and hitboxes
  - `input/` - Discrete input actions shared across runtimes (see Input Action System below)
  - `pathfinding/` - A* pathfinding service and path visualization
- `runtimes/` - Platform-specific runtime implementations
  - `desktop_runtime/` - Desktop version
  - `oculus_runtime/` - Oculus Quest VR version
  - `debug_runtime/` - HTTP-controlled runtime for automation and testing
- `tools/` - Development tools
  - `dark_query/`, `dark_viewer/` - CLI tools for inspecting game data
  - `shock2-sdk/` - TypeScript SDK for driving the debug runtime (npm package)
  - `debug_command/` - CLI client for the debug runtime (placeholder)

## Entity System Workflow

The System Shock 2 entity system is central to game logic. Understanding it is crucial for most gameplay modifications.

### Core Entity Concepts

- **Templates**: Blueprint definitions with unique IDs (stored in gamesys + mission files)
- **Properties**: Data components defining entity behavior (P$ chunks)
- **Links**: Relationships between entities (L$ chunks, with optional LD$ data)
- **Inheritance**: MetaProp links create template hierarchies
- **Scripts**: Rust implementations providing entity logic

### Key Files and Data Flow

```
Data Files:
├── shock2.gam (gamesys)     - Base templates, common objects
└── *.mis (missions)         - Level-specific entities, overrides

Parsing:
├── dark/src/gamesys/        - Gamesys parsing (shock2.gam)
├── dark/src/ss2_entity_info.rs - Core entity data structures
├── dark/src/properties/     - Property definitions (P$ chunks)
└── dark/src/mission/        - Mission file parsing (*.mis)

Runtime:
├── shock2vr/src/mission/mod.rs - Entity merging and instantiation
├── shock2vr/src/mission/entity_creator.rs - Entity creation logic
└── shock2vr/src/scripts/    - Entity behavior implementations
```

## Input Action System

Discrete, non-contextual inputs (quick save/load, debug spawns, pathfinding test) flow through a unified action system in `shock2vr/src/input/` rather than per-runtime key handling:

```
Desktop:  GLFW polling → DesktopInputMapper → InputActionState → ActionDispatcher → Effects
Debug:    HTTP POST /v1/input/action      → InputActionState → ActionDispatcher → Effects
Oculus:   (no mapper yet - passes an empty InputActionState)
```

- **`InputAction`** (`input/actions.rs`) - serde-enabled enum of all discrete actions
- **`InputActionState`** (`input/state.rs`) - edge-triggered action state, consumed once per `Game::update`
- **`ActionDispatcher`** (`input/dispatcher.rs`) - maps triggered actions to `Effect`s; takes `&InputContext` because player-relative actions need head rotation

**Contextual** hand interactions (trigger pull, grab, drop) are NOT actions - they depend on game state and are handled by `VirtualHand` reading `InputContext` directly.

### Adding a New Action

1. Add a variant to `InputAction` plus its `all()` / `as_str()` entries (`shock2vr/src/input/actions.rs`)
2. Map it to an `Effect` in `ActionDispatcher::dispatch` (`shock2vr/src/input/dispatcher.rs`)
3. Optional: bind a key in `DesktopInputMapper` (`runtimes/desktop_runtime/src/input_mapper.rs`)
4. It is now triggerable via HTTP (`POST /v1/input/action`) and the SDK (`game.input.trigger(...)`) with no further wiring

## Tooling Notes

### Entity, Motion & Speech Queries (`dark_query`)

`cargo dq` inspects gamesys/mission data without launching the game — use it to understand entity relationships and debug complex interactions (`cargo dq --help` for full usage):

```bash
# Entities and templates (positive IDs = entities, negative = templates)
cargo dq entities earth.mis --limit 5          # list entities in a mission
cargo dq entities earth.mis 443                # entity detail: properties, links (both directions), inheritance tree
cargo dq templates 22                          # template -22 detail (avoids awkward negative-ID args)
cargo dq entities earth.mis --filter "*Door*"  # wildcard search across names, properties, links, scripts
cargo dq entities earth.mis --filter "P$SymName:*Robot*"  # property-value filter
cargo dq entities earth.mis --filter "S$stddoor"          # script filter (inheritance-aware)
cargo dq entities --only-unparsed              # find entities with unparsed data

# Motion database (creature animations, tag-based like the original spew files)
cargo dq motion human +playspecmotion --limit 5  # creature by name or ActorType id (0=human, 2=droid, ...)
cargo dq motion 0 +cs:184                        # tags support values (e.g. a specific cutscene)

# Speech database (voices, concepts, tags)
cargo dq speech                                  # list voices
cargo dq speech 2 +concept:spotplayer +alertlevel:three  # query clips by tag filters
```

Tips: use `--limit N` for quick iteration; all filters are case-insensitive and inheritance-aware.

### Benchmarks (`tools/bench`)

`cargo bn` is the benchmark CLI, namespaced by subsystem so new domains (e.g. mission loading) can be added beside `path`. The `path` domain loads AIPATH data straight from mission files (no game session) and exercises `shock2vr::pathfinding::PathfindingService`, so pathfinding changes are measurable:

```bash
cargo bn path stats medsci1.mis     # cell/link counts, flag + okBits audit, walk connectivity
cargo bn path bench --all           # seeded queries: latency, success rate, path quality (inflation/turn)
cargo bn path bench medsci1.mis --queries 500 --json   # machine-readable, reproducible via --seed
cargo bn path show medsci1.mis --from "-10,0,5" --to "20,0,30"  # dump one path's waypoints
cargo bn path cell medsci1.mis --at "20,0,30"   # all cells overlapping a position, with links
```

Run `cargo bn path bench` before and after touching `shock2vr/src/pathfinding/` or `dark/src/mission/path_database.rs` and compare the tables (fixed `--seed` makes runs comparable).

To visualize a path in-game instead of numerically: launch the debug runtime, set start/goal with the `PathfindingTestCycle` input action (bound to P on desktop), and screenshot - see "Iterating on Visual Features" below.

### Iterating on Visual Features

For debugging visual/rendering changes without a full interactive session:

1. **dark_viewer with `--debug-no-render`**: Loads assets and exits after the first frame, useful for adding logging to inspect model/asset data:

   ```bash
   cargo dv grunt_p.bin --debug-no-render
   # Overlay the fitted per-joint hitbox shapes (capsules/boxes) on the animated
   # mesh - physics-free, animatable - to eyeball fit across poses:
   cargo dv grunt_p.bin --animation <clip> --debug-hitboxes --debug-skeletons
   ```

2. **Debug Runtime** (see `projects/debug-runtime.md`): HTTP-controlled game runtime for programmatic control and introspection:

   ```bash
   # Start debug runtime. NOTE: do NOT add an extra `--` after `cargo dbgr` -
   # the alias already ends in `--`, so `cargo dbgr -- --mission ...` passes a
   # literal `--` to clap and fails with "unexpected argument '--mission'".
   #
   # Presentation defaults to FLATSCREEN (like desktop_runtime) - first-person
   # viewmodel, screen-space HUD, right-hand trigger fires. Pass `--vr` for the
   # VR forearm/two-hand path. (Flat is what most weapon/aim testing needs; in VR
   # mode the flat weapon-wield / fire path is inactive.)
   #
   # The window is HIDDEN by default (offscreen render) so the runtime never
   # steals focus or pops to the foreground - `/v1/screenshot` still works.
   # Pass `--visible` to watch the game in a real window.
   cargo dbgr --mission medsci1.mis --port 8080

   # Control via HTTP
   curl http://127.0.0.1:8080/v1/step -X POST -d '{"frames": 10}'
   curl http://127.0.0.1:8080/v1/screenshot -X POST -d '{"filename": "test.png"}'

   # Inspect physics state (works for full missions AND debug_* scenes).
   # Bodies are enumerated from Rapier directly, so many bodies can share one
   # entity_id (e.g. ragdoll limbs) - use ?entity_id=N to scope to one entity.
   curl "http://127.0.0.1:8080/v1/physics/bodies?entity_id=4"
   curl http://127.0.0.1:8080/v1/physics/bodies/12   # detail by body_id

   # Inspect impulse joints (ragdoll constraint health): per-joint anchor
   # separation + applied impulse, labeled by skeleton bone. A healthy ball
   # joint at rest has separation ~0 and a small impulse; persistent values mean
   # the rig is fighting itself.
   curl http://127.0.0.1:8080/v1/physics/joints
   curl http://127.0.0.1:8080/v1/ragdoll/metrics   # per-ragdoll settle metrics

   # Trigger discrete input actions (same actions as desktop keybindings)
   curl http://127.0.0.1:8080/v1/input/actions
   curl -X POST http://127.0.0.1:8080/v1/input/action -d '{"action": "PathfindingTestCycle"}'

   # Inject a script message into an entity (damage/frob/AI signal/switch-link
   # on/off) - drives script behavior directly without a world interaction.
   # Body is a tagged DebugEntityMessage. TurnOn/TurnOff are what tripwires and
   # buttons send over SwitchLinks - use them to exercise doors/traps directly.
   curl -X POST http://127.0.0.1:8080/v1/entities/122/message -d '{"type": "Damage", "amount": 5.0}'
   curl -X POST http://127.0.0.1:8080/v1/entities/40/message -d '{"type": "TurnOn"}'

   # Move the player: right stick = locomotion [strafe, forward], left stick
   # x = turn, left stick y = fly up/down. Set a channel, then step to advance.
   # (Teleporting into a trigger volume fires it, same as walking in.)
   curl -X POST http://127.0.0.1:8080/v1/control/input -d '{"right_hand.thumbstick": [0.0, 1.0]}'

   # IMPORTANT: Always shut down when done to avoid interfering with user's session
   curl -X POST http://127.0.0.1:8080/v1/shutdown
   ```

   **Entity IDs are NOT stable across runs**: the runtime assigns each object a
   shipyard entity ID at load, and a given object gets a **different ID every
   launch** (these also differ from the mission-file object IDs that `dark_query`
   prints). Only **templates** (negative IDs / `PropTemplateId`) and mission-file
   object IDs are stable identities; concrete runtime entity IDs are not. So any
   automation must **discover entities by name or template** each run - e.g.
   `GET /v1/entities?filter=OG-Pipe` then read `AIBehavior`/`AIAlertness` from
   `/v1/entities/:id` - and must **never hardcode a runtime entity ID** (a
   hardcoded ID silently points at a different object next run). The
   `template_id` field in `/v1/entities` output *is* stable across runs and
   matches `dark_query`'s id space, so it's a durable handle for identifying an
   entity (e.g. all three eng1 Blue Monkeys report `template_id` 164/781/809
   every launch); filtering by **name** also works.

   **Stepping & determinism**: stepping uses a **fixed 60 Hz timestep**, so
   `{"frames": N}` advances exactly `N/60` s of simulation time and
   `{"duration":"3s"}` runs exactly `3 * 60` frames - deterministic and
   independent of HTTP request timing (a settling ragdoll falls at a real rate
   regardless of how fast you poll). Only free-running (not stepping) uses real
   wall-clock dt.

   **Reliable control (no headers/retries/sleeps needed)**: `/v1/step` **blocks
   until all requested frames have actually run**, and `/v1/screenshot` captures
   the **fully-rendered** frame, so a plain `step` then `screenshot` is
   deterministic - no `sleep`, no re-shooting to dodge blank frames. JSON bodies
   are parsed **regardless of `Content-Type`**, so `curl -d '{"frames":120}'`
   (without `-H 'Content-Type: application/json'`) works; an omitted body is
   treated as `{}`. (Note: time-based systems - particles, animation - only
   advance via `/v1/step`; physics also advances while free-running because it
   uses its own fixed substep.)

3. **TypeScript SDK (`tools/shock2-sdk`)** — **preferred for multi-step testing and verification**. A Playwright-style wrapper over the debug runtime HTTP API that handles the full lifecycle: spawning the runtime, waiting for readiness, capturing logs, and automatic shutdown via `await using`. See `tools/shock2-sdk/README.md` for the full API.

   ```bash
   cd tools/shock2-sdk
   npm install         # first time only
   npm test            # fast unit tests
   npm run test:e2e    # launches real debug runtimes (pathfinding scenario, mission load smoke tests)
   ```

   ```ts
   import { GameServer } from "@shock2vr/sdk";

   await using game = await GameServer.launch({ mission: "medsci1.mis", port: 8091 });
   await game.step({ frames: 10 });
   await game.input.trigger("PathfindingTestCycle");
   await game.waitFor(
     async () => (await game.pathfindingTest.status()).state === "WaitingForGoal",
   );
   ```

   **When to use which**: raw `curl` is fine for one-off pokes at a running instance; use the SDK whenever a task needs launch/verify/shutdown or multi-step assertions. Write durable scenario tests as `tools/shock2-sdk/test/*.e2e.test.ts` (gated behind `SHOCK2_E2E=1` so `npm test` stays fast). `test/missions.e2e.test.ts` verifies every mission in `Data/` loads — run it after changes to level loading or entity instantiation.

4. **Debug Scenes**: Minimal test scenes for isolating specific features. Pass as the `--mission` argument:

   | Scene                    | Purpose                                      |
   | ------------------------ | -------------------------------------------- |
   | `debug_camera`           | Test security camera AI behavior             |
   | `debug_turret`           | Test turret AI and targeting                 |
   | `debug_ragdoll`          | Test ragdoll physics                         |
   | `debug_hitbox`           | View fitted hitbox shapes vs ragdoll colliders across poses |
   | `debug_gloves`           | Test VR hand/glove rendering                 |
   | `debug_teleport`         | Test VR teleport locomotion                  |
   | `debug_joint_constraint` | Test physics joint constraints               |
   | `debug_hud`              | Test HUD rendering                           |
   | `debug_map`              | Test map/automap rendering                   |
   | `debug_minimal`          | Bare minimum scene for basic testing         |
   | `debug_weapons`          | Flat weapon viewmodel + aim (wall ahead; cycle weapons with `CycleWeapon`) |
   | `debug_psi`              | Psi amp casting (auto-equips the amp; select powers with `CyclePsiPower`) |

   ```bash
   # Use with debug runtime for programmatic control
   cargo dbgr --mission debug_camera --port 8080
   ```

   Debug scenes are defined in `shock2vr/src/scenes/` and provide isolated environments for testing specific game systems without loading full missions. They are fully introspectable over HTTP (entities, physics bodies, raycast) just like real missions, via `GameScene::as_debuggable()`.

### Available Mission Files

For testing entity queries and game features, these mission files are available in `Data/`:

| Mission       | Description                    |
| ------------- | ------------------------------ |
| `earth.mis`   | Earth - tutorial/intro level   |
| `station.mis` | Station - hub area             |
| `medsci1.mis` | MedSci deck 1                  |
| `medsci2.mis` | MedSci deck 2                  |
| `eng1.mis`    | Engineering deck 1             |
| `eng2.mis`    | Engineering deck 2             |
| `hydro1.mis`  | Hydroponics deck 1             |
| `ops1.mis`    | Operations deck 1              |
| `rec1.mis`    | Recreation deck 1              |
| `command1.mis`| Command deck 1                 |

Use with `dark_query`: `cargo dq entities earth.mis --limit 10`

This table lists the most commonly used levels; `Data/` contains the full set (23 `.mis` files including `hydro2/3`, `ops2-4`, `rec2/3`, `command2`, `rick1-3`, `many`, `shodan`). The SDK smoke test `tools/shock2-sdk/test/missions.e2e.test.ts` verifies all of them load.

**Known issue**: `shodan.mis` currently crashes on load (AIPATH parser, [#267](https://github.com/tommy-xr/shock2quest/issues/267)).

### File Format Investigation

When working with entity data, you may need to examine raw game files:

1. **Inspect model files**:

```bash
# Use dark_viewer to examine model structure
cargo dv grunt_p.bin

# Use dark_query to list entities
cargo dq entities medsci1.mis
```

2. **Compare gamesys vs mission data**:

   - Gamesys contains base templates and common definitions
   - Mission files override/extend gamesys data for level-specific needs
   - The `merge_with_gamesys()` function combines both sources

3. **Verify property data**:
   - Property chunks have 8-character names (P$Position, P$Scripts, etc.)
   - Each property has a length prefix and binary data
   - Property parsing must exactly match Dark Engine format

### 2D Screen Layouts (`*R.BIN` widget rects)

Each frontend/HUD screen in `res/intrface/` pairs a `*.PCX` backdrop with a `<SCREEN>R.BIN`
**layout file** giving the pixel-perfect widget positions — so don't eyeball overlay
coordinates, read them from the BIN. Examples: `LOADING.PCX` + `loadingr.BIN` (loading
screen: disc, bar, status text), `MAIN.PCX` + `MAINR.BIN` (main menu: 6 buttons + corner),
`NEWGAME.PCX` + `NEWGAMER.BIN`, `OPTION*.PCX` + `OPTION*R.BIN`, `GAMELODR/GAMESAVR.BIN`.

Format: a header-less list of **LTRB `int16` rectangles** in the 640×480 UI canvas, 8 bytes
each (`file_size / 8` = widget count), in a screen-defined order. Load via
`dark::importers::UI_LAYOUT_IMPORTER` → `Vec<MapRect>` (same binary format as the
map-position files). See `shock2vr/src/scenes/loading.rs` for a usage example, and
`projects/loading-screen.md` §6.1 for the loading-screen breakdown.

Quick peek (decode the rects directly):

```bash
python3 -c "import struct,sys; d=open(sys.argv[1],'rb').read(); v=struct.unpack('<%dh'%(len(d)//2),d); print([(v[i],v[i+1],v[i+2]-v[i],v[i+3]-v[i+1]) for i in range(0,len(v),4)])" /path/to/res/intrface/loadingr.BIN
```

### Entity System Reference

See `references/entities.md` for comprehensive documentation of:

- Template inheritance mechanisms
- Property types and data formats
- Link types and their purposes
- File format specifications
- Code architecture details

## Development Commands

### Building

- Desktop: `cd runtimes/desktop_runtime && cargo run --release`
- Quest VR: `cd runtimes/oculus_runtime && source ./set_up_android_sdk.sh && cargo apk run --release`

### Cargo Aliases

For faster development, the project includes convenient cargo aliases (defined in `.cargo/config.toml`):

- `cargo dr` - Desktop runtime (shorthand for `cargo run -p desktop_runtime --`)
- `cargo dq` - Dark query CLI tool (shorthand for `cargo run -p dark_query --`)
- `cargo dv` - Dark viewer tool (shorthand for `cargo run -p dark_viewer --`)
- `cargo dbgr` - Debug runtime with HTTP control (shorthand for `cargo run -p debug_runtime --`)
- `cargo dbgc` - Debug command client (shorthand for `cargo run -p debug_command --`)
- `cargo bn` - Benchmark CLI (shorthand for `cargo run --release -p bench --`; `bench` collides with the built-in cargo command)

Example usage:
```bash
cargo dr --release --experimental teleport
cargo dq entities earth.mis --filter "*Door*" --limit 10
cargo dv grunt_p.bin
```

**Note**: These aliases only work for desktop development. Android builds still require the full `cargo apk` commands.

**For agents**: Do not use `cargo dr` - it opens an interactive window that requires user intervention to close. Use the debug runtime (`cargo dbgr`) instead, which can be programmatically controlled and shut down via HTTP.

### Experimental Features

The project supports experimental flags for gating in-progress features during development:

#### Using Experimental Flags

- Add `--experimental` flag followed by feature names when running desktop runtime
- Example: `cargo run -- --experimental teleport`
- Multiple features: `cargo run -- --experimental teleport,feature2`

#### Available Experimental Features

- **`teleport`**: VR teleport movement system
  - Enables point-and-teleport locomotion for VR comfort
  - Alternative to smooth movement that can cause motion sickness
  - Triggered via controller trigger button

- **`ragdoll`**: spawn a physics ragdoll on creature death (`SlayEntity`) instead of
  just removing the entity. Without it, death is unchanged.

- **`ragdoll_multibody`**: make ragdolls use reduced-coordinate (multibody) joints
  anchored at the limb articulation points — limbs can't separate (no hip gap), but
  the rig is experimental (extremities can jitter on floor contact). Without it,
  ragdolls use the stable impulse-joint rig. See `projects/ragdoll-settling-followup.md`.

- **`loading_screen`**: show the animated loading screen during level transitions
  (`GlobalEffect::TransitionLevel` / `TestReload`) instead of switching instantly. The
  transition is deferred: the outgoing scene is saved, the `LoadingScene` renders for a
  brief minimum, then the (currently synchronous) load runs. The `DebugReloadLevel` input
  action reloads the current level in place to exercise this. See
  `projects/loading-screen.md`. Without it, transitions are synchronous and unchanged.

#### Adding New Experimental Features

1. **Gate the feature in code**:

   ```rust
   if options.experimental_features.contains("feature_name") {
       // Enable feature logic
   }
   ```

2. **Update this documentation** to list the new experimental feature

### Code Quality

- Check code: `cargo check`
- Format code: `cargo fmt`
- Lint code: `cargo clippy`
- Run tests: `cargo test`

### Build Validation

**MANDATORY: Core crates must compile before committing any changes.**

#### CI Compiles with `-D warnings`

The Build & Unit Test workflow sets `RUSTFLAGS="-D warnings"`, so **any warning (including `dead_code`) fails CI** even though it compiles locally. Validate with CI's flags before pushing:

```bash
RUSTFLAGS="-D warnings" cargo check -p shock2vr -p desktop_runtime -p debug_runtime
```

#### Always Scope to Packages (`-p`)

Do not run bare `cargo check`/`cargo build` at the workspace root on desktop - the workspace includes Android-only crates (`oculus_runtime` → `ndk-sys`, `oboe-sys`) that fail to compile for non-Android targets. CI scopes every build per-package for the same reason.

#### Standard Validation

For most changes to core crates (`shock2vr`, `dark`, `engine`):

```bash
cargo check -p shock2vr
```

This validates the main game logic without requiring platform-specific setup.

#### Runtime-Specific Validation

Only validate specific runtimes when you've made changes to them:

```bash
# Desktop runtime (if changes made to runtimes/desktop_runtime)
cargo check -p desktop_runtime

# Oculus runtime (if changes made to runtimes/oculus_runtime)
cd runtimes/oculus_runtime
source ./set_up_android_sdk.sh
cargo apk check
```

**Note**: The oculus runtime requires Android SDK setup. Only validate it when making oculus-specific changes.

## Incremental Change Process

### 1. Research Phase

- Read relevant source files and understand data flow
- Check `references/` folder for technical specifications
- Understand existing patterns and conventions

### 2. Planning Phase

- Break the change into 2-3 small, logical steps maximum
- Identify which files need modification
- Plan testing approach for each increment
- Consider VR performance implications:
  - Quest hardware constraints require efficient code
  - Analyze rendering paths for frame rate optimization
  - Review memory usage patterns

### 3. Implementation Phase

- Make minimal changes to achieve one specific goal
- If there are issues found, like a bug or potential refactoring, that are outside of the scope of the current goal, you _MAY_ open an issue with enough details to make it actionable in a separate pass.
- It may be necessary to create one-off CLI tools to exercise functionality - feel free to add these as part of the PR. This is especially useful when a change requires understanding game metadata (the .gam or .mis files) - querying the data with existing parsing tools helps with understanding the format.
- Follow existing code patterns and naming conventions
- **Run `cargo check -p shock2vr` after each logical group of changes** (see Build Validation for CI's exact flags)

### 4. Validation Phase

- Validate per **Build Validation** above - never commit code that doesn't compile warning-free
- Test core functionality on desktop (debug runtime / SDK for anything needing a running game)
- Verify VR compatibility if changes affect rendering
- Update documentation if architectural changes were made

## Special Considerations for Trait/Interface Changes

When modifying trait definitions or function signatures:

1. **Change the trait definition first**
2. **Immediately run `cargo check` to identify ALL affected implementations**
3. **Fix each implementation before proceeding**
4. **Re-run `cargo check` after each fix to ensure progress**
5. **Only commit when compilation is successful**

This pattern prevents leaving the codebase in a broken state and ensures all implementations stay in sync.

## Data Path Management

The project includes a centralized data path management system to handle platform-specific data locations:

### Using `shock2vr::paths::data_root()`

**ALWAYS use `shock2vr::paths::data_root()` instead of hardcoded "Data/" paths.**

```rust
use shock2vr::paths;

// ✅ Correct - uses data_root() helper
let motiondb_path = paths::data_root().join("motiondb.bin");
let error_msg = format!("File not found under {}/res/motions", paths::data_root().display());

// ❌ Incorrect - hardcoded paths
let motiondb_path = "Data/motiondb.bin";
let motiondb_path = "../../Data/motiondb.bin";
```

### How `data_root()` Works

- **Desktop**:
  1. First checks `DARK_ASSET_PATH` environment variable if set
  2. Then searches `["./Data", "../Data", "../../Data", "."]` for sentinel files (`shock2.gam`, `motiondb.bin`, etc.)
  3. Falls back to `"../../Data"` if no sentinel files found
- **Android**: Returns `/mnt/sdcard/shock2quest`

### Environment Variable

Set `DARK_ASSET_PATH` to point to your data directory for multi-repo development:

```bash
export DARK_ASSET_PATH=/path/to/your/shock2/data
cargo run -p dark_query -- entities
```

**Note**: The `engine` crate cannot depend on `shock2vr`, so `engine/src/gl_engine.rs` keeps its hardcoded path.

## Testing

- Make sure, when adding a test that exercises code in a PR, to do a _negative_ test first - it should fail without the necessary change. Then, validate the code change makes it green
- For behavior that needs a running game (level loading, AI, input, gameplay), write an SDK scenario test in `tools/shock2-sdk/test/*.e2e.test.ts` (gated behind `SHOCK2_E2E=1`) - see "Iterating on Visual Features" above

### Run the full e2e suite before landing a change

**Before landing any non-trivial change, run the full SDK e2e suite** (skip only
for trivial / docs / formatting edits):

```bash
cd tools/shock2-sdk && npm run test:e2e
```

CI runs only the fast `npm test` (unit tests); the e2e suite is **opt-in
(`SHOCK2_E2E=1`) and is NOT run by CI**, so a green CI does not mean the game
still loads or runs. `test/missions.e2e.test.ts` launches every mission in
`Data/` and is the safety net for changes to parsing, level loading, entity
instantiation, or serialization - exactly the class of change that can compile
and pass unit tests while breaking every mission at runtime (e.g. a property
parser reading the wrong byte count). If the full suite is too slow to run every
iteration, at minimum run `missions.e2e.test.ts` plus any e2e test covering the
system you touched, and run the full suite once before you consider the change
done.
