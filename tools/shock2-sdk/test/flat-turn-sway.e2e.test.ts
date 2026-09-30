import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import { cycleToWeapon } from "./helpers/weapon.js";

test("flat turn sway moves the viewmodel, preserves aim, and settles", {
  skip: process.env.SHOCK2_E2E !== "1", timeout: 180_000,
}, async () => {
  await using game = await GameServer.launch({ mission: "debug_weapons" });
  await game.step({ frames: 60 });
  const gun = await cycleToWeapon(game, (e) => e.template_id === -17, { settleFrames: 120 });
  const sample = async () => {
    const detail = await game.entities.detail(gun.id);
    const forward = (name: string): number[] => {
      const prop = detail.properties.find((p) => p.name === name);
      assert.ok(prop, name);
      return JSON.parse(prop.value).forward;
    };
    return { muzzle: forward("WeaponMuzzle"), aim: forward("FlatAim") };
  };
  for (let frame = 1; frame <= 60; frame++) {
    await game.input.set("head.look", [frame * 1.5, frame * 0.25]);
    await game.step({ frames: 1 });
  }
  const moving = await sample();
  await game.step({ frames: 180 });
  const settled = await sample();
  assert.ok(moving.muzzle.some((v, i) => Math.abs(v - settled.muzzle[i]) > 0.01),
    `turning should displace the gun: moving=${JSON.stringify(moving)} settled=${JSON.stringify(settled)}`);
  assert.deepEqual(moving.aim, settled.aim, "cosmetic sway must never bend the shot ray");
  await game.step({ frames: 120 });
  const rest = await sample();
  assert.ok(rest.muzzle.every((v, i) => Math.abs(v - settled.muzzle[i]) < 0.001));
});
