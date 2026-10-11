import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";

// End-to-end test for scale tweqs (`P$CfgTweqScale`/`P$StTweqScale`).
//
//   npm run test:e2e        (or SHOCK2_E2E=1 node --test dist/test/)
//
// Negative-first: the scale tweq was unparsed, so hydro1's worm goo stayed at
// its authored size. Its tweq breathes the two horizontal (Dark x/y) axes
// between 1.0 and 1.05 at 0.002 per 100 ms - 0.02 per second - and leaves the
// up axis alone. Runtime ids change per launch; goo is found by name.
const e2eEnabled = process.env.SHOCK2_E2E === "1";

test(
  "hydro1: worm goo breathes on its horizontal axes at the authored rate",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({ mission: "hydro1.mis" });
    await game.step({ frames: 5 });
    const goo = (await game.entities.list({ filter: "Worm Goo", limit: 200 })).entities.sort(
      (a, b) => a.distance - b.distance,
    )[0];
    assert.ok(goo, "hydro1 should place worm goo near the start");
    // Look down at the floor so the goo is drawn and reported by /v1/scene.
    await game.input.set("head.look", [0, 60]);

    const renderedScale = async () => {
      await game.step({ frames: 1 });
      const [object] = (await game.scene.objects({ entityId: goo.id })).objects;
      assert.ok(object, "the goo should be drawn");
      return object.scale;
    };

    const start = await renderedScale();
    await game.step({ frames: 60 });
    const later = await renderedScale();
    // One second at 0.02/s, from wherever the first frame left it.
    assert.ok(Math.abs(later[0] - start[0] - 0.02) < 0.002, `x grew ${later[0] - start[0]}`);
    assert.ok(Math.abs(later[2] - start[2] - 0.02) < 0.002, `z grew ${later[2] - start[2]}`);
    assert.equal(later[1], 1, "the up axis is not tweqed");

    // It bounces between its limits rather than growing without bound.
    const samples: number[] = [];
    for (let i = 0; i < 12; i++) {
      await game.step({ frames: 30 });
      samples.push((await renderedScale())[0]);
    }
    assert.ok(Math.max(...samples) <= 1.05 + 1e-4, `peaked at ${Math.max(...samples)}`);
    assert.ok(Math.min(...samples) >= 1.0 - 1e-4, `dipped to ${Math.min(...samples)}`);
    assert.ok(Math.max(...samples) - Math.min(...samples) > 0.03, "and keeps moving");
  },
);
