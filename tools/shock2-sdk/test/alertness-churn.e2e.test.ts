import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";

// End-to-end test against a real debug runtime. Requires game assets in
// Data/ and compiles the runtime on first run, so it is opt-in:
//
//   npm run test:e2e        (or SHOCK2_E2E=1 node --test dist/test/)
const e2eEnabled = process.env.SHOCK2_E2E === "1";

function dist3(a: [number, number, number], b: [number, number, number]): number {
  const dx = a[0] - b[0];
  const dy = a[1] - b[1];
  const dz = a[2] - b[2];
  return Math.sqrt(dx * dx + dy * dy + dz * dz);
}

function distXZ(a: [number, number, number], b: [number, number, number]): number {
  const dx = a[0] - b[0];
  const dz = a[2] - b[2];
  return Math.sqrt(dx * dx + dz * dz);
}

test(
  "no AI freezes mid-route after alertness churn (regression: #481)",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "medsci1.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8105),
      experimental: ["nav_bridges"],
    });

    await game.step({ frames: 10 });
    await game.player.teleport({ x: -14.0, y: 0.5, z: -30.0 });
    await game.step({ frames: 10 });

    const player = await game.player.position();
    const playerPos: [number, number, number] = [player.x, player.y, player.z];
    const hybrids = (await game.entities.list({ filter: "OG-", limit: 20 })).entities.filter(
      (e) => e.name.startsWith("OG-"),
    );
    assert.ok(hybrids.length >= 2, `expected hybrids in medsci1, got ${hybrids.length}`);

    // The #481 repro: alert everyone, let alertness decay (behavior churn:
    // chase -> search/wander), then pin a chase and run long enough that
    // routes cross stairs and doorway seams.
    await game.input.trigger("DebugAlertAll");
    await game.step({ frames: 630 });
    await game.input.trigger("DebugForceChase");
    await game.step({ frames: 1800 });

    // Freeze signature: an AI that is still far (XZ) from the END of its own
    // current route yet doesn't move, ALONE. Excluded (not this bug):
    // - parked AT its route end (legitimately waiting at the closest
    //   reachable point) or with no route at all (sealed rooms - Failed);
    // - within engagement range of the player (arrived; jostling against
    //   the player's capsule is combat, not navigation);
    // - another creature within 2.5 units: either a mutual crowd jam or the
    //   crowd-separation equilibrium (separation holds neighbors apart at
    //   roughly its 2.4-unit radius); such pairs are crowd dynamics, not a
    //   navigation freeze (see #487 / the furniture-route issue).
    const before = new Map<number, [number, number, number]>();
    for (const h of hybrids) {
      const d = await game.entities.detail(h.id);
      before.set(h.id, d.position);
    }
    await game.step({ frames: 300 }); // 5s observation window

    const routes = new Map(
      (await game.pathfinding.aiPaths()).map((e) => [e.entity_id, e] as const),
    );
    const positions = new Map<number, [number, number, number]>();
    for (const h of hybrids) {
      positions.set(h.id, (await game.entities.detail(h.id)).position);
    }
    const frozen: string[] = [];
    for (const h of hybrids) {
      const p = positions.get(h.id)!;
      const moved = dist3(p, before.get(h.id)!);
      const route = routes.get(h.id);
      if (!route || route.outcome === "Failed" || route.waypoints.length === 0) continue;
      if (dist3(p, playerPos) < 6.0) continue; // arrived / engaging
      const jammed = hybrids.some(
        (o) => o.id !== h.id && dist3(positions.get(o.id)!, p) < 2.5,
      );
      if (jammed) continue; // mutual crowd jam - tracked separately
      const routeEnd = route.waypoints[route.waypoints.length - 1];
      const remaining = distXZ(p, routeEnd);
      if (remaining > 3.0 && moved < 0.3) {
        frozen.push(
          `${h.id}: ${remaining.toFixed(1)} XZ from its route end, moved ${moved.toFixed(2)} in 5s (${route.outcome} route, ${route.waypoints.length} wps)`,
        );
      }
    }
    assert.deepEqual(
      frozen,
      [],
      `AIs frozen mid-route after alertness churn:\n  ${frozen.join("\n  ")}`,
    );
  },
);
