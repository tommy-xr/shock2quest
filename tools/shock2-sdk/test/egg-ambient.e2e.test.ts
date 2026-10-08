import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";

// Retail Eggs (-1474) inherits AmbientHacked -> eggloop, which selects one
// of egglp1/2/3 and loops seamlessly. BaseEgg only plays pod_exp on TurnOn;
// opening the shell does not remove its ambient property.
for (const vr of [false, true]) {
  for (const mission of ["debug_annelid", "hydro1.mis"]) {
    test(`egg ambience follows the pod across hatching and range changes (${mission}, ${vr ? "VR" : "flat"})`, {
      skip: process.env.SHOCK2_E2E !== "1", timeout: 300_000,
    }, async () => {
      await using game = await GameServer.launch({ mission, port: 0, debugFlags: vr ? ["--vr"] : [] });
      await game.step({ frames: 1 });
      const [pod] = await game.entities.byTemplate(mission === "debug_annelid" ? -1476 : 326);
      assert.ok(pod);
      // Stay outside the gallery's proximity hatch trigger.
      const near = { x: pod.position[0]! + 5, y: pod.position[1]! + 1, z: pod.position[2]! };
      await game.player.teleport(near);
      await game.step({ frames: 2 });
      const before = (await game.audio.loops()).loops.find(loop => loop.entity_id === pod.id);
      assert.ok(before, "the pod's inherited ambient emitter must reach the audio engine");
      assert.equal(before.owner, "ambient_emitter");
      assert.match(before.sample, /^egglp[123]\.wav$/i);
      assert.equal(before.paused, false);

      await game.entities.sendMessage(pod.id, { type: "TurnOn" });
      await game.step({ frames: 10 });
      const opened = (await game.audio.loops()).loops.find(loop => loop.entity_id === pod.id);
      assert.equal(opened?.handle, before.handle, "hatching must not stop or restart the hum");
      assert.equal(opened?.sample, before.sample);

      await game.player.teleport({ x: 1000, y: 1000, z: 1000 });
      await game.step({ frames: 2 });
      assert.ok(!(await game.audio.loops()).loops.some(loop => loop.entity_id === pod.id));
      await game.player.teleport(near);
      await game.step({ frames: 2 });
      const returned = (await game.audio.loops()).loops.find(loop => loop.entity_id === pod.id);
      assert.ok(returned, "returning to the egg restarts its ambient loop");
      assert.notEqual(returned.handle, before.handle);
      assert.match(returned.sample, /^egglp[123]\.wav$/i);
    });
  }
}
