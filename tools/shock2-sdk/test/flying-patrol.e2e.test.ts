import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";

for (const vr of [false, true]) {
  test(`Overlord descends to its authored patrol goal (${vr ? "VR" : "flat"})`,
    { skip: process.env.SHOCK2_E2E !== "1", timeout: 600_000 }, async () => {
      await using game = await GameServer.launch({ mission: "many.mis", debugFlags: vr ? ["--vr"] : [] });
      await game.step({ frames: 2 });
      const creatures = (await game.entities.list({ filter: "Overlord", limit: 100 })).entities;
      const overlord = creatures.find(e => e.template_id === 679)!;
      const neighbors = [729, 453].map(id => creatures.find(e => e.template_id === id)!);
      const moved = new Map<number, number>();
      assert.ok(overlord, "authored Overlord 679 exists");
      const start = overlord.position!;
      let lowest = start[1];
      for (let i = 0; i < 12; i++) {
        await game.step({ frames: 30 });
        const current = await game.entities.detail(overlord.id);
        lowest = Math.min(lowest, current.position![1]);
        for (const creature of neighbors) {
          const at = (await game.entities.detail(creature.id)).position!;
          const before = creature.position!;
          const distance = Math.hypot(at[0] - before[0], at[1] - before[1], at[2] - before[2]);
          moved.set(creature.id, Math.max(moved.get(creature.id) ?? 0, distance));
        }
      }
      for (const creature of neighbors) {
        assert.ok((moved.get(creature.id) ?? 0) > 1,
          `Overlord ${creature.template_id} must traverse its route with flight and ordinary floor links`);
      }
      assert.ok(start[1] - lowest > 1.5,
        `Overlord must descend toward its lower patrol goal: start=${start[1]}, lowest=${lowest}, paths=${JSON.stringify((await game.pathfinding.aiPaths()).filter(p => p.entity_id === overlord.id))}, entity=${JSON.stringify(await game.entities.detail(overlord.id))}`);
    });
}

for (const vr of [false, true]) {
  test(`command2 Overlord investigates a reachable lower sound (${vr ? "VR" : "flat"})`,
    { skip: process.env.SHOCK2_E2E !== "1", timeout: 600_000 }, async () => {
      await using game = await GameServer.launch({ mission: "command2.mis", debugFlags: vr ? ["--vr"] : [] });
      await game.step({ frames: 2 });
      const creature = (await game.entities.list({ filter: "Overlord", limit: 100 }))
        .entities.find(e => e.template_id === 161)!;
      assert.ok(creature);
      const start = creature.position!;
      // Above the room's y=6.2 floor by its authored 3.4-unit hover offset.
      // The native patrol points into a disconnected region; a heard sound
      // supplies a reachable goal through the ordinary investigation behavior.
      await game.entities.sendMessage(creature.id, { type: "HeardNoise", origin: [-9.6, 9.6, 35.2] });
      let lowest = start[1];
      let closest = Infinity;
      for (let i = 0; i < 12; i++) {
        await game.step({ frames: 15 });
        const at = (await game.entities.detail(creature.id)).position!;
        lowest = Math.min(lowest, at[1]);
        closest = Math.min(closest, Math.hypot(at[0] + 9.6, at[1] - 9.6, at[2] - 35.2));
      }
      assert.ok(start[1] - lowest > 0.4, `expected descent to lower sound: ${start[1]} -> ${lowest}`);
      assert.ok(closest < 1.6, `expected to reach the lower sound, closest=${closest}`);
    });
}
