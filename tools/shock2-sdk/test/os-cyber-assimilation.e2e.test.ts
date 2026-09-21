import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import { acquireOsUpgrade } from "./helpers/os-upgrade.js";

for (const owned of [false, true]) test(`Cyber-Assimilation robot loot and healing (owned=${owned})`, {
  skip: process.env.SHOCK2_E2E !== "1", timeout: 240_000,
}, async () => {
  await using game = await GameServer.launch({ mission: owned ? "medsci2.mis" : "eng1.mis" });
  await game.step({ frames: 5 });
  if (owned) {
    await acquireOsUpgrade(game, "Cyber-Assimilation");
    await game.transitionLevel("eng1.mis");
  }
  const [robot] = await game.entities.byTemplate(1056);
  assert.ok(robot, "authored Protocol Droid on Engineering deck");
  const before = new Set((await game.entities.byTemplate(-436)).map(e => e.id));
  await game.entities.sendMessage(robot.id, { type: "Damage", amount: 1000 });
  await game.step({ frames: 120 });
  const drops = (await game.entities.byTemplate(-436)).filter(e => !before.has(e.id));
  assert.equal(drops.length, owned ? 1 : 0, "the upgrade alone grants the guaranteed robot module");
  if (!owned) return;
  const module = drops[0];
  await game.player.give(module.id);
  const player = (await game.info()).player;
  const wound = Math.min(20, player.hit_points! - 1);
  assert.ok(wound > 0);
  await game.entities.sendMessage(player.entity_id!, { type: "Damage", amount: wound });
  await game.step({ frames: 1 });
  const damaged = (await game.info()).player;
  assert.equal(damaged.hit_points, player.hit_points! - wound);
  await game.entities.sendMessage(module.id, { type: "Frob" });
  await game.step({ frames: 1 });
  assert.equal((await game.info()).player.hit_points, Math.min(damaged.hit_points! + 15, damaged.max_hit_points!));
  assert.ok(!(await game.player.inventory()).items.some(e => e.entity_id === module.id));
  assert.ok(!(await game.entities.byTemplate(-436)).some(e => e.id === module.id));
});
