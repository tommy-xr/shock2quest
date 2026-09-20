import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import { acquireOsUpgrade } from "./helpers/os-upgrade.js";
import { fireOnce } from "./helpers/weapon.js";

test("classic Lethal Weapon acquired at a machine increases melee damage after a deck round trip", {
  skip: process.env.SHOCK2_E2E !== "1", timeout: 300_000,
}, async () => {
  await using game = await GameServer.launch({ mission: "earth.mis" });
  await game.step({ frames: 5 });
  await game.player.spawnItem(-928);
  await game.input.trigger("EquipWrench");
  await game.step({ frames: 5 });
  async function shoot() {
    const [droid] = await game.entities.byTemplate(593);
    assert.ok(droid, "authored Earth training droid");
    const [x, y, z] = (await game.entities.detail(droid.id)).position;
    await game.player.teleport({ x: x + 1.2, y: y + 1, z });
    await game.step({ frames: 60 });
    await game.player.aimAt(droid, { hitbox: "torso", visibility: "required" });
    await game.step({ frames: 3 });
    const hp = async () => Number((await game.entities.detail(droid.id)).properties.find(p => p.name === "HitPoints")!.value);
    const before = await hp();
    await fireOnce(game);
    await game.step({ frames: 105 });
    const after = await hp();
    assert.ok(after > 0, "the droid survives so zero-HP clamping cannot distort the comparison");
    assert.ok(before > after);
    return before - after;
  }
  const baseline = await shoot();
  assert.equal(baseline, 6);
  await game.transitionLevel("medsci2.mis");
  await acquireOsUpgrade(game, "Lethal Weapon");
  await game.transitionLevel("earth.mis");
  const upgraded = await shoot();
  assert.equal(upgraded, Math.round(baseline * 1.35), JSON.stringify({ baseline, upgraded }));
  assert.ok(upgraded > baseline);
});
