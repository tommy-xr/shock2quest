import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";

// Animated world textures render on `Game::render_time`, the world clock:
// it advances at `world_time_scale` and stops while the pause menu is up.
//
// Opt-in (compiles the runtime + needs Data/ assets):
//   npm run test:e2e        (or SHOCK2_E2E=1 node --test dist/test/)
const e2eEnabled = process.env.SHOCK2_E2E === "1";

test(
  "render clock follows world time and freezes while paused",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({ mission: "debug_minimal" });
    await game.step({ frames: 10 });
    const advance = async (frames: number) => {
      const before = (await game.step({ frames: 1 })).render_time;
      const after = (await game.step({ frames })).render_time;
      return after - before;
    };
    const near = (actual: number, expected: number, what: string) =>
      assert.ok(Math.abs(actual - expected) < 1e-3, `${what}: ${actual}`);

    near(await advance(60), 1.0, "60 frames at scale 1.0");

    await game.devParams.set("world_time_scale", 0.5);
    near(await advance(60), 0.5, "60 frames at scale 0.5");

    await game.input.trigger("TogglePauseMenu");
    await game.step({ frames: 1 });
    assert.equal((await game.info()).paused, true, "the menu should be up");
    near(await advance(60), 0.0, "60 paused frames");
  },
);
