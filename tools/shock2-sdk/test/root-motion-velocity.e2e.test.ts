import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";

// End-to-end test against a real debug runtime. Requires game assets in
// Data/ and compiles the runtime on first run, so it is opt-in:
//
//   npm run test:e2e        (or SHOCK2_E2E=1 node --test dist/test/)
const e2eEnabled = process.env.SHOCK2_E2E === "1";

test(
  "root motion drives entities at the mocap's per-frame rate, not a constant average",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "medsci1.mis",
    });

    await game.step({ frames: 10 });

    // Spawn a monster, identifying it by diffing the entity list.
    const preSpawn = await game.entities.list({ filter: "OG-Pipe", limit: 50 });
    const known = new Set(preSpawn.entities.map((e) => e.id));
    await game.input.trigger("SpawnDebugMonster");
    await game.step({ frames: 30 });
    const postSpawn = await game.entities.list({ filter: "OG-Pipe", limit: 50 });
    const monster = postSpawn.entities.find(
      (e) => e.name === "OG-Pipe" && !known.has(e.id),
    );
    assert.ok(monster, "expected a newly spawned OG-Pipe");

    // A killing blow interrupts into the death clip immediately, so the
    // whole sampled window is one clip. The death clips carry ~1.8 units of
    // net root motion whose rate varies strongly across the clip (windup,
    // fall, rest) - per-frame root velocity must reproduce that profile,
    // while the old clip-average made the body glide at constant speed.
    await game.entities.sendMessage(monster.id, {
      type: "Damage",
      amount: 1000,
    });

    const positions: [number, number, number][] = [];
    for (let i = 0; i < 34; i++) {
      await game.step({ frames: 15 });
      positions.push((await game.entities.detail(monster.id)).position);
    }

    const speeds: number[] = [];
    for (let i = 1; i < positions.length; i++) {
      const dx = positions[i][0] - positions[i - 1][0];
      const dz = positions[i][2] - positions[i - 1][2];
      speeds.push(Math.hypot(dx, dz));
    }
    const moving = speeds.filter((s) => s > 0.02);
    const total = speeds.reduce((a, b) => a + b, 0);
    assert.ok(
      total > 0.5,
      `expected the death's root motion to move the body (total ${total.toFixed(2)})`,
    );
    assert.ok(
      moving.length >= 3,
      `expected several moving intervals, got ${moving.length}`,
    );

    const mean = moving.reduce((a, b) => a + b, 0) / moving.length;
    const max = Math.max(...moving);
    assert.ok(
      max / mean > 1.6,
      `expected a non-uniform speed profile (max/mean ${(max / mean).toFixed(2)}; ` +
        `a constant-velocity glide is ~1.0)`,
    );
  },
);
