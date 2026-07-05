import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { EntityDetailResult } from "../src/index.js";

// End-to-end: a calm AI flagged to patrol (P$AI_Patrol) walks its authored
// route of AIPatrol-linked points, instead of standing idle. eng1 ships a
// substantial patrol network; several of its native creatures are patrollers.
//
// Opt-in (needs Data/ assets + compiles the runtime): npm run test:e2e
const e2eEnabled = process.env.SHOCK2_E2E === "1";

function aiProp(detail: EntityDetailResult, name: string): string | undefined {
  return detail.properties.find((p) => p.name === name)?.value;
}

function distXZ(
  a: [number, number, number],
  b: [number, number, number],
): number {
  return Math.hypot(a[0] - b[0], a[2] - b[2]);
}

// eng1's native AI archetypes that carry patrol flags
const CREATURE_NAMES = ["OG-Pipe", "OG-Shotgun", "Blue Monkey"];

test(
  "a calm patroller walks its authored route",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "eng1.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8161),
    });

    // Let the level's AIs instantiate.
    await game.step({ frames: 60 });

    // Gather eng1's native creatures.
    const creatures: number[] = [];
    for (const name of CREATURE_NAMES) {
      const list = await game.entities.list({ filter: name, limit: 100 });
      for (const e of list.entities) {
        if (e.name === name) creatures.push(e.id);
      }
    }
    assert.ok(creatures.length > 0, "eng1 should have native creatures");

    // Force every creature calm, then find one that enters Patrol - only a
    // creature with P$AI_Patrol and a reachable route does (the rest go Idle),
    // so this both discovers a patroller and proves the behavior is selected.
    for (const id of creatures) {
      await game.entities.sendMessage(id, {
        type: "SetAlertness",
        level: "Lowest",
      });
    }
    await game.step({ frames: 20 });

    let patroller: number | undefined;
    for (const id of creatures) {
      const detail = await game.entities.detail(id);
      if (aiProp(detail, "AIBehavior") === "Patrol") {
        patroller = id;
        break;
      }
    }
    assert.ok(
      patroller !== undefined,
      "expected at least one eng1 creature to enter Patrol when calm",
    );

    // Get the patroller out of the player's sight so it stays calm on its own -
    // teleport the player far off, then calm the patroller once. This lets a
    // SINGLE PatrolBehavior instance run uninterrupted, so the observed motion
    // really comes from arriving at a point and advancing to the next (not from
    // the behavior being rebuilt each tick).
    await game.player.teleport({ x: 300, y: 0, z: 300 });
    await game.step({ frames: 5 });
    await game.entities.sendMessage(patroller, {
      type: "SetAlertness",
      level: "Lowest",
    });
    await game.step({ frames: 10 });

    // Walk the route. Sum per-tick displacement (cumulative path length) so the
    // check holds even when a loop brings the AI back near where it started.
    let prev = (await game.entities.detail(patroller)).position;
    let traveled = 0;
    let stayedPatrolling = true;
    for (let tick = 0; tick < 10; tick++) {
      await game.step({ frames: 120 });
      const detail = await game.entities.detail(patroller);
      traveled += distXZ(detail.position, prev);
      prev = detail.position;
      if (aiProp(detail, "AIBehavior") !== "Patrol") stayedPatrolling = false;
    }

    assert.ok(
      stayedPatrolling,
      "an out-of-sight patroller should stay in Patrol the whole time",
    );
    // A standing Idle AI accumulates ~0; a patroller walks point to point
    // (route legs are tens of units apart), so it covers real ground.
    assert.ok(
      traveled > 8,
      `patroller should walk a meaningful distance along its route, traveled ${traveled.toFixed(1)}`,
    );
  },
);
