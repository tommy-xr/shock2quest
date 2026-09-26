import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";

for (const vr of [false, true]) {
  test(`temporary stats affect pools and expire at ten seconds (${vr ? "VR" : "flat"})`, {
    skip: process.env.SHOCK2_E2E !== "1", timeout: 180_000,
  }, async () => {
    await using game = await GameServer.launch({ mission: "debug_psi", debugFlags: vr ? ["--vr"] : [] });
    await game.step({ frames: 30 });
    const before = (await game.info()).player;
    assert.equal(before.stats!.strength, 6);
    assert.equal(before.effective_stats!.strength, 6);
    for (const stat of ["strength", "endurance", "psionic_ability"] as const) {
      await game.player.applyStatModifier({ source: `test:${stat}`, stat, delta: 2, duration_secs: 10 });
    }
    const active = (await game.info()).player;
    assert.equal(active.stats!.strength, 6, "the trained sheet stays unchanged");
    assert.equal(active.effective_stats!.strength, 8, "temporary bonuses exceed the training cap");
    assert.ok(active.max_hit_points! > before.max_hit_points!);
    assert.equal(active.max_hit_points! - active.hit_points!, before.max_hit_points! - before.hit_points!);
    assert.equal(active.max_psi_points, before.max_psi_points, "retail psi capacity uses base PSI");
    await game.step({ frames: 599 });
    assert.equal((await game.info()).player.effective_stats!.strength, 8);
    await game.step({ frames: 1 });
    const expired = (await game.info()).player;
    assert.equal(expired.effective_stats!.strength, 6);
    assert.equal(expired.max_hit_points, before.max_hit_points);
    assert.equal(expired.hit_points, before.hit_points);
    assert.equal(expired.stats!.modifiers.length, 0);
    await assert.rejects(game.player.applyStatModifier({ source: "invalid", stat: "strength", delta: 2, duration_secs: -1 }));
    assert.equal((await game.info()).player.effective_stats!.strength, 6);
  });
}

test("temporary Strength survives save/load and spills overflow when it expires", {
  skip: process.env.SHOCK2_E2E !== "1", timeout: 600_000,
}, async () => {
  await using game = await GameServer.launch({ mission: "earth.mis" });
  await game.step({ frames: 30 });
  const before = (await game.info()).player;
  assert.equal(before.stats!.strength, 1);
  await game.player.applyStatModifier({ source: "test:capacity", stat: "strength", delta: 2, duration_secs: 10 });
  // A wrench occupies a full grid column. Strength 1 has ten columns;
  // temporary Strength 3 has twelve. No item may disappear on shrink.
  const created = [];
  for (let i = 0; i < 12; i++) created.push(await game.player.spawnItem(-928));
  await assert.rejects(game.player.spawnItem(-928), "the expanded backpack has a finite capacity");
  await game.step({ frames: 120 });
  const saveName = `e2e_timed_stats_${Date.now()}`;
  await game.save(saveName);
  await game.load(saveName);
  const loaded = (await game.info()).player;
  assert.equal(loaded.stats!.strength, 1);
  assert.equal(loaded.effective_stats!.strength, 3);
  assert.ok(loaded.stats!.modifiers[0]!.remaining.secs >= 7);
  await game.step({ frames: 490 });
  assert.equal((await game.info()).player.effective_stats!.strength, 1);
  const carried = (await game.player.inventory()).items;
  assert.equal(carried.length, 10);
  const allWrenches = await game.entities.byTemplate(-928);
  assert.equal(allWrenches.length, created.length, "overflow spills instead of being destroyed");
});
