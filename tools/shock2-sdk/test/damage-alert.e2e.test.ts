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

test(
  "taking damage alerts an unaware AI (it aggros and chases)",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "medsci1.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8103),
    });

    await game.step({ frames: 10 });

    // Spawn a monster, identified by diffing the entity list across the
    // spawn (medsci1 has native OG-Pipes too).
    const preSpawn = await game.entities.list({ filter: "OG-Pipe", limit: 50 });
    const known = new Set(preSpawn.entities.map((e) => e.id));
    await game.input.trigger("SpawnDebugMonster");
    await game.step({ frames: 30 });
    const postSpawn = await game.entities.list({ filter: "OG-Pipe", limit: 50 });
    const monster = postSpawn.entities.find(
      (e) => e.name === "OG-Pipe" && !known.has(e.id),
    );
    assert.ok(monster, "expected a newly spawned OG-Pipe");

    // Unaware before the shot.
    let detail = await game.entities.detail(monster.id);
    assert.equal(aiProp(detail, "AIAlertness"), "Lowest");
    assert.equal(aiProp(detail, "AIBehavior"), "Idle");

    // A non-lethal hit aggros it: escalates straight to Moderate (chase),
    // no line of sight required.
    await game.entities.sendMessage(monster.id, { type: "Damage", amount: 1.0 });
    await game.step({ frames: 30 });
    detail = await game.entities.detail(monster.id);
    assert.equal(aiProp(detail, "AIAlertness"), "Moderate");
    const behavior = aiProp(detail, "AIBehavior");
    assert.ok(
      behavior === "Chase" || behavior === "MeleeAttack",
      `expected Chase or MeleeAttack after taking damage, got ${behavior}`,
    );
  },
);
