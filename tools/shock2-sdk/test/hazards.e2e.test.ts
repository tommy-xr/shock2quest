import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import { pullTrigger } from "./helpers/weapon.js";

const enabled = process.env.SHOCK2_E2E === "1";

test("hazards: toxin persists, weaker exposure does not stack, and Endurance mitigates ticks", { skip: !enabled, timeout: 120_000 }, async () => {
  await using game = await GameServer.launch({ mission: "medsci1.mis" });
  await game.step({ frames: 2 });
  const initial = (await game.info()).player;
  assert.ok(initial.entity_id !== null);
  await game.entities.sendMessage(initial.entity_id, { type: "Hazard", toxin: true, amount: 4 });
  await game.step({ frames: 1 });
  await game.entities.sendMessage(initial.entity_id, { type: "Hazard", toxin: true, amount: 2 });
  await game.step({ frames: 1 });
  assert.equal((await game.info()).player.toxin_level, 4);
  const hp = (await game.info()).player.hit_points!;
  await game.step({ frames: 601 });
  const poisoned = (await game.info()).player;
  assert.equal(poisoned.toxin_level, 4);
  assert.equal(poisoned.hit_points, hp - 4);
  await game.player.setStats({ endurance: 6 });
  await game.step({ frames: 600 });
  assert.equal((await game.info()).player.toxin_level, 4);
  assert.equal((await game.info()).player.hit_points, hp - 5, "poison retains its one-point minimum at END 6");
  const patch = await game.player.spawnItem("Detox Patch");
  await game.entities.sendMessage(patch.entity_id, { type: "Frob" });
  await game.step({ frames: 2 });
  assert.equal((await game.info()).player.toxin_level, 2);
  assert.ok(!(await game.player.inventory()).items.some(item => item.entity_id === patch.entity_id));
  const suit = await game.player.spawnItem(-83);
  await game.entities.sendMessage(suit.entity_id, { type: "Frob" });
  await game.step({ frames: 2 });
  await game.entities.sendMessage(initial.entity_id, { type: "Hazard", toxin: true, amount: 12 });
  await game.step({ frames: 2 });
  assert.equal((await game.info()).player.toxin_level, 3, "Vacc Suit reduces new toxin exposure by 75%");
  const save = `hazards-${Date.now()}`;
  await game.save(save);
  const cure = await game.player.spawnItem("Detox Patch");
  await game.entities.sendMessage(cure.entity_id, { type: "Frob" });
  await game.step({ frames: 2 });
  assert.equal((await game.info()).player.toxin_level, 1);
  await game.load(save);
  assert.equal((await game.info()).player.toxin_level, 3);
  const restoredPlayer = (await game.info()).player.entity_id!;
  await game.entities.sendMessage(restoredPlayer, { type: "Hazard", toxin: true, amount: 16 });
  await game.step({ frames: 2 });
  assert.equal((await game.info()).player.toxin_level, 4, "equipped suit survives entity remapping on load");
});


test("hazards: authored Engineering room accumulates radiation only while occupied", { skip: !enabled, timeout: 180_000 }, async () => {
  await using game = await GameServer.launch({ mission: "eng1.mis" });
  await game.step({ frames: 2 });
  const start = (await game.info()).player.position;
  // ROOM_DB object 226 has a generated forwarding sensor and a marker at zero.
  // Discover its runtime sensor rather than hardcoding a per-launch entity id.
  const room = (await game.entities.list()).entities.find(e => e.template_id === 226 && e.name === "Base Room");
  assert.ok(room);
  await game.player.teleport({ x: room.position[0], y: room.position[1] - .5, z: room.position[2] });
  await game.step({ frames: 60 });
  const exposed = (await game.info()).player.radiation_level;
  assert.ok(exposed > .5, `room sensor must accumulate radiation, got ${exposed}`);
  await game.player.teleport({ x: start[0], y: start[1], z: start[2] });
  await game.step({ frames: 60 });
  assert.equal((await game.info()).player.radiation_level, exposed, "exit removes ambient exposure without curing stored radiation");
  const cleanup = (await game.entities.list({ filter: "RadBeGone" })).entities.find(e => e.template_id === 178);
  assert.ok(cleanup);
  await game.entities.sendMessage(cleanup.id, { type: "TurnOn" });
  await game.step({ frames: 2 });
  assert.equal(await game.quests.get("EngineRadClear"), "incomplete", "retail sets the raw quest flag to 1");
  const afterCleanup = (await game.info()).player.radiation_level;
  assert.equal(afterCleanup, exposed, "reactor cleanup does not cure the player");
  await game.player.teleport({ x: room.position[0], y: room.position[1] - .5, z: room.position[2] });
  await game.step({ frames: 60 });
  assert.equal((await game.info()).player.radiation_level, afterCleanup, "reactor cleanup disables subsequent room exposure");

});


test("hazards: casting Toxin Shield prevents new contamination", { skip: !enabled, timeout: 120_000 }, async () => {
  await using game = await GameServer.launch({ mission: "debug_psi" });
  await game.step({ frames: 10 });
  for (let i = 0; i < 40; i++) {
    if ((await game.info()).player.selected_psi_power === "Toxin Shield") break;
    await game.input.trigger("CyclePsiPower");
    await game.step({ frames: 1 });
  }
  assert.equal((await game.info()).player.selected_psi_power, "Toxin Shield");
  await pullTrigger(game);
  await game.step({ frames: 10 });
  const player = (await game.info()).player;
  assert.ok(player.active_psi_powers.includes("Toxin Shield"));
  await game.entities.sendMessage(player.entity_id!, { type: "Hazard", toxin: true, amount: 10 });
  await game.step({ frames: 2 });
  assert.equal((await game.info()).player.toxin_level, 0);
  await game.step({ frames: 3000 });
  assert.ok(!(await game.info()).player.active_psi_powers.includes("Toxin Shield"));
  await game.entities.sendMessage(player.entity_id!, { type: "Hazard", toxin: true, amount: 3 });
  await game.step({ frames: 2 });
  assert.equal((await game.info()).player.toxin_level, 3, "protection ends with the sustained power");
});
