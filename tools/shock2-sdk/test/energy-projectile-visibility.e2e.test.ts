import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import type { Vec3 } from "../src/index.js";
import { cycleToWeapon } from "./helpers/weapon.js";
import { aimVrHandAt, quatFromTo } from "./helpers/vr-hand.js";

for (const [weapon, template] of [["Laser Pistol", -2474], ["EMP Rifle", -235]] as const) {
  for (const vr of [false, true]) {
    test(`${weapon} reveals its bolt after the LaserShot delay in ${vr ? "VR" : "flat"}`,
      { skip: process.env.SHOCK2_E2E !== "1" }, async () => {
        await using game = await GameServer.launch({ mission: "debug_weapons", debugFlags: vr ? ["--vr"] : [] });
        await game.step({ frames: 10 });
        const gun = await cycleToWeapon(game, e => e.name === weapon, { settleFrames: 90 });
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
        const shot = (await game.entities.list()).entities.find(e => e.template_id === template);
        assert.ok(shot, "the normal firing path must create its live energy projectile");
        const p = shot.position;
        await game.camera.set({ position: [p[0] + 1, p[1] + 0.5, p[2] + 2], lookAt: p });
        await game.step({ frames: 1 });
        assert.equal((await game.scene.objects({entityId: shot.id})).objects.length, 0,
          "the bolt starts hidden while it clears the muzzle");
        await game.step({ frames: 4 });
        assert.ok((await game.scene.objects({entityId: shot.id})).objects.length > 0,
          "LaserShot must submit its bolt mesh after the 50ms reveal delay");
        const entities = (await game.entities.list()).entities;
        if (template === -235) {
          assert.ok(entities.some(e => e.name === "EMP Blue"), "the authored blue bitmap rider remains");
          assert.ok(entities.some(e => e.name === "EMP2"), "the authored disk rider remains");
        } else {
          assert.ok(entities.some(e => e.name === "Blue Laser Trail"));
        }
      });
  }
}

// Sweep close-wall distances so collision deletion falls around the 50ms
// reveal boundary; a queued render effect must tolerate an already-dead bolt.
test("laser impacts around the reveal frame are safe", {
  skip: process.env.SHOCK2_E2E !== "1",
}, async () => {
  await using game = await GameServer.launch({ mission: "debug_weapons", debugFlags: ["--vr"] });
  await game.step({ frames: 10 });
  const gun = await cycleToWeapon(game, e => e.name === "Laser Pistol", { settleFrames: 90 });
  await aimVrHandAt(game, gun.position as Vec3, 0.45, 1, 0);
  await game.step({ frames: 8 });
  assert.equal((await game.info()).player.right_hand_entity_id, gun.id);
  await game.input.set("right_hand.rotation", quatFromTo([0, 0, -1], [-1, 0, 0]));
  for (const x of [-9, -9.4, -9.8, -10.2]) {
    await game.input.set("right_hand.position", [x, 1, -2]);
    await game.step({ frames: 3 });
    await game.input.set("right_hand.trigger", 1);
    await game.step({ frames: 1 });
    await game.input.set("right_hand.trigger", 0);
    const shot = (await game.entities.list()).entities.find(e => e.template_id === -2474);
    assert.ok(shot, `shot should launch at hand x=${x}`);
    await game.step({ frames: 9 });
    const entities = (await game.entities.list()).entities;
    assert.ok(!entities.some(e => e.id === shot.id), `close-wall shot must impact at x=${x}`);
    assert.ok(entities.some(e => e.template_id === -4276), "wall impact must emit Laser Spang");
    await game.step({ frames: 90 });
  }
});
