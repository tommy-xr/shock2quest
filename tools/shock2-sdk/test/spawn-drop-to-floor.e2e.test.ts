import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";

// End-to-end test against a real debug runtime. Requires game assets in
// Data/ and compiles the runtime on first run, so it is opt-in:
//
//   npm run test:e2e        (or SHOCK2_E2E=1 node --test dist/test/)
const e2eEnabled = process.env.SHOCK2_E2E === "1";

// Regression for #401: SpawnDebugMonster used to place the spawn at a naive
// forward offset with NO drop-to-floor raycast. On pitched-down aim the offset
// lands inside/below the level geometry, so the monster is created below the
// floor and falls out of the world (body y plummeting from ~floor level to
// tens-negative). The fix casts straight down onto the floor at the target's
// XZ and rests the monster there.
test(
  "SpawnDebugMonster drops the spawn to the floor even on pitched-down aim",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "medsci1.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8131),
      echoLogs: process.env.SHOCK2_ECHO_LOGS === "1",
    });

    await game.step({ frames: 10 });

    // Floor reference: the player stands on it, so its y is ~the floor height
    // where the monster should come to rest.
    const floorY = (await game.player.position()).y;

    // Aim steeply DOWN into the floor. At this pitch the naive forward offset
    // (up + forward, then rotated by the head) tips several units below the
    // floor - the exact case that used to spawn the monster below the world.
    // (Empirically on medsci1: floor ~ -5, naive spawn ~ -8.3, then plummeting
    // to ~ -200 within a few seconds. The fixed path rests it at ~ -4.)
    await game.input.set("head.look", [0, 80]);
    await game.step({ frames: 2 });

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

    // Read the spawned body's y right after spawn...
    async function bodyY(): Promise<number> {
      const { bodies } = await game.physics.bodies({ entityId: monster!.id });
      assert.ok(bodies.length > 0, "spawned monster should own a physics body");
      // The creature capsule is the lowest body; take the min y across bodies.
      return Math.min(...bodies.map((b) => b.position[1]));
    }
    const initialY = await bodyY();

    // ...and after letting physics run for ~2s. If the monster spawned below
    // the floor it keeps falling into the void; if it rests on the floor it
    // stays put.
    await game.step({ frames: 120 });
    const finalY = await bodyY();

    // It must not have plummeted out of the world. Pre-fix, finalY reaches
    // tens-negative (the issue records y ~ -5 -> -135, observed ~ -200 here);
    // the drop-to-floor spawn keeps it within a couple of units of the floor it
    // was placed on.
    assert.ok(
      finalY > floorY - 2.0,
      `monster fell through the floor: floorY=${floorY.toFixed(2)}, ` +
        `initialY=${initialY.toFixed(2)}, finalY=${finalY.toFixed(2)}`,
    );
    // And it did not spawn below the floor to begin with (pre-fix ~ -8.3).
    assert.ok(
      initialY > floorY - 2.0,
      `monster spawned below the floor: floorY=${floorY.toFixed(2)}, ` +
        `initialY=${initialY.toFixed(2)}`,
    );
  },
);
