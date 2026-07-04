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

test(
  "a chasing AI pursues the last-seen position, not the vanished player",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "medsci1.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8110),
    });

    await game.step({ frames: 10 });

    const preSpawn = await game.entities.list({ filter: "OG-Pipe", limit: 50 });
    const known = new Set(preSpawn.entities.map((e) => e.id));
    await game.input.trigger("SpawnDebugMonster");
    await game.step({ frames: 30 });
    const postSpawn = await game.entities.list({ filter: "OG-Pipe", limit: 50 });
    const monster = postSpawn.entities.find(
      (e) => e.name === "OG-Pipe" && !known.has(e.id),
    );
    assert.ok(monster, "expected a newly spawned OG-Pipe");

    // Chase with the player in sight: awareness tracks the live position.
    await game.entities.sendMessage(monster.id, {
      type: "SetAlertness",
      level: "Moderate",
    });
    await game.step({ frames: 30 });
    let detail = await game.entities.detail(monster.id);
    assert.equal(aiProp(detail, "AITargetVisible"), "true");

    // Vanish to a no-line-of-sight pocket. The chase must keep working the
    // OLD position: awareness freezes, and during the pre-decay window the
    // monster gains no ground on the player's NEW location.
    const oldPos = await game.player.position();
    for (let attempt = 0; attempt < 3; attempt++) {
      await game.player.teleport({ x: -13.61, y: -5.8, z: 30.75 });
      await game.step({ frames: 5 });
      const pos = await game.player.position();
      if (Math.hypot(pos.x - -13.61, pos.z - 30.75) < 3) break;
    }
    await game.step({ frames: 10 });
    detail = await game.entities.detail(monster.id);
    assert.equal(
      aiProp(detail, "AITargetVisible"),
      "false",
      "line of sight must break after the teleport",
    );
    const newPos = await game.player.position();
    const distNewBefore = distanceXZ(detail.position, newPos);
    const distOldBefore = distanceXZ(detail.position, oldPos);

    // Two seconds of chasing (inside the ~3s decay window).
    await game.step({ frames: 120 });
    detail = await game.entities.detail(monster.id);
    const distNewAfter = distanceXZ(detail.position, newPos);
    const distOldAfter = distanceXZ(detail.position, oldPos);

    assert.ok(
      distNewAfter > distNewBefore - 1.0,
      `the chase must not gain ground on the vanished player (before=${distNewBefore.toFixed(2)}, after=${distNewAfter.toFixed(2)})`,
    );
    assert.ok(
      distOldAfter <= Math.max(distOldBefore, 2.5),
      `the chase should stay on the last-seen position (before=${distOldBefore.toFixed(2)}, after=${distOldAfter.toFixed(2)})`,
    );
  },
);
