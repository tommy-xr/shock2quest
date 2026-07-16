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
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8103),
      experimental: ["nav_bridges"],
    });

    await game.step({ frames: 10 });

    // The spawn point is the sealed cryo recovery room (AI-unreachable by
    // design); measure convergence toward a player standing in the open
    // deck instead.
    await game.player.teleport({ x: -14.0, y: 0.5, z: -30.0 });
    await game.step({ frames: 10 });

    // Negative control first: an unpinned DebugAlertAll decays back toward
    // idle within a few seconds when the AI can't see the player.
    const hybrids = (await game.entities.list({ filter: "OG-", limit: 20 })).entities.filter(
      (e) => e.name.startsWith("OG-"),
    );
    assert.ok(hybrids.length >= 2, `expected hybrids in medsci1, got ${hybrids.length}`);
    const probe = hybrids[0];

    await game.input.trigger("DebugAlertAll");
    await game.step({ frames: 30 });
    let detail = await game.entities.detail(probe.id);
    assert.equal(aiProp(detail, "AIAlertness"), "Moderate");
    await game.step({ frames: 600 }); // 10s unseen
    detail = await game.entities.detail(probe.id);
    assert.notEqual(
      aiProp(detail, "AIAlertness"),
      "Moderate",
      "unpinned alertness should have decayed after 10s without sight",
    );

    // Baseline distances BEFORE pinning - convergence is measured from the
    // moment the pin lands, before anyone starts moving.
    const player = await game.player.position();
    const playerPos: [number, number, number] = [player.x, player.y, player.z];
    const before = new Map<number, number>();
    for (const h of hybrids) {
      const d = await game.entities.detail(h.id);
      before.set(h.id, dist3(d.position, playerPos));
    }

    // DebugForceChase: pinned - still chasing after the same 10s window.
    await game.input.trigger("DebugForceChase");
    await game.step({ frames: 30 });
    detail = await game.entities.detail(probe.id);
    assert.ok(
      ["Moderate", "High"].includes(aiProp(detail, "AIAlertness") ?? ""),
      `expected pinned alertness, got ${aiProp(detail, "AIAlertness")}`,
    );
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
    for (const h of hybrids) {
      const d = await game.entities.detail(h.id);
      const delta = dist3(d.position, playerPos) - (before.get(h.id) ?? 0);
      if (delta < -2.0) closer += 1;
    }
    assert.ok(
      closer >= 2,
      `expected at least 2 of ${hybrids.length} hybrids to close on the player, got ${closer}`,
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
  },
);
