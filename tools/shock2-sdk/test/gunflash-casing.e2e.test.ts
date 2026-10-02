import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer, attachSupportHand } from "../src/index.js";
import type { Vec3 } from "../src/index.js";
import { cycleToWeapon, moveSupportPump } from "./helpers/weapon.js";
import { aimVrHandAt, quatConjugate, quatFromTo, quatRotate } from "./helpers/vr-hand.js";

import type { Quat } from "./helpers/vr-hand.js";

const enabled = process.env.SHOCK2_E2E === "1";
for (const [weapon, casingTemplate] of [["Assault Rifle", -2657], ["Pistol", -2657], ["Shotgun", -2658]] as const) {
  for (const presentation of ["flat", "left", "right"] as const) {
    test(`${weapon} flashes and ejects casings independently in ${presentation}`,
      { skip: enabled ? false : "set SHOCK2_E2E=1 to run" }, async () => {
        const vr = presentation !== "flat";
        const hand = presentation === "left" ? "left" : "right";
        await using game = await GameServer.launch({ mission: "debug_weapons", debugFlags: vr ? ["--vr"] : [] });
        await game.step({ frames: 10 });
        // The pistol draw lasts two seconds; measure ejection from its settled
        // carry pose, not the banked equip gesture.
        const gun = await cycleToWeapon(game, e => e.name === weapon, { settleFrames: 150 });
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
        const spawned = (await game.entities.list()).entities.filter(e => !before.has(e.id));
        const flashes = spawned.filter(e => e.name === "Assault Flash" || e.name === "Shotgun Flash");
        assert.equal(flashes.length, 1, "one visible muzzle flash per shot");
        assert.ok((await game.scene.objects({ entityId: flashes[0].id })).objects.length > 0,
          "the flash must reach the renderer");
        if (vr && weapon === "Shotgun") {
          assert.equal(spawned.filter(e => e.template_id === casingTemplate).length, 0,
            "VR shotgun retains its shell until the physical rear stroke");
          await attachSupportHand(game, hand);
          await moveSupportPump(game, hand, 1);
        }
        const pumping = !vr && weapon === "Shotgun";
        if (pumping) {
          assert.equal(spawned.filter(e => e.template_id === casingTemplate).length, 0,
            "flat shotgun retains its shell until the pump stroke");
          await game.step({ frames: 47 });
        }
        const casings = (await game.entities.list()).entities.filter(e => !before.has(e.id) && e.template_id === casingTemplate);
        assert.equal(casings.length, 1, "one spent casing per shot");
        const casing = casings[0];
        assert.ok((await game.scene.objects({ entityId: casing.id })).objects.length > 0,
          "the casing must reach the renderer, including when it reuses the expired flash's entity slot");
        const initial = casing.position as Vec3;
        if (!vr) {
          const { player } = await game.info();
          const relative = initial.map((v, i) => v - player.position[i]) as Vec3;
          const pawnSpace = quatRotate(quatConjugate(player.rotation), relative);
          const [cw, cx, cy, cz] = player.camera_rotation;
          const eyeSpace = quatRotate([-cx, -cy, -cz, cw],
            pawnSpace.map((v, i) => v - player.camera_offset[i]) as Vec3);
          const halfHeight = -eyeSpace[2] * Math.tan(45 * Math.PI / 360);
          assert.ok(halfHeight > 0 && Math.abs(eyeSpace[0]) < halfHeight * 4 / 3
            && Math.abs(eyeSpace[1]) < halfHeight,
            `casing starts at the visible viewmodel port, not offscreen: ${JSON.stringify(eyeSpace)}`);
        }
        // Both classic and 25AE shell meshes have their long axis along +Y.
        // A level gun must launch that axis sideways, not standing upright.
        const pose = (await game.entities.detail(casing.id)).rotation as Quat;
        const longAxis = quatRotate(pose, [0, 1, 0]);
        assert.ok(pumping || Math.abs(longAxis[1]) < 0.3,
          `casing must lie along the barrel at launch: ${JSON.stringify(longAxis)}`);
        await game.step({ frames: 8 });
        const later = (await game.entities.detail(casing.id)).position as Vec3;
        assert.ok(later[1] - initial[1] > 0.15,
          `authored upward ejection must separate from the breech: ${JSON.stringify({initial, later})}`);
        // With a level -X barrel, authored sideways speed is -0.2 world units/s;
        // it reflects with the handed port and the VR AR's model-axis scale.
        // Allow physics integration.
        if (!pumping) {
          const sideways = later[2] - initial[2];
          const reflected = (hand === "left") !== (vr && weapon === "Assault Rifle");
          assert.ok(reflected ? sideways > 0.01 : sideways < -0.01,
            `casing must leave the ${hand} ejection side: ${sideways}`);
        }
        await game.step({ frames: 60 });
        assert.ok(!(await game.entities.list()).entities.some(e => e.id === casing.id),
          "casing retains its authored effect lifetime");
      });
  }
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
