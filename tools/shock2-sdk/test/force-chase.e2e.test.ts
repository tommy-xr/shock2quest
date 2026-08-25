import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { EntityDetailResult } from "../src/index.js";

// End-to-end test against a real debug runtime. Requires game assets in
// Data/ and compiles the runtime on first run, so it is opt-in:
//
//   npm run test:e2e        (or SHOCK2_E2E=1 node --test dist/test/)
const e2eEnabled = process.env.SHOCK2_E2E === "1";

function aiProp(detail: EntityDetailResult, name: string): string | undefined {
  return detail.properties.find((p) => p.name === name)?.value;
}

function dist3(a: [number, number, number], b: [number, number, number]): number {
  const dx = a[0] - b[0];
  const dy = a[1] - b[1];
  const dz = a[2] - b[2];
  return Math.sqrt(dx * dx + dy * dy + dz * dz);
}

test(
  "DebugForceChase pins alertness (no decay) and converges monsters across the map",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "medsci1.mis",
      experimental: ["nav_bridges"],
    });

    await game.step({ frames: 10 });

    // The spawn point is the sealed cryo recovery room (AI-unreachable by
    // design); measure convergence toward a player standing in the open
    // deck instead.
    await game.player.teleport({ x: -14.0, y: 0.5, z: -30.0 });
    await game.step({ frames: 10 });

    const hybrids = (await game.entities.list({ filter: "OG-", limit: 20 })).entities.filter(
      (e) => e.name.startsWith("OG-"),
    );
    assert.ok(hybrids.length >= 2, `expected hybrids in medsci1, got ${hybrids.length}`);

    // Baseline distances BEFORE pinning - convergence is measured from the
    // moment the pin lands, before anyone starts moving. (The pin phase runs
    // on a fresh world: alertness churn from a prior alert/decay cycle can
    // leave AIs in a stuck animation state - a separate, pre-existing bug -
    // which would corrupt the convergence measurement.)
    const player = await game.player.position();
    const playerPos: [number, number, number] = [player.x, player.y, player.z];
    const before = new Map<number, number>();
    for (const h of hybrids) {
      const d = await game.entities.detail(h.id);
      before.set(h.id, dist3(d.position, playerPos));
    }

    // DebugForceChase: pinned - still chasing after a 10s unseen window
    // (the negative decay control runs at the end of the test).
    let detail: EntityDetailResult;
    await game.input.trigger("DebugForceChase");
    await game.step({ frames: 30 });
    // Probe whichever hybrid actually took the pin, rather than the nearest:
    // medsci1's scripted vent hybrid (mission object 613) authors an alert cap
    // of Lowest (P$AI_AlertC), so it legitimately cannot be alerted at all.
    const pinned = [];
    for (const h of hybrids) {
      const d = await game.entities.detail(h.id);
      if (["Moderate", "High"].includes(aiProp(d, "AIAlertness") ?? "")) {
        pinned.push(h);
      }
    }
    assert.ok(
      pinned.length >= 2,
      `expected at least 2 hybrids to take the pinned alertness, got ${pinned.length}`,
    );
    const probe = pinned[0];
    await game.step({ frames: 600 });
    detail = await game.entities.detail(probe.id);
    assert.ok(
      ["Moderate", "High"].includes(aiProp(detail, "AIAlertness") ?? ""),
      `pinned alertness must not decay (got ${aiProp(detail, "AIAlertness")})`,
    );

    // Convergence: over a further 20s of pinned chase, hybrids across the
    // deck close on the player (nav_bridges reconnects the mesh; some are
    // legitimately walled off - furniture-sealed rooms, locked doors - so
    // require progress from several, not all).
    await game.step({ frames: 1200 });
    let closer = 0;
    let minFinal = Infinity;
    let sumDelta = 0;
    for (const h of hybrids) {
      const d = await game.entities.detail(h.id);
      const final = dist3(d.position, playerPos);
      const delta = final - (before.get(h.id) ?? 0);
      minFinal = Math.min(minFinal, final);
      sumDelta += delta;
      if (delta < -2.0) closer += 1;
      console.log(
        `  hybrid ${h.id}: ${(before.get(h.id) ?? 0).toFixed(1)} -> ${final.toFixed(1)} (${delta >= 0 ? "+" : ""}${delta.toFixed(1)})`,
      );
    }
    console.log(`  sum distance delta: ${sumDelta.toFixed(1)} across ${hybrids.length} hybrids`);
    assert.ok(
      closer >= 2,
      `expected at least 2 of ${hybrids.length} hybrids to close on the player, got ${closer}`,
    );
    // Aggregate convergence metric: total approach across the fleet. The
    // main-branch baseline measures ~-17 over 90s with everything the old
    // graph allowed; a healthy pinned chase on the fixed graph clears -25
    // in this 30s window with room to spare. Guards regressions in the
    // pathfinding graph or steering without depending on any single hybrid.
    assert.ok(
      sumDelta <= -20.0,
      `expected the fleet to approach by 20+ units total, got ${sumDelta.toFixed(1)}`,
    );
    // ...and at least one actually ARRIVES (engagement range), not just
    // drifts closer - the arrival is the point of the pin
    assert.ok(
      minFinal < 15.0,
      `expected a hybrid to reach engagement range, closest ended at ${minFinal.toFixed(1)}`,
    );

    // DebugCalmAll clears the pin: alertness decays freely again.
    await game.input.trigger("DebugCalmAll");
    await game.step({ frames: 30 });
    detail = await game.entities.detail(probe.id);
    assert.equal(aiProp(detail, "AIAlertness"), "Lowest");
    await game.step({ frames: 120 });
    detail = await game.entities.detail(probe.id);
    assert.equal(
      aiProp(detail, "AIAlertness"),
      "Lowest",
      "after DebugCalmAll the pin must be gone (no snap back to chase)",
    );

    // Negative control last: an unpinned DebugAlertAll decays back toward
    // idle within a few seconds when the AI can't see the player - the
    // behavior the pin exists to override.
    await game.input.trigger("DebugAlertAll");
    await game.step({ frames: 30 });
    detail = await game.entities.detail(probe.id);
    assert.equal(aiProp(detail, "AIAlertness"), "Moderate");
    await game.step({ frames: 600 }); // 10s unseen
    detail = await game.entities.detail(probe.id);
    assert.notEqual(
      aiProp(detail, "AIAlertness"),
      "Moderate",
      "unpinned alertness should have decayed after 10s without sight",
    );
  },
);
