import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import type { SceneObjectSummary } from "../src/types.js";

const options = { skip: process.env.SHOCK2_E2E !== "1", timeout: 600_000 };
for (const vr of [false, true]) {
  test(`retail LGMD plants animate per instance with stable collision in ${vr ? "VR" : "flat"}`, options, async () => {
    await using game = await GameServer.launch({ mission: "medsci2.mis", debugFlags: vr ? ["--vr"] : [] });
    await game.player.teleport({ x: 48, y: 0, z: -21 });
    await game.input.set("head.look", [0,0]);
    await game.camera.set({ position: [46.4,1.7,-20.5], lookAt: [50.35,0.6,-19.3] });
    // Authored pots settle onto the bar. Capture only after that physical motion.
    await game.step({ frames: 180 });
    const plants = (await game.entities.list({ filter: "Plant #2", limit: 100 })).entities
      .filter((e) => [715,716].includes(e.template_id!));
    assert.equal(plants.length,2,"discover both authored instances by stable mission identity");
    const before = await Promise.all(plants.map((e) => game.physics.bodies({ entityId:e.id })));
    assert.ok(before.every((b)=>b.bodies.length>0),"plants have real collision bodies");
    const snapshots: number[][][] = [];
    for (const frames of [1,60]) {
      const step = await game.step({ frames });
      const weights: number[][] = [];
      for (const plant of plants) {
        const [x,,z] = plant.position!;
        const objects = (await game.scene.objects({ entityId: plant.id })).objects;
        const shapes = objects.filter((o) => o.morph_weights);
        assert.ok(shapes.length > 0,`named shapes retained on ${plant.name}`);
        const value = step.new_total_time * 1000 + (-x + z) * 2.5;
        const expected = [4000,600,1000].map((period) => Math.sin(value / period) / 2);
        for (const object of shapes) {
          object.morph_weights!.forEach((weight,i) => {
            assert.ok(Math.abs(weight - expected[i]!) < 0.015,
              `authored weight ${weight} versus ${expected[i]} at ${step.new_total_time}`);
          });
        }
        weights.push([...shapes[0]!.morph_weights!]);
      }
      assert.notDeepEqual(weights[0],weights[1],"shared model retains each instance's phase");
      snapshots.push(weights);
    }
    assert.notDeepEqual(snapshots[0],snapshots[1],"simulation stepping drives shape weights");
    for (let i=0;i<plants.length;i++) {
      const after = await game.physics.bodies({ entityId:plants[i]!.id });
      const rest = (b: typeof after) => b.bodies.map((body) => ({ id:body.body_id, position:body.position, rotation:body.rotation }));
      assert.deepEqual(rest(after),rest(before[i]!),"shape motion must not move collision bodies");
    }
  });
}

for (const vr of [false,true]) {
  test(`retail two-subobject egg depositor contracts in ${vr ? "VR" : "flat"}`, options, async () => {
    await using game = await GameServer.launch({ mission:"many.mis", debugFlags:vr ? ["--vr"]:[] });
    // This point is inside the authored Base Room. Teleporting into the
    // depositor's wall leaves the player outside portal cells and culls it.
    await game.player.teleport({ x:71.6,y:27.3,z:176.4 });
    await game.input.set("head.look",[0,0]);
    await game.camera.set({ position:[71.6,27.3,176.4],lookAt:[75.692444,25.492779,172.61142] });
    const depositor = (await game.entities.list({filter:"Egg Depositor",limit:100})).entities.find((e)=>e.template_id===110);
    assert.ok(depositor,"discover the authored depositor, not an ephemeral runtime id");
    const before = await game.physics.bodies({entityId:depositor.id});
    const weights: number[] = [];
    for (const frames of [180,120]) {
      await game.step({frames});
      const objects: SceneObjectSummary[] = (await game.scene.objects({entityId:depositor.id})).objects;
      const shapes = objects.filter((o)=>o.model?.toLowerCase()==="orifice" && o.morph_weights);
      assert.ok(shapes.length>0,"LGMD contract target reaches the skinned draw path");
      for (const object of shapes) assert.deepEqual(object.morph_weights,shapes[0]!.morph_weights);
      weights.push(shapes[0]!.morph_weights![2]);
    }
    assert.notEqual(weights[0],weights[1],"authored third-channel contract weight advances");
    const after = await game.physics.bodies({entityId:depositor.id});
    const rest = (b:typeof after)=>b.bodies.map((body)=>({id:body.body_id,position:body.position,rotation:body.rotation}));
    assert.deepEqual(rest(after),rest(before),"contraction preserves collision");
  });
}
