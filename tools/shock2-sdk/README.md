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
```

To attach to a runtime you started yourself (`cargo dbgr -- --mission ... --port 8080`):

```ts
const game = await GameServer.connect("http://127.0.0.1:8080");
```

## Notes

- `GameServer.launch` finds the cargo workspace by walking up from `cwd`;
  pass `repoRoot` to override. First launch may take minutes while cargo
  compiles; the default readiness timeout is 5 minutes.
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
