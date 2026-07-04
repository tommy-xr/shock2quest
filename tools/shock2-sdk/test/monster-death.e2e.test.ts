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
  "a killed monster dies and stays dead",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "medsci1.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8104),
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

    // Alert it first so the post-death alertness machinery has both paths to
    // fire (escalation while the player is visible, decay afterwards) - the
    // regression this guards against is any alertness level change replacing
    // DeadBehavior.
    // Only step a few frames past the alert: forcing alertness queues a
    // fresh chase clip, so the kill below lands near the clip's start and
    // the eager-death assertion actually discriminates (waiting for the
    // clip to complete would blow the half-second window).
    await game.input.trigger("DebugAlertAll");
    await game.step({ frames: 10 });
    let detail = await game.entities.detail(monster.id);
    assert.equal(aiProp(detail, "AIBehavior"), "Chase");

    // Lethal damage reacts eagerly: the death animation interrupts the
    // in-flight chase clip instead of waiting for it to complete, so the
    // behavior must read Dead within half a second of the killing blow.
    await game.entities.sendMessage(monster.id, {
      type: "Damage",
      amount: 1000,
    });
    await game.step({ frames: 30 });
    assert.equal(
      aiProp(await game.entities.detail(monster.id), "AIBehavior"),
      "Dead",
      "killing blow should interrupt the playing clip immediately",
    );

    // Let the crumple (and any interrupted clip still in the animation
    // queue) finish - death clips carry root motion, so the body still
    // translates for a couple of seconds after DeadBehavior is set.
    await game.step({ frames: 240 });
    assert.equal(
      aiProp(await game.entities.detail(monster.id), "AIBehavior"),
      "Dead",
    );

    // The corpse must stay dead: the alertness escalate (1.5s) and decay (3s)
    // windows both elapse several times over while the player stands in view.
    const deadPos = (await game.entities.detail(monster.id)).position;
    for (let i = 0; i < 5; i++) {
      await game.step({ frames: 120 });
      detail = await game.entities.detail(monster.id);
      assert.equal(
        aiProp(detail, "AIBehavior"),
        "Dead",
        `corpse resurrected after ${(i + 1) * 2}s`,
      );
    }

    // ... and must not wander off.
    const finalPos = detail.position;
    const drift = Math.hypot(
      finalPos[0] - deadPos[0],
      finalPos[2] - deadPos[2],
    );
    assert.ok(
      drift < 0.5,
      `corpse moved ${drift.toFixed(2)} units after death`,
    );
  },
);
