import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";

// End-to-end test against a real debug runtime. Requires game assets in
// Data/ and compiles the runtime on first run, so it is opt-in:
//
//   npm run test:e2e        (or SHOCK2_E2E=1 node --test dist/test/)
const e2eEnabled = process.env.SHOCK2_E2E === "1";

// Regression for #484: ids exposed as i32 must resolve recycled Shipyard slots.
// Mission loading already recycles slots; newly spawned monsters must remain
// inspectable and receive damage even when their internal generation is nonzero.
test(
  "entities in recycled slots can be messaged and inspected",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "medsci1.mis",
      echoLogs: process.env.SHOCK2_ECHO_LOGS === "1",
    });

    await game.step({ frames: 10 });

    // Spawn a monster in front of the player, identifying it by diffing the
    // entity list across the spawn (the level may contain other OG-Pipes).
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

    for (let round = 1; round <= 2; round++) {
      const monster = await spawnMonster();

      // Detail must resolve the same id the list endpoint handed out.
      const detail = await game.entities.detail(monster.id);
      assert.equal(detail.name, "OG-Pipe", `round ${round}: detail lookup`);

      await game.entities.sendMessage(monster.id, { type: "Damage", amount: 1000 });
      await game.step({ frames: 30 });
      const corpse = await game.entities.detail(monster.id);
      assert.equal(corpse.properties.find((p) => p.name === "AIBehavior")?.value, "Dead",
        `round ${round}: damage should reach the spawned monster`);
    }
  },
);
