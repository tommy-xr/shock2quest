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

function distanceXZ(a: [number, number, number], b: { x: number; z: number }): number {
  const dx = a[0] - b.x;
  const dz = a[2] - b.z;
  return Math.sqrt(dx * dx + dz * dz);
}

test(
  "forced alertness deterministically drives AI behavior and chase steering",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "medsci1.mis",
    });

    await game.step({ frames: 10 });

    // Spawn a monster in front of the player, identifying it by diffing the
    // entity list across the spawn (medsci1 has native OG-Pipes too).
    const preSpawn = await game.entities.list({ filter: "OG-Pipe", limit: 50 });
    const known = new Set(preSpawn.entities.map((e) => e.id));
    await game.input.trigger("SpawnDebugMonster");
    await game.step({ frames: 30 });
    const postSpawn = await game.entities.list({ filter: "OG-Pipe", limit: 50 });
    const monster = postSpawn.entities.find(
      (e) => e.name === "OG-Pipe" && !known.has(e.id),
    );
    assert.ok(
      monster,
      `expected a newly spawned OG-Pipe, got ${JSON.stringify(postSpawn.entities.map((e) => e.id))}`,
    );

    // Freshly spawned: unaware and idle.
    let detail = await game.entities.detail(monster.id);
    assert.equal(aiProp(detail, "AIAlertness"), "Lowest");
    assert.equal(aiProp(detail, "AIBehavior"), "Idle");

    // Baseline distance while the monster is still idle (stationary).
    const player = await game.player.position();
    const before = distanceXZ(detail.position, player);

    // DebugAlertAll forces Moderate alertness -> Chase behavior, with no
    // dependence on FOV or visibility timing.
    await game.input.trigger("DebugAlertAll");
    await game.step({ frames: 30 });
    detail = await game.entities.detail(monster.id);
    assert.equal(aiProp(detail, "AIAlertness"), "Moderate");
    assert.equal(aiProp(detail, "AIBehavior"), "Chase");

    // The chasing monster closes on the player.
    await game.step({ frames: 120 });
    detail = await game.entities.detail(monster.id);
    const after = distanceXZ(detail.position, player);
    assert.ok(
      after < before - 1.0,
      `expected the monster to close on the player (before=${before.toFixed(2)}, after=${after.toFixed(2)})`,
    );

    // DebugCalmAll returns it to idle.
    await game.input.trigger("DebugCalmAll");
    await game.step({ frames: 30 });
    detail = await game.entities.detail(monster.id);
    assert.equal(aiProp(detail, "AIAlertness"), "Lowest");
    assert.equal(aiProp(detail, "AIBehavior"), "Idle");

    // Per-entity forcing via the message endpoint works too. The monster is
    // already at melee range from the chase, so Chase may legitimately
    // escalate to MeleeAttack via next_behavior - the alertness level is the
    // deterministic assertion here.
    const accepted = await game.entities.sendMessage(monster.id, {
      type: "SetAlertness",
      level: "Moderate",
    });
    assert.ok(accepted);
    await game.step({ frames: 30 });
    detail = await game.entities.detail(monster.id);
    assert.equal(aiProp(detail, "AIAlertness"), "Moderate");
    const behavior = aiProp(detail, "AIBehavior");
    assert.ok(
      behavior === "Chase" || behavior === "MeleeAttack",
      `expected Chase or MeleeAttack, got ${behavior}`,
    );
  },
);
