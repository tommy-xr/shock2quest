import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";

const enabled = process.env.SHOCK2_E2E === "1";

test("Survive grows swarmer and black eggs; black eggs hatch once and can be destroyed safely", {
  skip: !enabled, timeout: 300_000,
}, async () => {
  await using game = await GameServer.launch({ mission: "earth_horde_test" });
  await game.devParams.set("horde_growth_seconds", 30);
  await game.devParams.set("horde_tech_enabled", 0);
  await game.step({ frames: 1 });
  const [ready] = (await game.entities.list({ filter: "Containment - Ready" })).entities;
  assert.ok(ready);
  await game.entities.sendMessage(ready.id, { type: "Frob" });
  // Stay above the arena while real combat time drives growth. Repositioning
  // each half-second prevents falling into pods or dying to wave enemies.
  for (let i = 0; i < 100; i++) {
    await game.player.teleport({ x: 10, y: 50, z: 30 });
    await game.step({ frames: 30 });
  }
  assert.ok((await game.entities.byTemplate(-1332)).length > 0, "containment must produce swarmer pods");
  const blackEggs = await game.entities.byTemplate(-3474);
  assert.ok(blackEggs.length >= 2, "multiple zones must produce rare black eggs");
  const slot = `horde_eggs_${Date.now()}`;
  assert.equal((await game.save(slot)).success, true);
  assert.equal((await game.load(slot)).success, true);
  // Discover again: save/load is allowed to remap runtime entity IDs.
  const eggs = await game.entities.byTemplate(-3474);
  assert.equal(eggs.length, blackEggs.length, "unopened black eggs survive loading");
  const before = new Set((await game.entities.byTemplate(-2014)).map(e => e.id));
  const victim = eggs[0]!;
  await game.entities.sendMessage(victim.id, { type: "Damage", amount: 100 });
  await game.step({ frames: 5 });
  assert.ok(!(await game.entities.byTemplate(-3474)).some(e => e.id === victim.id));
  assert.deepEqual(new Set((await game.entities.byTemplate(-2014)).map(e => e.id)), before,
    "destroying an unopened black egg must not release a spider");
  const hatch = eggs[1]!;
  const [x, y, z] = hatch.position;
  await game.player.teleport({ x: x! + 2, y: y!, z: z! });
  await game.step({ frames: 35 });
  assert.ok(!(await game.entities.byTemplate(-3474)).some(e => e.id === hatch.id), "hatching consumes the black shell");
  const spiders = (await game.entities.byTemplate(-2014)).filter(e => !before.has(e.id));
  assert.equal(spiders.length, 1, "one black egg releases exactly one baby spider");
  assert.ok((await game.entities.byTemplate(-4647)).length > 0, "the shell bursts into its authored fragments");
  await game.player.teleport({ x: 10, y: 50, z: 30 });
  assert.equal((await game.save(slot)).success, true);
  assert.equal((await game.load(slot)).success, true);
  await game.step({ frames: 35 });
  assert.equal((await game.entities.byTemplate(-2014)).length, before.size + 1,
    "loading a hatched egg must not duplicate its payload");
  assert.equal((await game.entities.byTemplate(-3474)).length, eggs.length - 2);
});
