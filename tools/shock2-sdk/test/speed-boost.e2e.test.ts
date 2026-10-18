import assert from "node:assert/strict";
import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";

import { GameServer } from "../src/index.js";

// The Speed Boost hypo (template -58) lasts 20 player-seconds. Slow world
// (default) halves the world clock and leaves the player's speed alone;
// Classic doubles the player's stick speed and leaves the world alone.
//
// Opt-in (compiles the runtime + needs Data/ assets):
//   npm run test:e2e        (or SHOCK2_E2E=1 node --test dist/test/)
const e2eEnabled = process.env.SHOCK2_E2E === "1";
const SPEED_BOOST = -58;

for (const mode of ["SlowWorld", "Classic"] as const) {
  test(`Speed Boost hypo in ${mode} mode`, { skip: !e2eEnabled, timeout: 600_000 }, async () => {
    const root = await mkdtemp(join(tmpdir(), "shock2-speed-boost-"));
    const previous = process.env.SHOCK2_SETTINGS_PATH;
    process.env.SHOCK2_SETTINGS_PATH = join(root, "settings.json");
    await writeFile(process.env.SHOCK2_SETTINGS_PATH, JSON.stringify({ vr: { speed_boosts: mode } }));
    try {
      await using game = await GameServer.launch({ mission: "debug_minimal" });
      await game.step({ frames: 10 });
      const worldSeconds = async (frames: number) => {
        const before = (await game.step({ frames: 1 })).render_time;
        return (await game.step({ frames })).render_time - before;
      };
      // Distance covered by a full forward stick; then return to the start.
      const walk = async () => {
        const start = await game.player.position();
        await game.input.set("right_hand.thumbstick", [0, 1]);
        await game.step({ frames: 30 });
        await game.input.set("right_hand.thumbstick", [0, 0]);
        const end = await game.player.position();
        await game.player.teleport(start);
        await game.step({ frames: 10 });
        return Math.hypot(end.x - start.x, end.z - start.z);
      };
      const baseline = await walk();
      assert.ok(baseline > 0.5, `the player should walk: ${baseline}`);

      const hypo = await game.player.spawnItem(SPEED_BOOST);
      await game.entities.sendMessage(hypo.entity_id, { type: "Frob" });
      await game.step({ frames: 1 });
      assert.equal((await game.player.inventory()).count, 0, "using the hypo consumes it");

      const boosted = await walk();
      const slowed = await worldSeconds(60);
      if (mode === "SlowWorld") {
        assert.ok(Math.abs(slowed - 0.5) < 1e-3, `world runs at half speed: ${slowed}`);
        assert.ok(Math.abs(boosted / baseline - 1) < 0.05, `player speed unchanged: ${boosted} vs ${baseline}`);
      } else {
        assert.ok(Math.abs(slowed - 1) < 1e-3, `world runs at full speed: ${slowed}`);
        assert.ok(Math.abs(boosted / baseline - 2) < 0.1, `player speed doubled: ${boosted} vs ${baseline}`);
      }

      // 20 s on the player's clock, however slow the world.
      await game.step({ frames: 20 * 60 });
      assert.ok(Math.abs((await worldSeconds(60)) - 1) < 1e-3, "the boost wears off");
      assert.ok(Math.abs((await walk()) / baseline - 1) < 0.05, "player speed restored");
    } finally {
      if (previous === undefined) delete process.env.SHOCK2_SETTINGS_PATH;
      else process.env.SHOCK2_SETTINGS_PATH = previous;
      await rm(root, { recursive: true, force: true });
    }
  });
}

test("dying ends the speed boost", { skip: !e2eEnabled, timeout: 600_000 }, async () => {
  await using game = await GameServer.launch({ mission: "debug_minimal" });
  await game.step({ frames: 10 });
  const hypo = await game.player.spawnItem(SPEED_BOOST);
  await game.entities.sendMessage(hypo.entity_id, { type: "Frob" });
  await game.step({ frames: 1 });
  const worldSeconds = async (frames: number) => {
    const before = (await game.step({ frames: 1 })).render_time;
    return (await game.step({ frames })).render_time - before;
  };
  assert.ok(Math.abs((await worldSeconds(60)) - 0.5) < 1e-3, "the boost slows the world");
  await game.entities.sendMessage((await game.info()).player.entity_id!, { type: "Damage", amount: 1000 });
  await game.step({ frames: 2 });
  assert.notEqual((await game.info()).player.life_state, "alive");
  assert.ok(Math.abs((await worldSeconds(60)) - 1) < 1e-3, "death ends the boost");
});
