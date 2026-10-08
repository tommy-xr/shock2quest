import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import type { EntityDetailResult } from "../src/index.js";

const enabled = process.env.SHOCK2_E2E === "1";
const property = (detail: EntityDetailResult, name: string) =>
  detail.properties.find((p) => p.name === name)?.value;

test("camera scanning audio stops on death and the shell survives a cold save/load", {
  skip: !enabled, timeout: 900_000,
}, async () => {
  const save = `camera_death_e2e_${Date.now()}`;
  // A retail mission can round-trip saves; identify its camera by authored ID.
  const template = 102;
  {
    await using game = await GameServer.launch({ mission: "medsci1.mis" });
    await game.step({ frames: 120 });
    const [camera] = await game.entities.byTemplate(template);
    assert.ok(camera);
    const motorSounds = (await game.audio.recent()).sounds.filter(
      (s) => s.source_entity?.template_id === template && s.volume_millibels === -2000,
    );
    assert.equal(motorSounds.length, 1, "idle scan starts one positional motor loop");
    const motor = motorSounds[0]!;
    assert.ok(motor.position.every((value, axis) =>
      Math.abs(value - camera.position[axis]!) < 0.00001), "motor plays at camera position");
    assert.equal(motor.pan_applied, false);
    assert.equal(motor.still_playing, true, "loop outlives its first sample");
    assert.ok((await game.audio.loops()).loops.some((s) => s.handle === motor.handle));

    await game.entities.sendMessage(camera.id, { type: "Damage", amount: 1000 });
    await game.step({ frames: 180 });
    assert.equal(property(await game.entities.detail(camera.id), "Model"), "camdam");
    assert.ok(!(await game.audio.loops()).loops.some((s) => s.handle === motor.handle));
    const stopped = (await game.audio.recent()).sounds.find((s) => s.handle === motor.handle);
    assert.ok(stopped?.stopped_at_sim_time !== null);
    assert.equal(stopped?.still_playing, false);
    assert.equal((await game.save(save)).success, true);
  }
  {
    await using game = await GameServer.launch({ mission: "medsci1.mis" });
    assert.equal((await game.load(save)).success, true);
    const [camera] = await game.entities.byTemplate(template);
    assert.ok(camera, "retained camera must exist after load");
    const watermark = Math.max(0, ...(await game.audio.recent()).sounds.map((s) => s.sequence));
    await game.entities.sendMessage(camera.id, { type: "Damage", amount: 1000 });
    await game.step({ frames: 600 });
    assert.equal(property(await game.entities.detail(camera.id), "Model"), "camdam");
    assert.equal((await game.audio.recent()).sounds.filter(
      (s) => s.sequence > watermark && s.source_entity?.template_id === template,
    ).length, 0, "destroyed camera cannot resume motor or speech after load");
  }
});
