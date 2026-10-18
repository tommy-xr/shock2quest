import assert from "node:assert/strict";
import { test } from "node:test";

import { launchWithSettings } from "./helpers/settings.js";
import { worldSeconds } from "./helpers/world-clock.js";

// The Speed Boost hypo (template -58) lasts 20 player-seconds. Slow world
// (default) slows the world clock to 0.25 and leaves the player's speed alone;
// Classic doubles the player's stick speed and leaves the world alone.
//
// Opt-in (compiles the runtime + needs Data/ assets):
//   npm run test:e2e        (or SHOCK2_E2E=1 node --test dist/test/)
const e2eEnabled = process.env.SHOCK2_E2E === "1";
const SPEED_BOOST = -58;

for (const mode of ["SlowWorld", "Classic"] as const) {
  test(`Speed Boost hypo in ${mode} mode`, { skip: !e2eEnabled, timeout: 600_000 }, async (t) => {
    await using game = await launchWithSettings(t, { vr: { speed_boosts: mode } }, { mission: "debug_minimal" });
    await game.step({ frames: 10 });
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
    const slowed = await worldSeconds(game, 60);
    if (mode === "SlowWorld") {
      assert.ok(Math.abs(slowed - 0.25) < 1e-3, `world runs at quarter speed: ${slowed}`);
      assert.ok(Math.abs(boosted / baseline - 1) < 0.05, `player speed unchanged: ${boosted} vs ${baseline}`);
    } else {
      assert.ok(Math.abs(slowed - 1) < 1e-3, `world runs at full speed: ${slowed}`);
      assert.ok(Math.abs(boosted / baseline - 2) < 0.1, `player speed doubled: ${boosted} vs ${baseline}`);
    }

    // 20 s on the player's clock, however slow the world.
    await game.step({ frames: 20 * 60 });
    assert.ok(Math.abs((await worldSeconds(game, 60)) - 1) < 1e-3, "the boost wears off");
    assert.ok(Math.abs((await walk()) / baseline - 1) < 0.05, "player speed restored");
  });
}

test("dying ends the speed boost", { skip: !e2eEnabled, timeout: 600_000 }, async (t) => {
  await using game = await launchWithSettings(t, {}, { mission: "debug_minimal" });
  await game.step({ frames: 10 });
  const hypo = await game.player.spawnItem(SPEED_BOOST);
  await game.entities.sendMessage(hypo.entity_id, { type: "Frob" });
  await game.step({ frames: 1 });
  assert.ok(Math.abs((await worldSeconds(game, 60)) - 0.25) < 1e-3, "the boost slows the world");
  await game.entities.sendMessage((await game.info()).player.entity_id!, { type: "Damage", amount: 1000 });
  await game.step({ frames: 2 });
  assert.notEqual((await game.info()).player.life_state, "alive");
  assert.ok(Math.abs((await worldSeconds(game, 60)) - 1) < 1e-3, "death ends the boost");
});

// Slowed time lowers the jump arc's gravity: the hypo's 0.5 doubles the hop.
test("the speed boost doubles the jump in Slow world mode", { skip: !e2eEnabled, timeout: 600_000 }, async (t) => {
  await using game = await launchWithSettings(t, {}, { mission: "debug_minimal" });
  await game.step({ frames: 10 });
  const hop = async () => {
    const start = (await game.player.position()).y;
    let apex = start;
    await game.input.set("jump", 1);
    await game.step({ frames: 2 });
    await game.input.set("jump", 0);
    for (let i = 0; i < 60; i++) {
      await game.step({ frames: 2 });
      apex = Math.max(apex, (await game.player.position()).y);
    }
    return apex - start;
  };
  const normal = await hop();
  assert.ok(normal > 0.5, `the player should hop: ${normal}`);
  const hypo = await game.player.spawnItem(SPEED_BOOST);
  await game.entities.sendMessage(hypo.entity_id, { type: "Frob" });
  await game.step({ frames: 1 });
  const slowed = await hop();
  assert.ok(Math.abs(slowed / normal - 2) < 0.15, `hop doubles: ${normal} -> ${slowed}`);
});
