import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";

for (const vr of [false, true]) {
  test(`Many vein frames hold and loop without changing collision in ${vr ? "VR" : "flat"}`, {
    skip: process.env.SHOCK2_E2E !== "1", timeout: 300_000,
  }, async () => {
    await using game = await GameServer.launch({ mission: "many.mis", debugFlags: vr ? ["--vr"] : [] });
    await game.step({ frames: 1 });
    // Mission object 463 inherits Vein; rediscover its runtime ID on every launch.
    const [vein] = await game.entities.byTemplate(463);
    assert.ok(vein);
    const detail = await game.entities.detail(vein.id);
    await game.devParams.set("free_camera_cull", 1);
    await game.player.teleport({ x: -46, y: 4, z: -83 });
    await game.camera.set({ position: [-46, 5, -90], lookAt: detail.position });
    const beforeBodies = (await game.physics.bodies({ entityId: vein.id })).bodies;
    assert.ok(beforeBodies.length > 0, "verify an authored colliding vein");
    const frames: number[] = [];
    for (let i = 0; i < 61; i++) {
      await game.step({ frames: 3 });
      const objects = (await game.scene.objects({ entityId: vein.id })).objects;
      assert.ok(objects.length > 0, "the authored Many vein is rendered");
      const names = [...new Set(objects.map(o => o.model))];
      assert.equal(names.length, 1);
      assert.match(names[0]!, /^vein\d+$/, "draw the remaster sequence instead of the static vein");
      frames.push(Number(names[0]!.slice(4)));
    }
    // Any full cycle contains frames 0..18 once and frame 19 eleven times.
    for (const cycle of [frames.slice(0, 30), frames.slice(30, 60)]) {
      assert.deepEqual([...cycle].sort((a, b) => a - b), [
        ...Array.from({ length: 19 }, (_, i) => i), ...Array<number>(11).fill(19),
      ]);
    }
    for (let i = 1; i < frames.length; i++) {
      assert.ok(frames[i - 1] === 19 ? [0, 19].includes(frames[i]!) : frames[i] === frames[i - 1]! + 1);
    }
    assert.deepEqual(frames.slice(0, 31), frames.slice(30), "the 1.5-second cycle repeats exactly");
    const afterBodies = (await game.physics.bodies({ entityId: vein.id })).bodies;
    assert.deepEqual(afterBodies, beforeBodies, "mesh animation preserves authored physics bodies");
    assert.deepEqual((await game.entities.detail(vein.id)).position, detail.position);
  });
}
