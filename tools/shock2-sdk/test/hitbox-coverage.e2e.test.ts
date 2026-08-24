import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";

const e2eEnabled = process.env.SHOCK2_E2E === "1";

/**
 * A creature's per-joint hitbox proxies are what melee, projectiles AND
 * frob/selection raycasts actually hit. When they were the raw per-joint vertex
 * AABBs they clustered around the joint origins, leaving the bone segments
 * between joints uncovered - a horizontal ray through a hybrid's thigh passed
 * clean through it, which is why corpses were so fiddly to aim at and loot.
 *
 * Sweep horizontal rays up the standing creature and require an unbroken run of
 * hits from just above the knees to over the head. With the old AABB proxies
 * this fails immediately (the lower band is empty); with the fitted
 * capsule/box shapes the whole body is covered.
 */
test(
  "creature hitbox proxies cover the whole body, with no gaps between joints",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "debug_melee",
      port: Number(process.env.SHOCK2_E2E_HITBOX_COVERAGE_PORT ?? 8393),
    });
    // Hitbox proxies are built (and their bodies registered with the query
    // pipeline) on the first updates.
    await game.step({ frames: 30 });

    const creatures = await game.entities.list({ filter: "OG-Pipe" });
    assert.ok(
      creatures.entities.length > 0,
      "debug_melee should contain OG-Pipe hybrids to swing at",
    );
    const creature = creatures.entities[0];
    const [cx, cy, cz] = creature.position;

    const detail = await game.entities.detail(creature.id);
    const proxyIds = new Set((detail.aim_points ?? []).map((p) => p.proxy_entity_id));
    assert.ok(proxyIds.size > 0, "creature should expose per-joint hitbox proxies");

    // Heights relative to the creature origin: roughly mid-thigh up to above the
    // head. The origin sits about a metre above the feet.
    const misses: string[] = [];
    const hits: number[] = [];
    for (let offset = -1.05; offset <= 1.45 + 1e-6; offset += 0.05) {
      const y = cy + offset;
      const hit = await game.raycast({
        start: [cx, y, cz - 3.0],
        end: [cx, y, cz + 3.0],
        collision_groups: ["hitbox"],
      });
      if (hit.hit && hit.entity_id !== null && proxyIds.has(hit.entity_id)) {
        hits.push(y);
      } else {
        misses.push(
          `y=${y.toFixed(2)} (${hit.hit ? `hit ${hit.entity_id}` : "no hit"})`,
        );
      }
    }

    assert.deepEqual(
      misses,
      [],
      `every height along the creature should hit one of its own hitbox proxies; uncovered: ${misses.join(", ")}`,
    );
    assert.ok(hits.length >= 50, `expected a dense sweep, got ${hits.length} samples`);
  },
);
