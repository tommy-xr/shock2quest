import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { EntityDetailResult } from "../src/index.js";

// End-to-end test against a real debug runtime. Requires game assets in
// Data/ and compiles the runtime on first run, so it is opt-in:
//
//   npm run test:e2e        (or SHOCK2_E2E=1 node --test dist/test/)
const e2eEnabled = process.env.SHOCK2_E2E === "1";

// earth.mis training protocol droids (mission objects 547/593, template
// -4015 "Training Droid"), which inherit the `Docile` metaproperty (-1073)
// authoring P$AI_AlertC = Lowest/Lowest/Lowest. Runtime entity ids are not
// stable across launches, so they are discovered by name.
const TRAINING_DROID_NAME = "Training Droid";

function aiProp(detail: EntityDetailResult, name: string): string | undefined {
  return detail.properties.find((p) => p.name === name)?.value;
}

function distanceXZ(a: [number, number, number], b: [number, number, number]): number {
  const dx = a[0] - b[0];
  const dz = a[2] - b[2];
  return Math.sqrt(dx * dx + dz * dz);
}

test(
  "docile training droids stay pinned at Lowest alertness and hold their stand",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "earth.mis",
    });

    await game.step({ frames: 10 });

    const { entities } = await game.entities.list({ filter: TRAINING_DROID_NAME });
    const droids = entities.filter((e) => e.name === TRAINING_DROID_NAME);
    assert.ok(
      droids.length > 0,
      `expected at least one "${TRAINING_DROID_NAME}" in earth.mis`,
    );

    const start = new Map<number, [number, number, number]>();
    for (const droid of droids) {
      const detail = await game.entities.detail(droid.id);
      assert.equal(
        aiProp(detail, "AIAlertness"),
        "Lowest",
        `droid ${droid.id} should spawn calm`,
      );
      start.set(droid.id, detail.position);
    }

    // Force every AI to Moderate. The Docile alert cap (max_level = Lowest)
    // must clamp that away for the training droids - before P$AI_AlertC was
    // registered under its truncated chunk name the property never parsed,
    // so they escalated to Chase and wandered off their stands.
    await game.input.trigger("DebugAlertAll");
    await game.step({ frames: 300 });

    for (const droid of droids) {
      const detail = await game.entities.detail(droid.id);
      assert.equal(
        aiProp(detail, "AIAlertness"),
        "Lowest",
        `droid ${droid.id} should be capped at Lowest by the Docile alert cap`,
      );
      const behavior = aiProp(detail, "AIBehavior");
      assert.notEqual(behavior, "Chase", `droid ${droid.id} should not chase`);

      const moved = distanceXZ(detail.position, start.get(droid.id)!);
      assert.ok(
        moved < 1.0,
        `droid ${droid.id} should hold its stand (moved ${moved.toFixed(2)} units)`,
      );
    }
  },
);
