import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import { acquireOsUpgrade } from "./helpers/os-upgrade.js";

test("Smasher waits for release and adds six base damage after its retail charge time", {
  skip: process.env.SHOCK2_E2E !== "1", timeout: 300_000,
}, async () => {
  await using game = await GameServer.launch({ mission: "medsci2.mis" });
  await acquireOsUpgrade(game, "Smasher");
  await game.transitionLevel("earth.mis");
  await game.player.spawnItem(-928);
  await game.input.trigger("EquipWrench");
  await game.step({ frames: 5 });
  const [droid] = await game.entities.byTemplate(593);
  assert.ok(droid);
  const [x, y, z] = (await game.entities.detail(droid.id)).position;
  await game.player.teleport({ x: x + 1.2, y: y + 1, z });
  await game.step({ frames: 60 });
  await game.player.aimAt(droid, { hitbox: "torso", visibility: "required" });
  await game.step({ frames: 3 });
  const hp = async () => Number((await game.entities.detail(droid.id)).properties.find(p => p.name === "HitPoints")!.value);
  for (const [hold, expected] of [[6, 6], [80, 12]]) {
    const before = await hp();
    await game.input.set("right_hand.trigger", 1);
    await game.step({ frames: hold });
    assert.equal(await hp(), before, "holding the trigger only winds up");
    await game.input.set("right_hand.trigger", 0);
    await game.step({ frames: 120 });
    assert.equal(before - await hp(), expected);
    assert.ok(await hp() > 0, "no zero-HP clamping");
  }
});
