import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { EntityDetailResult } from "../src/index.js";

const e2eEnabled = process.env.SHOCK2_E2E === "1";

function numericProp(detail: EntityDetailResult, name: string): number {
  const value = detail.properties.find((property) => property.name === name)?.value;
  assert.ok(value !== undefined, `${detail.name} ${detail.entity_id} has no ${name}`);
  return Number(value);
}

function distance(
  position: [number, number, number],
  target: { x: number; y: number; z: number },
): number {
  return Math.hypot(position[0] - target.x, position[1] - target.y, position[2] - target.z);
}

test(
  "ranged AIs do not hurt allies during a pinned corridor convergence",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "medsci1.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8146),
      experimental: ["nav_bridges"],
    });

    await game.step({ frames: 10 });
    const player = { x: -14.0, y: 0.5, z: -30.0 };
    await game.player.teleport(player);
    await game.step({ frames: 10 });

    const hybrids = (await game.entities.list({ filter: "OG-", limit: 20 })).entities.filter(
      (entity) => entity.name.startsWith("OG-"),
    );
    assert.ok(hybrids.length >= 2, `expected medsci1 hybrids, got ${hybrids.length}`);

    const initialHealth = new Map<number, number>();
    for (const hybrid of hybrids) {
      const detail = await game.entities.detail(hybrid.id);
      initialHealth.set(hybrid.id, numericProp(detail, "HitPoints"));
    }

    // The issue's authored reproduction funnels melee and ranged hybrids onto
    // the same player down a narrow corridor. The player never attacks, so any
    // health loss during this sequence is friendly projectile damage.
    await game.input.trigger("DebugAlertAll");
    await game.step({ frames: 630 });
    await game.input.trigger("DebugForceChase");
    await game.step({ frames: 1800 });

    let closestShotgun = Infinity;
    const damaged: string[] = [];
    for (const hybrid of hybrids) {
      const detail = await game.entities.detail(hybrid.id);
      if (hybrid.name === "OG-Shotgun") {
        closestShotgun = Math.min(closestShotgun, distance(detail.position, player));
      }
      const before = initialHealth.get(hybrid.id)!;
      const after = numericProp(detail, "HitPoints");
      if (after < before) {
        damaged.push(`${hybrid.name} ${hybrid.id}: ${before} -> ${after}`);
      }
    }

    assert.ok(
      closestShotgun < 6.0,
      `the fixture must bring a ranged AI into engagement range (closest ${closestShotgun.toFixed(2)})`,
    );
    assert.deepEqual(
      damaged,
      [],
      `hybrids took damage while only their allies were firing:\n${damaged.join("\n")}`,
    );
  },
);
