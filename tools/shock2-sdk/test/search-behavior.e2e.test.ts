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

function distanceXZ(
  a: [number, number, number],
  b: { x: number; z: number },
): number {
  const dx = a[0] - b.x;
  const dz = a[2] - b.z;
  return Math.sqrt(dx * dx + dz * dz);
}

async function teleportVerified(
  game: Awaited<ReturnType<typeof GameServer.launch>>,
  target: { x: number; y: number; z: number },
): Promise<void> {
  // Teleports have been observed to intermittently no-op (reported success,
  // position unchanged) - verify and retry so a stranded player doesn't
  // invalidate the scenario.
  for (let attempt = 0; attempt < 3; attempt++) {
    await game.player.teleport(target);
    await game.step({ frames: 5 });
    const pos = await game.player.position();
    const dx = pos.x - target.x;
    const dz = pos.z - target.z;
    if (Math.sqrt(dx * dx + dz * dz) < 3) {
      return;
    }
  }
  throw new Error("teleport did not take effect after 3 attempts");
}

test(
  "a High-origin search survives further alertness decay",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "medsci1.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8108),
    });

    await game.step({ frames: 10 });

    const preSpawn = await game.entities.list({ filter: "OG-Pipe", limit: 50 });
    const known = new Set(preSpawn.entities.map((e) => e.id));
    await game.input.trigger("SpawnDebugMonster");
    // Let it see the player while idle so a last-known position is recorded.
    await game.step({ frames: 60 });
    const postSpawn = await game.entities.list({ filter: "OG-Pipe", limit: 50 });
    const monster = postSpawn.entities.find(
      (e) => e.name === "OG-Pipe" && !known.has(e.id),
    );
    assert.ok(monster, "expected a newly spawned OG-Pipe");

    // Force full combat alert, then vanish: decay runs High -> Moderate
    // (search starts) -> Low (the search must keep running - it hands off
    // on its own schedule, not on the next decay tick).
    await game.entities.sendMessage(monster.id, {
      type: "SetAlertness",
      level: "High",
    });
    // A verified no-line-of-sight pocket around corners from the start area
    // (NOT the farthest spawn: routes toward it can reacquire sight).
    await teleportVerified(game, { x: -13.61, y: -5.8, z: 30.75 });

    await game.waitFor(
      async () => {
        await game.step({ frames: 30 });
        const d = await game.entities.detail(monster.id);
        return aiProp(d, "AIBehavior") === "Search" ? d : undefined;
      },
      {
        timeoutMs: 120_000,
        description: "High-origin decay to enter Search",
      },
    );

    // 3.2 sim-seconds later the Moderate->Low decay (exactly 3.0s after
    // Search entry) has fired, while the search's own give-up (6s of
    // scanning) is still well out - even when the polling loop detected
    // the entry a step late. With chase-to-last-seen the monster is
    // already standing on the last-known spot when Search begins, so the
    // scan clock starts immediately.
    await game.step({ frames: 192 });
    const detail = await game.entities.detail(monster.id);
    assert.equal(
      aiProp(detail, "AIBehavior"),
      "Search",
      `the search must survive the next decay step (alertness=${aiProp(detail, "AIAlertness")}, visible=${aiProp(detail, "AITargetVisible")})`,
    );
  },
);

test(
  "losing sight of the player triggers a search of the last-known position",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "medsci1.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8106),
    });

    await game.step({ frames: 10 });

    // Spawn a monster in front of the player, identified by diffing the
    // entity list across the spawn (medsci1 has native OG-Pipes too).
    const preSpawn = await game.entities.list({ filter: "OG-Pipe", limit: 50 });
    const known = new Set(preSpawn.entities.map((e) => e.id));
    await game.input.trigger("SpawnDebugMonster");
    await game.step({ frames: 30 });
    const postSpawn = await game.entities.list({ filter: "OG-Pipe", limit: 50 });
    const monster = postSpawn.entities.find(
      (e) => e.name === "OG-Pipe" && !known.has(e.id),
    );
    assert.ok(monster, "expected a newly spawned OG-Pipe");

    // Alert it and let it see + chase the player for a moment, so a
    // last-known position is recorded.
    const lastKnown = await game.player.position();
    await game.entities.sendMessage(monster.id, {
      type: "SetAlertness",
      level: "Moderate",
    });
    await game.step({ frames: 60 });
    let detail = await game.entities.detail(monster.id);
    assert.equal(aiProp(detail, "AIAlertness"), "Moderate");

    // Teleport the player to the farthest native OG-Pipe's spawn position:
    // guaranteed floor-valid, and far enough through multiple rooms that the
    // monster has no line of sight and cannot reach it before alertness
    // decays. Sight breaks, and instead of dropping straight to wander the
    // AI must investigate where the player WAS.
    const farthest = preSpawn.entities.reduce((a, b) =>
      b.distance > a.distance ? b : a,
    );
    assert.ok(
      farthest.distance > 30,
      `need a distant teleport target, farthest native is ${farthest.distance.toFixed(1)}`,
    );
    await teleportVerified(game, {
      x: farthest.position[0],
      y: farthest.position[1] + 0.5,
      z: farthest.position[2],
    });

    const sawSearch = await game.waitFor(
      async () => {
        await game.step({ frames: 30 });
        const d = await game.entities.detail(monster.id);
        return aiProp(d, "AIBehavior") === "Search" ? d : undefined;
      },
      {
        timeoutMs: 120_000,
        description: "AI to enter Search after losing sight of the player",
      },
    );

    // The searcher heads for the last-known position, not the player's new
    // location: over the next seconds its distance to the OLD spot shrinks
    // (it may already be close from the chase - accept either progress or
    // arrival inside the search radius).
    const before = distanceXZ(sawSearch.position, lastKnown);
    await game.step({ frames: 120 });
    detail = await game.entities.detail(monster.id);
    const after = distanceXZ(detail.position, lastKnown);
    assert.ok(
      after < Math.max(before, 2.5),
      `searcher should close on the last-known position (before=${before.toFixed(2)}, after=${after.toFixed(2)})`,
    );

    // After scanning the spot without reacquiring the player, the search
    // hands off to Wander (or decays to Idle) - it must not chase the
    // player's NEW position, which it never saw.
    const settled = await game.waitFor(
      async () => {
        await game.step({ frames: 60 });
        const d = await game.entities.detail(monster.id);
        const behavior = aiProp(d, "AIBehavior");
        return behavior === "Wander" || behavior === "Idle" ? d : undefined;
      },
      {
        timeoutMs: 120_000,
        description: "search to give up and hand off to Wander/Idle",
      },
    );
    const newPlayer = await game.player.position();
    assert.ok(
      distanceXZ(settled.position, newPlayer) > 5,
      "the AI must not have magically tracked the teleported player",
    );
  },
);
