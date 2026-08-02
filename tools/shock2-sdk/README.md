# @shock2vr/sdk

Playwright-style TypeScript SDK for driving the shock2vr [debug runtime](../../projects/debug-runtime.md)
programmatically. Designed for automation scripts, regression tests, and
coding agents that need to "play" the game without a human at the keyboard.

Zero runtime dependencies — uses native `fetch` and `node:child_process`
(Node 20+).

## Quick start

```bash
cd tools/shock2-sdk
npm install
npm test            # unit tests
npm run test:e2e    # launches a real debug runtime against medsci1.mis
```

```ts
import { GameServer } from "@shock2vr/sdk";

// Spawns `cargo run -p debug_runtime`, waits for /v1/health, captures logs.
// `await using` shuts the runtime down automatically at scope exit.
await using game = await GameServer.launch({
  mission: "medsci1.mis",
  port: 8091,
});

// The game starts paused; advance it explicitly.
await game.step({ frames: 10 });
await game.step({ duration: "1s" });

// Player
const pos = await game.player.position();
await game.player.teleport({ x: pos.x + 5, y: pos.y, z: pos.z });

// Debug provisioning: establish a starting loadout a scenario doesn't hand you
// (e.g. a "Marine" playstyle in a mid-game level). Items are addressed by
// template - name or the stable template id - and land in the backpack exactly
// as a pickup does; only genuine pickup items are accepted. Wield one by
// double-clicking it in the use-mode inventory strip.
const shotgun = await game.player.spawnItem("Shotgun");
await game.player.spawnItem(-18); // Assault Rifle, by stable template id

// The character sheet, mirroring the read side (`(await game.info()).player.stats`).
// Every field is optional and names the level to establish; provisioning only
// raises (a lower target, or one above the cap, throws 400).
await game.player.setStats({
  strength: 3,
  skills: { standard_weapons: 4 },
  psi_tier: 2,
  cyber_modules: 20,
});
// NB: the sheet is persistent storage, but only some of it drives gameplay
// today (Hack + cyber_affinity gate hacking; the weapon proficiencies and psi
// tier are not yet wired to any derived effect). Provisioning them establishes
// the character, not a behavior change.

// Entities
const doors = await game.entities.list({ filter: "*Door*", limit: 10 });
const detail = await game.entities.detail(doors.entities[0].id);

// Inject a script message into an entity (damage, frob, AI signal)
await game.entities.sendMessage(doors.entities[0].id, { type: "Damage", amount: 5 });
await game.entities.sendMessage(doors.entities[0].id, { type: "Frob" });

// Discrete input actions (the keybinding system)
await game.input.actions(); // ["PathfindingTestCycle", "QuickSave", ...]
await game.input.trigger("PathfindingTestCycle");

// Continuous input channels
await game.input.set("right_hand.trigger_value", 1.0);

// Aim through the production camera/weapon/interaction path. Creatures select
// a classified live hitbox; doors/buttons select their nearest visible surface.
// Prefer this over hand-composing head.rotation: raw head controls are
// pawn-local, so mission spawns and loaded saves can rotate their world result.
const aim = await game.player.aimAt(target, {
  hitbox: "torso",
  visibility: "required",
});
// aim reports the selected owner/proxy/body/joint/world point, view LOS, and
// whether the production-equivalent interaction ray confirmed the requested
// target. If every matching proxy is blocked it throws AimOcclusionError, whose
// structured result identifies the blocker. `fallback_used` only describes
// hitbox classification; it never means the target is visible.
//
// Visibility is checked from the flat camera eye (`origin: "view"`). It does
// not guarantee that the offset weapon muzzle is clear, so step and verify the
// shot/target state instead of treating a successful aim as proof of damage.
//
// For ordinary objects, { hitbox: "center" } means the visible selectable
// surface on the center ray; the authored position is only a fallback. It still
// means the authored center for a classified creature.
await game.input.set("right_hand.squeeze_value", 1); // frob highlighted target
await game.step({ frames: 2 });
await game.input.set("right_hand.squeeze_value", 0);

// Verify state instead of scraping logs
const status = await game.pathfindingTest.status();
// => { state: "WaitingForGoal", test_path_waypoints: 0 }

// Polling assertions
await game.waitFor(
  async () => (await game.pathfindingTest.status()).state === "ShowingPath",
  { timeoutMs: 5000, description: "path to be computed" },
);

// Screenshots and physics
await game.screenshot("test.png");
await game.raycast({ start: [0, 0, 0], end: [10, 0, 0] });

// Recently played environmental sounds (resolved schema sample + query tags) -
// the headless way to assert audio, e.g. weapon impact sounds.
const { sounds } = await game.audio.recent();
```

To attach to a runtime you started yourself (`cargo dbgr -- --mission ... --port 8080`):

```ts
const game = await GameServer.connect("http://127.0.0.1:8080");
```

## Notes

- `GameServer.launch` finds the cargo workspace by walking up from `cwd`;
  pass `repoRoot` to override. First launch may take minutes while cargo
  compiles; the default readiness timeout is 5 minutes.
- **Lifecycle / reaping stale runtimes**: a long automation session that dies
  without reaching `/v1/shutdown` (a crashed agent, a killed test runner)
  leaves its `debug_runtime` orphaned - still bound to its port and burning a
  CPU core. To guard against that, `launch()` reaps (SIGKILL) any
  `debug_runtime` already bound to the requested port *before* starting a new
  one, by default. This only ever targets the exact port you asked for
  (`port ?? 8080`) - never a fallback port picked by the bind-race retry loop,
  and never a process that isn't a `debug_runtime` by command line, and never
  a port a live `GameServer` in *this* process already owns (that one is
  skipped as "not stale," not killed). It does **not** distinguish an actual
  orphan from another agent's currently-running session that happens to share
  the same port - if you rely on the existing walk-up-to-a-free-port behavior
  to coexist with concurrent runs on a shared default port, pass an explicit
  unique `port` per run, or set `reapStale: false` to fall back to walk-up
  instead of killing what's there.
- Injected actions apply on the next game update, which runs even while
  paused (with zero delta time).
- `game.logs()` returns recent runtime output (also included in launch
  failure errors). Pass `echoLogs: true` to stream it to stderr.
- If the game thread panics during launch, the error includes the panic
  message and full callstack — `launch()` sets `RUST_BACKTRACE=1` for the
  spawned runtime (export `RUST_BACKTRACE=full` yourself for more frames).
- Writing a new scenario test: copy `test/pathfinding.e2e.test.ts`. Gate
  long-running tests behind `SHOCK2_E2E=1` so `npm test` stays fast.
- The e2e suite runs **serially** (`--test-concurrency=1`): each test spawns its
  own heavy debug runtime, so one-at-a-time avoids port/resource contention
  without hand-syncing ports across files (a unique default port per file is
  still kept as a courtesy for running files individually).
- Reliability runs (catch flakiness in timing-sensitive tests): `npm run
  test:e2e:reliability` runs the e2e suite 10x (also serial). Target one test
  with `node scripts/reliability.mjs <count> "<name pattern>"`, e.g.
  `node scripts/reliability.mjs 20 "muzzle flash"`.
