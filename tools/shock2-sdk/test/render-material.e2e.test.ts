import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import { aimVrHandAt } from "./helpers/vr-hand.js";
import { cycleToWeapon } from "./helpers/weapon.js";

const options = { skip: process.env.SHOCK2_E2E !== "1", timeout: 600_000 };

for (const vr of [false, true]) {
  test(
    `25AE pistol submits its complete authored material in ${vr ? "VR" : "flat"}`,
    options,
    async () => {
      await using game = await GameServer.launch({
        mission: "debug_weapons",
        debugFlags: vr ? ["--vr"] : [],
      });
      await game.step({ frames: 10 });
      const gun = await cycleToWeapon(
        game,
        (entity) => entity.template_id === -17,
        { settleFrames: vr ? 90 : 10 },
      );
      if (vr) {
        await aimVrHandAt(game, gun.position!, 0.45, 1);
        await game.step({ frames: 8 });
        assert.equal((await game.info()).player.right_hand_entity_id, gun.id);
      }
      await game.step({ frames: 10 });
      const objects = (await game.scene.objects({ limit: 2000 })).objects;
      // Check the wielded model, not the unskinned bench pistol: LGMD weapon
      // parts use a skinned shader while their art still belongs to obj/.
      const pistol = objects.filter(
        (object) => object.model?.toLowerCase() === "atek_h" && object.entity_id === gun.id,
      );
      assert.ok(pistol.length > 0, "pistol model is submitted to renderer");
      const material = pistol.find((object) => object.material_passes?.length === 5);
      assert.ok(
        material?.material_only,
        `pistol must retain invis/diffuse/fill/rim/specular order: ${JSON.stringify(pistol)}`,
      );
      assert.deepEqual(material.material_passes, [
        "Authored(SrcAlpha, One)",
        "Authored(SrcAlpha, InvSrcAlpha)",
        "Authored(SrcAlpha, One)",
        "Authored(SrcAlpha, One)",
        "Authored(SrcAlpha, One)",
      ]);
    },
  );
}

test("25AE chemical glass retains ordered modulation, cubemap and rim passes", options, async () => {
  await using game = await GameServer.launch({
    mission: "debug_weapons",
    debugFlags: ["--vr"],
  });
  await game.step({ frames: 10 });
  await game.input.set("right_hand.squeeze", 1);
  const item = await game.player.spawnItem(-140, { hand: "right" });
  await game.step({ frames: 10 });
  assert.equal((await game.info()).player.right_hand_entity_id, item.entity_id);
  const objects = (await game.scene.objects({ entityId: item.entity_id })).objects;
  const glass = objects.find((object) => object.material_passes?.length === 3);
  assert.ok(glass?.material_only, JSON.stringify(objects));
  assert.deepEqual(glass.material_passes, [
    "Authored(DstColor, Zero)",
    "Authored(One, One)",
    "Authored(SrcAlpha, One)",
  ]);
});

test("Command2 authored forcefield retains both unlit layers", options, async () => {
  await using game = await GameServer.launch({ mission: "command2.mis" });
  await game.step({ frames: 10 });
  const doors = (await game.entities.list({ filter: "Comforce Door", limit: 100 })).entities;
  const door = doors.sort((a, b) => a.template_id! - b.template_id!)[0]!;
  assert.ok(door.position);
  const [x, y, z] = door.position;
  await game.player.teleport({ x: x + 2, y, z });
  await game.step({ frames: 1 });
  const objects = (await game.scene.objects({ entityId: door.id })).objects;
  const field = objects.find((object) => object.material_passes?.length === 2);
  assert.ok(field?.material_only, JSON.stringify(objects));
  assert.deepEqual(field.material_passes, [
    "Authored(SrcAlpha, InvSrcAlpha)",
    "Authored(SrcAlpha, One)",
  ]);
});
