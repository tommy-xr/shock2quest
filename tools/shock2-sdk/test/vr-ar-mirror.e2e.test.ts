import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import { muzzleFrameOf } from "./helpers/weapon.js";

for (const hand of ["left", "right"] as const) test(`AR VR reflection (${hand}) keeps the barrel and renderer consistent`, {
  skip: process.env.SHOCK2_E2E !== "1", timeout: 180_000,
}, async () => {
  await using game = await GameServer.launch({ mission: "debug_weapons", debugFlags: ["--vr"] });
  await game.step({ frames: 10 });
  await game.input.set(`${hand}_hand.squeeze`, 1);
  const gun = await game.player.spawnItem(-18, { hand });
  await game.input.set(`${hand}_hand.position`, [hand === "left" ? -.3 : .3, 1.2, -.6]);
  await game.input.set(`${hand}_hand.rotation`, [0, 0, 0, 1]);
  await game.step({ frames: 5 });
  assert.equal(hand === "left" ? (await game.info()).player.wielded_entity_id : (await game.info()).player.right_hand_entity_id, gun.entity_id);
  const draws = (await game.scene.objects({ entityId: gun.entity_id })).objects;
  assert.ok(draws.length > 0);
  const culling = draws.map(d => d.backface_culling).filter(w => w !== null);
  assert.ok(culling.length > 0);
  assert.ok(culling.every(w => w === (hand === "right" ? "CounterClockwise" : "Clockwise")), JSON.stringify(culling));
  const muzzle = muzzleFrameOf(await game.entities.detail(gun.entity_id));
  assert.ok(muzzle.forward[2] < -.99, "reflection cannot reverse the barrel");
});

test("flat AR keeps its authored winding", { skip: process.env.SHOCK2_E2E !== "1", timeout: 180_000 }, async () => {
  await using game = await GameServer.launch({ mission: "debug_weapons" });
  await game.step({ frames: 10 });
  const gun = await game.player.spawnItem(-18);
  await game.input.trigger("EquipAssaultRifle");
  await game.step({ frames: 150 });
  const draws = (await game.scene.objects({ entityId: gun.entity_id })).objects;
  assert.ok(draws.length > 0);
  const culling = draws.map(d => d.backface_culling).filter(w => w !== null);
  assert.ok(culling.length > 0 && culling.every(w => w === "Clockwise"));
});
