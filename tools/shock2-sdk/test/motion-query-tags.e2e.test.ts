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

// A motion query that matches nothing logs a WARN naming the query; the
// animation silently never plays. These assertions pin the wound and death
// queries to resolving clips for the creatures medsci1 actually contains.
function failedQueries(logs: string[], tag: string): string[] {
  return logs.filter(
    (line) => line.includes("Unable to find animation") && line.includes(tag),
  );
}

test(
  "wound and death motion queries resolve (hybrid wound, human death)",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "medsci1.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8106),
    });

    await game.step({ frames: 10 });

    // Spawn a pipe hybrid, identifying it by diffing the entity list.
    const preSpawn = await game.entities.list({ filter: "OG-Pipe", limit: 50 });
    const known = new Set(preSpawn.entities.map((e) => e.id));
    await game.input.trigger("SpawnDebugMonster");
    await game.step({ frames: 30 });
    const postSpawn = await game.entities.list({ filter: "OG-Pipe", limit: 50 });
    const hybrid = postSpawn.entities.find(
      (e) => e.name === "OG-Pipe" && !known.has(e.id),
    );
    assert.ok(hybrid, "expected a newly spawned OG-Pipe");

    // Non-lethal damage: the wound reaction queries `receivewound` at the
    // next clip completion. Hybrid wound clips sit under a combat-context
    // key in the motion db, so without the context tag the query fails.
    await game.entities.sendMessage(hybrid.id, { type: "Damage", amount: 1 });
    await game.step({ frames: 180 });
    assert.deepEqual(
      failedQueries(game.logs(), "receivewound"),
      [],
      "hybrid wound query should resolve a clip",
    );

    // Human death: FemaleMedsci's death clips are keyed one level deeper
    // (under die); without that tag she dies frozen with no animation.
    const humans = await game.entities.list({
      filter: "FemaleMedsci",
      limit: 5,
    });
    const human = humans.entities[0];
    assert.ok(human, "expected FemaleMedsci in medsci1");
    await game.entities.sendMessage(human.id, {
      type: "Damage",
      amount: 1000,
    });
    await game.step({ frames: 180 });
    assert.deepEqual(
      failedQueries(game.logs(), "crumple"),
      [],
      "human death query should resolve a clip",
    );
    assert.equal(
      aiProp(await game.entities.detail(human.id), "AIBehavior"),
      "Dead",
    );
  },
);
