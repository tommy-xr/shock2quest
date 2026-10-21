import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
for (const vr of [false, true]) {
  test(`authored monster worm idle advances independently in ${vr ? "VR" : "flat"}`, {
    skip: process.env.SHOCK2_E2E !== "1", timeout: 240_000,
  }, async () => {
    await using game = await GameServer.launch({ mission: "debug_hitbox", debugFlags: vr ? ["--vr"] : [] });
    await game.step({ frames: 1 });
    await game.input.trigger("SpawnDebugMonster");
    await game.step({ frames: 2 });
    const hybrids = await game.entities.byTemplate(-397);
    assert.equal(hybrids.length, 2);
    const worm = async (id: number) => {
      const objects = (await game.scene.objects({ entityId: id })).objects.filter(o => o.morph_names?.includes("worm"));
      assert.ok(objects.length > 0);
      return objects[0]!.morph_weights?.[objects[0]!.morph_names!.indexOf("worm")] ?? 0;
    };
    const before = await Promise.all(hybrids.map(e => worm(e.id)));
    await game.step({ frames: 20 });
    const after = await Promise.all(hybrids.map(e => worm(e.id)));
    assert.notDeepEqual(after, before, "production driver must advance the authored worm shape");
    assert.notEqual(after[0], after[1], "same model instances retain independent phases");
  });
}

for (const vr of [false, true]) {
  test(`authored rumbler worm idle runs on a real Command1 actor in ${vr ? "VR" : "flat"}`, {
    skip: process.env.SHOCK2_E2E !== "1", timeout: 240_000,
  }, async () => {
    await using game = await GameServer.launch({ mission: "command1.mis", debugFlags: vr ? ["--vr"] : [] });
    await game.step({ frames: 1 });
    const rumblers = (await game.entities.list({ filter: "Rumbler", limit: 20 })).entities;
    assert.ok(rumblers.length > 0);
    const rumbler = rumblers.find(e => e.template_id === 2313)!;
    assert.ok(rumbler?.position);
    const [x, y, z] = rumbler.position;
    await game.player.teleport({ x: x + 3, y: y + 2, z });
    await game.camera.set({ position: [x + 3, y + 2, z + 3], lookAt: [x, y, z] });
    await game.step({ frames: 2 });
    const weights = async () => {
      const objects = (await game.scene.objects({ entityId: rumbler.id })).objects.filter(o => o.morph_names?.includes("worm"));
      assert.ok(objects.length > 0, "actual Rumbler family model has the authored worm target");
      return objects.map(o => o.morph_weights?.[o.morph_names!.indexOf("worm")] ?? 0);
    };
    const before = await weights();
    await game.step({ frames: 20 });
    assert.notDeepEqual(await weights(), before);
    await game.entities.sendMessage(rumbler.id, { type: "Damage", amount: 1000 });
    await game.step({ frames: 2 });
    const dead = await weights();
    await game.step({ frames: 20 });
    assert.deepEqual(await weights(), dead, "dead actors retain the last deformation");
  });
}
