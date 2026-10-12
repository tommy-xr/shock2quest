import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";

for (const vr of [false, true]) {
  test(`batched and single-frame stepping reach identical gameplay state (${vr ? "VR" : "flat"})`, {
    skip: process.env.SHOCK2_E2E !== "1", timeout: 180_000,
  }, async () => {
    const states = [];
    for (const batchSize of [1, 1200]) {
      await using game = await GameServer.launch({
        mission: "debug_weapons", debugFlags: vr ? ["--vr"] : [],
      });
      const implant = (await game.player.spawnItem(-101)).entity_id;
      await game.entities.sendMessage(implant, { type: "Frob" });
      await game.step({ frames: 1 });
      const charged = Number((await game.entities.detail(implant)).properties.find(p => p.name === "Energy")?.value);
      assert.ok(charged > 2, "fixture implant must start charged");

      let rendered = 0;
      for (let frame = 0; frame < 1200; frame += batchSize) {
        const step = await game.step({ frames: batchSize });
        assert.equal(step.frames_advanced, batchSize);
        rendered += step.frames_rendered;
      }
      assert.equal(rendered, 1200 / batchSize);
      const info = await game.info();
      const player = info.player!;
      assert.equal(player.life_state, "alive");
      const energy = Number((await game.entities.detail(implant)).properties.find(p => p.name === "Energy")?.value);
      assert.equal(energy, charged - 2, "both ten-second drain ticks must fire");
      // Compare gameplay fields, excluding per-launch entity IDs and wall-clock timings.
      states.push({
        frame: info.frame_index, time: info.time, position: player.position, rotation: player.rotation,
        health: player.hit_points, psi: player.psi_points, stats: player.effective_stats,
        energy,
      });
    }
    assert.deepEqual(states[1], states[0]);
  });

  test(`hidden stepping draws the final frame while advancing every tick (${vr ? "VR" : "flat"})`, {
    skip: process.env.SHOCK2_E2E !== "1", timeout: 180_000,
  }, async () => {
    await using game = await GameServer.launch({
      mission: "debug_weapons", debugFlags: vr ? ["--vr"] : [],
    });
    const implant = (await game.player.spawnItem(-101)).entity_id;
    await game.entities.sendMessage(implant, { type: "Frob" });
    const first = await game.step({ frames: 1 });
    assert.equal(first.frames_rendered, 1);
    const charged = Number((await game.entities.detail(implant)).properties.find(p => p.name === "Energy")?.value);
    assert.ok(charged > 1, "fixture implant must start charged");

    const batch = await game.step({ frames: 120 });
    assert.equal(batch.frames_advanced, 120);
    assert.equal(batch.new_frame_index, first.new_frame_index + 120);
    assert.ok(Math.abs(batch.time_advanced - 2) < 0.001);
    assert.equal(batch.frames_rendered, 1, "intermediate frames need no GPU draw");
    assert.equal((await game.scene.objects()).frame_index, batch.new_frame_index);
    assert.equal((await game.info()).frame_index, batch.new_frame_index);

    // The authored ten-second drain must run even when its intermediate images do not.
    const duration = await game.step({ duration: "10s" });
    assert.ok(Math.abs(duration.time_advanced - 10) < 0.02);
    assert.equal(duration.frames_rendered, 1);
    assert.equal((await game.scene.objects()).frame_index, duration.new_frame_index);
    const remaining = Number((await game.entities.detail(implant)).properties.find(p => p.name === "Energy")?.value);
    assert.equal(remaining, charged - 1);
  });
}
