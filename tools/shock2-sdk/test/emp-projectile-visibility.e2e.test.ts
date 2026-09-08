import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import type { Vec3 } from "../src/index.js";
import { cycleToWeapon } from "./helpers/weapon.js";
import { aimVrHandAt, quatFromTo } from "./helpers/vr-hand.js";

for (const vr of [false, true]) {
  test(`EMP keeps its hidden root model out of ${vr ? "VR" : "flat"} rendering`,
    { skip: process.env.SHOCK2_E2E !== "1" }, async () => {
      await using game = await GameServer.launch({ mission: "debug_weapons", debugFlags: vr ? ["--vr"] : [] });
      await game.step({ frames: 10 });
      const gun = await cycleToWeapon(game, e => e.name === "EMP Rifle", { settleFrames: 90 });
      if (vr) {
        await aimVrHandAt(game, gun.position as Vec3, 0.45, 1, 0);
        await game.step({ frames: 8 });
        assert.equal((await game.info()).player.right_hand_entity_id, gun.id);
        await game.input.set("right_hand.position", [0, 1, -2]);
        await game.input.set("right_hand.rotation", quatFromTo([0, 0, -1], [-1, 0, 0]));
        await game.step({ frames: 3 });
      }
      await game.input.set("right_hand.trigger", 1);
      await game.step({ frames: 1 });
      await game.input.set("right_hand.trigger", 0);
      const shot = (await game.entities.list()).entities.find(e => e.template_id === -235);
      assert.ok(shot, "the normal firing path must create its live EMP projectile");
      const p = shot.position;
      await game.camera.set({ position: [p[0] + 1, p[1] + 0.5, p[2] + 2], lookAt: p });
      await game.step({ frames: 1 });
      assert.equal((await game.scene.objects({entityId: shot.id})).objects.length, 0,
        "NoRender swingba2 must never be submitted as projectile geometry");
      const entities = (await game.entities.list()).entities;
      assert.ok(entities.some(e => e.name === "EMP Blue"), "the authored blue bitmap rider remains");
      assert.ok(entities.some(e => e.name === "EMP2"), "the authored disk rider remains");
    });
}
