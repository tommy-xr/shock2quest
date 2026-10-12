import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";

// Real gamesys pods/scripts on the isolated annelid bench. The camera never
// moves the player into the bench's proximity trigger; activation is explicit.
for (const vr of [false, true]) {
  test(`egg opening is named, one-shot and interrupted by destruction in ${vr ? "VR" : "flat"}`, {
    skip: process.env.SHOCK2_E2E !== "1", timeout: 300_000,
  }, async () => {
    await using game = await GameServer.launch({ mission: "debug_annelid", debugFlags: vr ? ["--vr"] : [] });
    await game.step({ frames: 30 });
    await game.camera.set({ position: [-4, 2, 3], lookAt: [-8, 0.5, 0] });
    let [pod] = await game.entities.byTemplate(-1335);
    assert.ok(pod);
    const shapes = async () => (await game.scene.objects({ entityId: pod!.id })).objects
      .filter((o) => o.model?.toLowerCase() === "eggop");
    await game.entities.sendMessage(pod.id, { type: "TurnOn" });
    await game.step({ frames: 6 });
    let objects = await shapes();
    assert.ok(objects.length > 0, "opened shell is drawn");
    for (const object of objects) {
      assert.deepEqual(object.morph_names, ["closed", "wide open", "twisting", "push out"]);
      assert.equal(object.morph_weights!.length, 4);
      assert.ok(object.morph_weights![0]! > 0.4 && object.morph_weights![0]! < 1);
      assert.ok(object.morph_weights![3]! > 0, "authored push-out moves during opening");
    }
    assert.equal((await game.entities.byTemplate(-182)).length, 1, "payload release stays at hatch");
    await game.entities.sendMessage(pod.id, { type: "TurnOn" });
    await game.step({ frames: 130 });
    assert.equal((await game.entities.byTemplate(-182)).length, 1, "retrigger creates no second grub");
    assert.ok((await shapes()).every((o) => o.morph_weights!.every((w) => w === 0)), "finish at authored rest shape");

    // Destroy a different pod during its opening. Destruction must cancel the
    // live script, with no delayed payload and no animation resurrecting it.
    const [other] = await game.entities.byTemplate(-1332);
    assert.ok(other);
    await game.entities.sendMessage(other.id, { type: "TurnOn" });
    await game.step({ frames: 6 });
    const spawned = (await game.entities.byTemplate(-183)).map((e) => e.id);
    assert.equal(spawned.length, 1);
    await game.entities.sendMessage(other.id, { type: "Damage", amount: 1000 });
    await game.step({ frames: 130 });
    assert.equal((await game.entities.byTemplate(-1332)).length, 0);
    assert.deepEqual((await game.entities.byTemplate(-183)).map((e) => e.id), spawned);
  });
}

// Generated debug scenes cannot save. Test hydration with a real Hydro1 pod.
for (const vr of [false, true]) {
  test(`Hydro1 egg resumes its opening after save/load in ${vr ? "VR" : "flat"}`, {
    skip: process.env.SHOCK2_E2E !== "1", timeout: 300_000,
  }, async () => {
    await using game = await GameServer.launch({ mission: "hydro1.mis", debugFlags: vr ? ["--vr"] : [] });
    await game.step({ frames: 1 });
    let [pod] = await game.entities.byTemplate(154);
    assert.ok(pod, "authored Grub Floor Pod 154");
    const [x,y,z] = pod.position;
    const camera = { position: [x,y+1.4,z+1] as [number,number,number], lookAt: [x,y+0.5,z] as [number,number,number] };
    await game.devParams.set("free_camera_cull", 1);
    await game.camera.set(camera);
    const shapes = async () => (await game.scene.objects({ entityId: pod!.id })).objects.filter((o) => o.model?.toLowerCase() === "eggop");
    const beforeCount = (await game.entities.byTemplate(-182)).length;
    await game.entities.sendMessage(pod.id, { type: "TurnOn" });
    await game.step({ frames: 9 });
    const before = (await shapes())[0]?.morph_weights;
    assert.ok(before && before[3]! > 0, "opening visible before saving");
    assert.equal((await game.entities.byTemplate(-182)).length, beforeCount + 1);
    const save = `egg-shapes-${vr ? "vr" : "flat"}-${Date.now()}`;
    await game.save(save);
    await game.load(save);
    await game.step({ frames: 1 });
    [pod] = await game.entities.byTemplate(154);
    assert.ok(pod, "rediscover runtime identity after hydration");
    await game.camera.set(camera);
    await game.step({ frames: 1 });
    const after = (await shapes())[0]?.morph_weights;
    assert.ok(after);
    assert.ok(Math.abs(after[0]! - before[0]!) < 0.08, `resume closed weight ${before[0]} -> ${after[0]}`);
    assert.ok(after[3]! >= before[3]!, "opening advances instead of resetting");
    assert.equal((await game.entities.byTemplate(-182)).length, beforeCount + 1, "hydrate without replaying payload");
    await game.entities.sendMessage(pod.id, { type: "TurnOn" });
    await game.step({ frames: 130 });
    assert.equal((await game.entities.byTemplate(-182)).length, beforeCount + 1, "saved latch prevents duplicate payload");
    assert.ok((await shapes()).every((o) => o.morph_weights!.every((w) => w === 0)));
  });
}
