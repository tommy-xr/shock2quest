import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import { e2ePort } from "./helpers/e2e-port.js";

// Explicitly requires a runtime built with shock2vr/lazy-hitboxes. The ordinary
// ranged/melee suites run unchanged against either backend.
const enabled = process.env.SHOCK2_E2E === "1" &&
  process.env.SHOCK2_EXPECT_LAZY_HITBOXES === "1";

test("query-only limbs expose aim points without bodies and are removed at ragdoll handoff",
  { skip: !enabled, timeout: 600_000 }, async () => {
    await using game = await GameServer.launch({
      mission: "debug_melee",
      experimental: ["ragdoll"],
      port: e2ePort(0, "SHOCK2_E2E_LAZY_HITBOXES_PORT"),
    });
    await game.step({ frames: 30 });
    const creatures = await game.entities.list({ filter: "OG-Pipe" });
    assert.ok(creatures.entities.length > 0);
    const creature = creatures.entities[0];
    const detail = await game.entities.detail(creature.id);
    const points = detail.aim_points ?? [];
    assert.ok(points.length > 1, "query-only limbs remain discoverable for aiming");
    const proxies = new Set(points.map((point) => point.proxy_entity_id));
    for (const point of points) {
      assert.equal(point.body_id, null);
      const bodies = await game.physics.bodies({ entityId: point.proxy_entity_id });
      assert.equal(bodies.bodies.length, 0, "no hidden Rapier limb body");
    }
    const [x, y, z] = points[0].position;
    const before = await game.raycast({ start: [x, y, z - 1], end: [x, y, z + 1],
      collision_groups: ["hitbox"] });
    assert.ok(before.hit && before.entity_id !== null && proxies.has(before.entity_id));

    await game.entities.sendMessage(creature.id, { type: "Damage", amount: 10000 });
    await game.step({ frames: 180 });
    assert.ok((await game.physics.ragdolls()).ragdolls.length > 0,
      "death must actually hand off to a ragdoll before checking removed proxies");
    const remaining = await game.entities.list({ filter: "OG-Pipe" });
    assert.ok(!remaining.entities.some((entity) => entity.id === creature.id),
      "ragdoll handoff must remove the animated creature");
    for (const point of points) {
      const [px, py, pz] = point.position;
      const after = await game.raycast({ start: [px, py, pz - 1], end: [px, py, pz + 1],
        collision_groups: ["hitbox"] });
      assert.ok(after.entity_id === null || !proxies.has(after.entity_id),
        "removed living limb must not absorb later shots");
    }
  });
