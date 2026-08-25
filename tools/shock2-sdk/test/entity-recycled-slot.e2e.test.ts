import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";

// End-to-end test against a real debug runtime. Requires game assets in
// Data/ and compiles the runtime on first run, so it is opt-in:
//
//   npm run test:e2e        (or SHOCK2_E2E=1 node --test dist/test/)
const e2eEnabled = process.env.SHOCK2_E2E === "1";

// Regression test for #484: the entity endpoints expose ids as
// `EntityId::inner() as i32`, which drops shipyard's generation bits. The
// handlers used to rebuild the id as a generation-0 handle, so any entity
// living in a recycled slot (generation > 0) could not be messaged or
// inspected. Mission load itself already recycles slots, so a freshly
// spawned monster lands in a generation > 0 slot straight away - on the
// broken build the FIRST sendMessage below fails with "not found or not
// alive". The kill/respawn cycle exercises the same path again across an
// explicit slot free (with --experimental ragdoll the killing blow removes
// the original entity), and the ragdoll count proves each message reached
// its monster.
test(
  "entities in recycled slots can be messaged and inspected",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "medsci1.mis",
      experimental: ["ragdoll"],
      echoLogs: process.env.SHOCK2_ECHO_LOGS === "1",
    });

    await game.step({ frames: 10 });

    // Spawn a monster in front of the player, identifying it by diffing the
    // entity list across the spawn (medsci1 has native OG-Pipes too).
    // `known` tracks live ids only - a spawn may reuse a killed monster's
    // slot, so killed ids must diff as new again.
    const known = new Set(
      (await game.entities.list({ filter: "OG-Pipe", limit: 50 })).entities.map(
        (e) => e.id,
      ),
    );
    const spawnMonster = async () => {
      await game.input.trigger("SpawnDebugMonster");
      await game.step({ frames: 30 });
      const listed = await game.entities.list({ filter: "OG-Pipe", limit: 50 });
      const monster = listed.entities.find(
        (e) => e.name === "OG-Pipe" && !known.has(e.id),
      );
      assert.ok(
        monster,
        `expected a newly spawned OG-Pipe, got ${JSON.stringify(listed.entities.map((e) => e.id))}`,
      );
      known.add(monster.id);
      return monster;
    };

    // Kill the monster and wait until its entity is gone: under
    // --experimental ragdoll the killing blow hands the corpse off to a
    // dedicated ragdoll entity and removes the original, freeing its slot.
    const killMonster = async (id: number) => {
      await game.entities.sendMessage(id, { type: "Damage", amount: 1000 });
      for (let i = 0; i < 20; i++) {
        await game.step({ frames: 15 });
        const listed = await game.entities.list({
          filter: "OG-Pipe",
          limit: 50,
        });
        if (!listed.entities.some((e) => e.id === id)) {
          known.delete(id);
          return;
        }
      }
      assert.fail(`monster ${id} was never removed after the killing blow`);
    };

    for (let round = 1; round <= 2; round++) {
      const monster = await spawnMonster();

      // Detail must resolve the same id the list endpoint handed out.
      const detail = await game.entities.detail(monster.id);
      assert.equal(detail.name, "OG-Pipe", `round ${round}: detail lookup`);

      // The kill message must reach THIS monster: its entity disappears
      // (checked inside killMonster) and exactly one new ragdoll appears.
      const ragdollsBefore = (await game.physics.ragdolls()).ragdolls.length;
      await killMonster(monster.id);
      assert.equal(
        (await game.physics.ragdolls()).ragdolls.length,
        ragdollsBefore + 1,
        `round ${round}: killing blow should hand off to a new ragdoll`,
      );
    }
  },
);
