import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";

// End-to-end test against a real debug runtime. Requires game assets in
// Data/ and compiles the runtime on first run, so it is opt-in:
//
//   npm run test:e2e        (or SHOCK2_E2E=1 node --test dist/test/)
const e2eEnabled = process.env.SHOCK2_E2E === "1";

// With `--experimental ragdoll`, a monster's death crumple hands the corpse
// over to physics: once the death animation completes, the animated corpse
// entity is replaced by a multibody ragdoll. (The no-flag path - animated
// corpse persists, no ragdoll - is covered by monster-death.e2e.test.ts,
// which would fail if the corpse entity ever vanished without the flag.)
test(
  "death crumple hands off to a ragdoll under --experimental ragdoll",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "medsci1.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8106),
      experimental: ["ragdoll"],
      echoLogs: process.env.SHOCK2_ECHO_LOGS === "1",
    });

    await game.step({ frames: 10 });
    assert.equal(
      (await game.physics.ragdolls()).ragdolls.length,
      0,
      "no ragdolls before anything dies",
    );

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
    // World y where the monster stood, for the fell-through-the-floor check
    // below (medsci1 floors sit at arbitrary world heights).
    const aliveY = monster.position[1];

    await game.entities.sendMessage(monster.id, {
      type: "Damage",
      amount: 1000,
    });

    // The death crumple has to actually play out before the handoff fires
    // (the ragdoll spawns on the crumple's AnimationCompleted, not on the
    // killing blow). Step in 0.5s batches until the ragdoll exists.
    let ragdolls = (await game.physics.ragdolls()).ragdolls;
    for (let i = 0; i < 30 && ragdolls.length === 0; i++) {
      await game.step({ frames: 30 });
      ragdolls = (await game.physics.ragdolls()).ragdolls;
    }
    assert.equal(
      ragdolls.length,
      1,
      "death crumple should spawn exactly one ragdoll within 15s",
    );
    assert.ok(
      ragdolls[0].body_count >= 15,
      `humanoid ragdoll should have a full rig, got ${ragdolls[0].body_count} bodies`,
    );

    // The animated corpse entity is replaced by the ragdoll corpse.
    const after = await game.entities.list({ filter: "OG-Pipe", limit: 50 });
    assert.ok(
      !after.entities.some((e) => e.id === monster.id),
      "the animated corpse entity should be removed once the ragdoll takes over",
    );

    // And the ragdoll settles rather than exploding: wait (up to 20s) for its
    // bulk motion to die down. A corpse can legitimately roll for a few
    // seconds on uneven ground; what must NOT happen is a runaway (speed
    // growing without bound) or a fall through the floor.
    let settled = ragdolls[0];
    for (let i = 0; i < 20 && settled.max_linear_speed >= 0.5; i++) {
      await game.step({ frames: 60 });
      const now = (await game.physics.ragdolls()).ragdolls[0];
      assert.ok(now, "ragdoll still tracked while settling");
      settled = now;
    }
    assert.ok(
      settled.max_linear_speed < 0.5,
      `corpse should settle within 20s, still moving at ${settled.max_linear_speed}`,
    );
    assert.ok(
      settled.min_y > aliveY - 5.0,
      `corpse must not fall through the floor: min_y=${settled.min_y}, alive y=${aliveY}`,
    );
    // ... and stays near where the monster died (catches large-but-finite
    // misbehavior - sliding/launching across the level - that the divergence
    // despawn threshold would not).
    assert.ok(
      settled.max_drift < 15.0,
      `corpse drifted ${settled.max_drift} units from its death spot`,
    );
  },
);

// The killing blow seeds the corpse: a directional lethal hit shoves the
// struck limb along the blow at handoff, so the corpse reacts to HOW it died.
// (Control measurement: a directionless kill leaves max body vx ~0.1; the
// seeded kill reaches ~2.5 - threshold 1.0 discriminates cleanly.)
test(
  "a directional killing blow seeds the ragdoll's reaction",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "medsci1.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8107),
      experimental: ["ragdoll"],
      echoLogs: process.env.SHOCK2_ECHO_LOGS === "1",
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

    // Kill with a world +X blow (what a projectile hit reports).
    await game.entities.sendMessage(monster.id, {
      type: "Damage",
      amount: 1000,
      direction: [1, 0, 0],
    });

    let ragdolls = (await game.physics.ragdolls()).ragdolls;
    for (let i = 0; i < 30 && ragdolls.length === 0; i++) {
      await game.step({ frames: 30 });
      ragdolls = (await game.physics.ragdolls()).ragdolls;
    }
    assert.equal(ragdolls.length, 1, "crumple should hand off to a ragdoll");

    // Sample body velocities right after the handoff: the struck limb must be
    // moving along the blow.
    await game.step({ frames: 3 });
    const bodies = await game.physics.bodies({
      entityId: ragdolls[0].entity_id,
    });
    const maxVx = Math.max(...bodies.bodies.map((b) => b.velocity[0]));
    assert.ok(
      maxVx > 1.0,
      `struck limb should move along the +X blow, max vx=${maxVx}`,
    );
  },
);
