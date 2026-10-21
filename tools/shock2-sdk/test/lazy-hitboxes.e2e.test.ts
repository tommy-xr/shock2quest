import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import { e2ePort } from "./helpers/e2e-port.js";

// Explicitly requires a runtime built with shock2vr/lazy-hitboxes. The ordinary
// ranged/melee suites run unchanged against either backend.
const enabled = process.env.SHOCK2_E2E === "1" &&
  process.env.SHOCK2_EXPECT_LAZY_HITBOXES === "1";

test("query-only limbs expose aim points without bodies and remain hittable after death",
  { skip: !enabled, timeout: 600_000 }, async () => {
    await using game = await GameServer.launch({
      mission: "debug_melee",
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
    const corpse = await game.entities.detail(creature.id);
    assert.ok((corpse.aim_points?.length ?? 0) > 1,
      "animated corpses retain their query-only limbs");
    for (const point of corpse.aim_points ?? []) {
      assert.equal(point.body_id, null);
      assert.ok(proxies.has(point.proxy_entity_id), "the corpse retains its limb proxies");
    }
    const [cx, cy, cz] = corpse.aim_points![0].position;
    const after = await game.raycast({ start: [cx, cy, cz - 1], end: [cx, cy, cz + 1],
      collision_groups: ["hitbox"] });
    assert.ok(after.hit && after.entity_id !== null && proxies.has(after.entity_id),
      "the animated corpse remains hittable");
  });
