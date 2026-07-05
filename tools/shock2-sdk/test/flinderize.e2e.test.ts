import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { PhysicsBodySummary, Vec3 } from "../src/types.js";

// End-to-end test against a real debug runtime. Requires game assets in
// Data/ and compiles the runtime on first run, so it is opt-in:
//
//   npm run test:e2e        (or SHOCK2_E2E=1 node --test dist/test/)
const e2eEnabled = process.env.SHOCK2_E2E === "1";

// medsci1 "Window 1" entities inherit from "Breakable Windows" (-1587), which
// carries 4 Flinderize links (Glass1..Glass4 shards with scatter=true and
// impulses 2/4/10/14). Slaying one must spawn shards scattered across the
// window's bounds and flying apart - not stacked at the center at rest.
const TARGET_NAME = "Window 1";

function horizontalSpeed(v: Vec3): number {
  return Math.hypot(v[0], v[2]);
}

function maxPairwiseDistance(bodies: PhysicsBodySummary[]): number {
  let max = 0;
  for (let i = 0; i < bodies.length; i++) {
    for (let j = i + 1; j < bodies.length; j++) {
      const [ax, ay, az] = bodies[i].position;
      const [bx, by, bz] = bodies[j].position;
      max = Math.max(max, Math.hypot(bx - ax, by - ay, bz - az));
    }
  }
  return max;
}

test(
  "Flinderize: slaying a breakable window scatters shards with an impulse",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "medsci1.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8112),
    });

    // Let the mission load and initialize scripts.
    await game.step({ frames: 2 });

    const { entities } = await game.entities.list({
      filter: TARGET_NAME,
      limit: 500,
    });
    const window = entities.find((e) => e.name === TARGET_NAME);
    assert.ok(window !== undefined, `expected to find a '${TARGET_NAME}' entity`);

    const before = await game.physics.bodies();
    const knownBodyIds = new Set(before.bodies.map((b) => b.body_id));

    // Breakable windows have 1 HP; one hit slays and flinderizes.
    await game.entities.sendMessage(window.id, { type: "Damage", amount: 5.0 });
    await game.step({ frames: 1 });

    // Identify shards by name, not just body novelty: body_id is a bare arena
    // index (reusable), and unrelated dynamic bodies could spawn the same frame.
    const after = await game.physics.bodies();
    const shards = after.bodies.filter(
      (b) =>
        !knownBodyIds.has(b.body_id) &&
        b.body_type === "dynamic" &&
        b.entity_name?.startsWith("Glass"),
    );

    // The window's 4 Flinderize links each spawn one shard.
    assert.ok(
      shards.length >= 4,
      `expected 4 spawned shard bodies (one per Flinderize link), got ${shards.length}`,
    );

    // scatter=true: shards spawn spread across the window's bounds, not all
    // at the object center.
    const spread = maxPairwiseDistance(shards);
    assert.ok(
      spread > 0.3,
      `expected shards scattered over the window (max pairwise distance > 0.3), got ${spread.toFixed(3)}`,
    );

    // Each link carries an impulse: one frame in, shards must be flying, not
    // just starting to free-fall (gravity alone gives ~0 horizontal speed).
    const fastest = Math.max(...shards.map((b) => horizontalSpeed(b.velocity)));
    assert.ok(
      fastest > 0.3,
      `expected at least one shard with horizontal speed > 0.3, got ${fastest.toFixed(3)}`,
    );
  },
);
