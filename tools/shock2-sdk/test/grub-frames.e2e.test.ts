import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";

for (const vr of [false, true]) {
  test(`authored grub model frames run at 20 Hz in ${vr ? "VR" : "flat"}`, {
    skip: process.env.SHOCK2_E2E !== "1", timeout: 300_000,
  }, async () => {
    await using game = await GameServer.launch({ mission: "debug_annelid", debugFlags: vr ? ["--vr"] : [] });
    await game.step({ frames: 1 });
    const [pod] = await game.entities.byTemplate(-1335);
    assert.ok(pod);
    await game.entities.sendMessage(pod.id, { type: "TurnOn" });
    await game.step({ frames: 180 });
    const [grub] = await game.entities.byTemplate(-182);
    assert.ok(grub);
    const detail = await game.entities.detail(grub.id);
    const [x, y, z] = detail.position;
    await game.camera.set({ position: [x + 1, y + 1, z + 1], lookAt: [x, y, z] });
    const beforeBodies = (await game.physics.bodies({ entityId: grub.id })).bodies;
    const frames: number[] = [];
    for (let i = 0; i < 21; i++) {
      await game.step({ frames: 3 });
      const objects = (await game.scene.objects({ entityId: grub.id })).objects;
      assert.ok(objects.length > 0, "the actual hatched entity is rendered");
      const names = [...new Set(objects.map(o => o.model))];
      assert.equal(names.length, 1);
      assert.match(names[0]!, /^NDgrub\d+$/, "draw shipped model frames instead of legacy grub3 joints");
      frames.push(Number(names[0]!.slice(6)));
    }
    assert.equal(new Set(frames.slice(0, 20)).size, 20);
    for (let i = 1; i < frames.length; i++) assert.equal(frames[i], (frames[i - 1]! + 1) % 20);
    const afterBodies = (await game.physics.bodies({ entityId: grub.id })).bodies;
    assert.deepEqual(afterBodies.map(b => b.body_id), beforeBodies.map(b => b.body_id), "frame changes preserve actor bodies");
    assert.equal((await game.entities.detail(grub.id)).aim_points?.length, detail.aim_points?.length);
    await game.entities.sendMessage(grub.id, { type: "Damage", amount: 1000 });
    await game.step({ frames: 3 });
    assert.equal((await game.entities.byTemplate(-182)).length, 0);
    assert.equal((await game.entities.byTemplate(-2666)).length, 1, "retain authored flinders, not commented ND corpse patch");
  });
}

for (const vr of [false, true]) {
  test(`Hydro1 grub frames survive save/load in ${vr ? "VR" : "flat"}`, {
    skip: process.env.SHOCK2_E2E !== "1", timeout: 300_000,
  }, async () => {
    await using game = await GameServer.launch({ mission: "hydro1.mis", debugFlags: vr ? ["--vr"] : [] });
    await game.step({ frames: 1 });
    const [pod] = await game.entities.byTemplate(154);
    assert.ok(pod);
    await game.entities.sendMessage(pod.id, { type: "TurnOn" });
    await game.step({ frames: 180 });
    let [grub] = await game.entities.byTemplate(-182);
    assert.ok(grub);
    await game.devParams.set("free_camera_cull", 1);
    const frame = async () => {
      const { position: [x, y, z] } = await game.entities.detail(grub!.id);
      await game.camera.set({ position: [x + 1, y + 1, z + 1], lookAt: [x, y, z] });
      await game.step({ frames: 1 });
      const objects = (await game.scene.objects({ entityId: grub!.id })).objects;
      assert.ok(objects.length > 0);
      assert.match(objects[0]!.model!, /^NDgrub\d+$/);
      return Number(objects[0]!.model!.slice(6));
    };
    const before = await frame();
    const save = `grub-frames-${vr ? "vr" : "flat"}-${Date.now()}`;
    await game.save(save);
    await game.load(save);
    await game.step({ frames: 1 });
    [grub] = await game.entities.byTemplate(-182);
    assert.ok(grub, "rediscover hydrated runtime identity");
    const after = await frame();
    assert.ok((after - before + 20) % 20 <= 1, `simulation phase continues across load: ${before} -> ${after}`);
    assert.equal((await game.entities.detail(grub.id)).aim_points?.length, 4);
    assert.ok((await game.physics.bodies({ entityId: grub.id })).bodies.length > 0);
    await game.step({ frames: 3 });
    const next = (await game.scene.objects({ entityId: grub.id })).objects[0]!.model!;
    assert.equal(Number(next.slice(6)), (after + 1) % 20, "hydrated grub keeps advancing");
  });
}
