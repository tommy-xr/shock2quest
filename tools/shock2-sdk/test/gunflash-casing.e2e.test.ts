import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import type { Vec3 } from "../src/index.js";
import { cycleToWeapon } from "./helpers/weapon.js";
import { aimVrHandAt, quatFromTo } from "./helpers/vr-hand.js";

const enabled = process.env.SHOCK2_E2E === "1";
for (const presentation of ["flat", "left", "right"] as const) {
  test(`AR15 casings launch independently in ${presentation}`,
    { skip: enabled ? false : "set SHOCK2_E2E=1 to run" }, async () => {
      const vr = presentation !== "flat";
      const hand = presentation === "left" ? "left" : "right";
      await using game = await GameServer.launch({ mission: "debug_weapons", debugFlags: vr ? ["--vr"] : [] });
      await game.step({ frames: 10 });
      const gun = await cycleToWeapon(game, e => e.name === "Assault Rifle", { settleFrames: 90 });
      if (vr) {
        await aimVrHandAt(game, gun.position as Vec3, 0.45, 1, 0, { hand });
        await game.step({ frames: 8 });
        const info = await game.info();
        assert.equal(hand === "right" ? info.player.right_hand_entity_id : info.player.wielded_entity_id, gun.id);
        await game.input.set(`${hand}_hand.position`, [0, 1, -2]);
        await game.input.set(`${hand}_hand.rotation`, quatFromTo([0, 0, -1], [-1, 0, 0]));
        await game.step({ frames: 3 });
      }
      const before = new Set((await game.entities.list()).entities.map(e => e.id));
      await game.input.set(`${hand}_hand.trigger`, 1);
      await game.step({ frames: 1 });
      await game.input.set(`${hand}_hand.trigger`, 0);
      const casing = (await game.entities.list()).entities.find(e => !before.has(e.id) && e.template_id === -2657);
      assert.ok(casing, "the authored casing GunFlash link must spawn an object");
      const initial = casing.position as Vec3;
      await game.step({ frames: 8 });
      const later = (await game.entities.detail(casing.id)).position as Vec3;
      assert.ok(later[1] - initial[1] > 0.15,
        `authored upward ejection must separate from the breech: ${JSON.stringify({initial, later})}`);
      // With a level -X barrel, authored sideways speed is -0.2 world units/s;
      // it reflects with the left-hand ejection port. Allow physics integration.
      {
        const sideways = later[2] - initial[2];
        assert.ok(hand === "left" ? sideways > 0.01 : sideways < -0.01,
          `casing must leave the ${hand} ejection side: ${sideways}`);
      }
      await game.step({ frames: 60 });
      assert.ok(!(await game.entities.list()).entities.some(e => e.id === casing.id),
        "casing retains its authored effect lifetime");
    });
}

// Debug scenes have no mission file to reload; exercise saves in a real mission.
test("flat AR15 casings stay transient across save/load", { skip: !enabled, timeout: 180_000 }, async () => {
  await using game = await GameServer.launch({ mission: "earth.mis" });
  await game.step({ frames: 30 });
  await game.player.setStats({ skills: { standard_weapons: 6 } });
  await game.player.spawnItem("Assault Rifle");
  await game.input.trigger("EquipAssaultRifle");
  await game.step({ frames: 5 });
  await game.input.set("right_hand.trigger", 1);
  await game.step({ frames: 1 });
  await game.input.set("right_hand.trigger", 0);
  assert.ok((await game.entities.list()).entities.some(e => e.template_id === -2657));
  const save = `casing_fx_${Date.now()}`;
  assert.equal((await game.save(save)).success, true);
  assert.equal((await game.load(save)).success, true);
  assert.ok(!(await game.entities.list()).entities.some(e => e.template_id === -2657),
    "short-lived casings must not return after loading");
  const gun = (await game.info()).player.wielded_entity_id;
  assert.ok(gun);
  assert.ok((await game.entities.detail(gun)).properties.some(p => p.name === "Model" && p.value === "ar15_h"));
});
