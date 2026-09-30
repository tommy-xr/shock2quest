import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import { cycleToWeapon } from "./helpers/weapon.js";

function animation(detail: { properties: { name: string; value: string }[] }) {
  const property = detail.properties.find(p => p.name === "FlatWeaponAnimation");
  assert.ok(property, "the first-person mesh has an animation driver");
  return JSON.parse(property.value) as { clip: string | null; frame: number; parameters: [number, number][] };
}

test("flat shotgun ejects during its pump stroke, once per successful shot", {
  skip: process.env.SHOCK2_E2E !== "1",
}, async () => {
  await using game = await GameServer.launch({ mission: "debug_weapons" });
  await game.step({ frames: 10 });
  const gun = await cycleToWeapon(game, e => e.name === "Shotgun", { settleFrames: 120 });
  await game.input.set("right_hand.trigger", 1);
  await game.step({ frames: 1 });
  await game.input.set("right_hand.trigger", 0);
  assert.equal((await game.entities.byTemplate(-2658)).length, 0,
    "the casing stays in the chamber until the pump moves back");
  assert.equal((await game.entities.byTemplate(-2653)).length, 1,
    "the muzzle flash still accompanies the shot immediately");
  await game.step({ frames: 43 });
  assert.equal((await game.entities.byTemplate(-2658)).length, 0);
  await game.step({ frames: 4 });
  assert.equal((await game.entities.byTemplate(-2658)).length, 1,
    "the pump ejects one shell at its authored event");
  assert.ok(animation(await game.entities.detail(gun.id)).parameters.some(([, value]) => Math.abs(value) > 0.1),
    "the first-person renderer receives a moving-part pose at ejection");
  await game.step({ frames: 10 });
  assert.equal((await game.entities.byTemplate(-2658)).length, 1,
    "crossing the event must not emit it again");
  await game.step({ frames: 150 });
  assert.ok(animation(await game.entities.detail(gun.id)).parameters.every(([, value]) => Math.abs(value) < 0.001),
    "the pump returns to rest");
});
