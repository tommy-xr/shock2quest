import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import { acquireOsUpgrade } from "./helpers/os-upgrade.js";

test("Power Psi acquired at a machine prevents burnout HP loss after transition", {
  skip: process.env.SHOCK2_E2E !== "1", timeout: 240_000,
}, async () => {
  await using game = await GameServer.launch({ mission: "medsci2.mis" });
  await game.step({ frames: 5 });
  await acquireOsUpgrade(game, "Power Psi");
  await game.transitionLevel("medsci1.mis");
  await game.player.spawnItem(-247);
  await game.input.trigger("EquipPsiAmp");
  await game.step({ frames: 10 });
  const before = (await game.info()).player;
  assert.ok(before.stats!.os_traits.includes(14));
  assert.ok(before.psi_points! > 0);
  await game.input.set("right_hand.trigger", 1);
  await game.step({ frames: 130 });
  const after = (await game.info()).player;
  assert.equal(after.psi_charge_phase, "burnout");
  assert.equal(after.psi_points, before.psi_points! - 1);
  assert.equal(after.hit_points, before.hit_points);
  await game.input.set("right_hand.trigger", 0);
});
