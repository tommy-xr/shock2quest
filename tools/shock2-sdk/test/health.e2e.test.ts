import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";

// End-to-end test against a real debug runtime. Requires game assets in
// Data/ and compiles the runtime on first run, so it is opt-in:
//
//   npm run test:e2e        (or SHOCK2_E2E=1 node --test dist/test/)
const e2eEnabled = process.env.SHOCK2_E2E === "1";

// medsci1 "Junction Box" is a non-creature prop with PropHitPoints { 3 }, so it
// routes damage through InternalSimpleHealth. It should survive two 1-point
// hits and only be slain (removed from the world) on the third.
const TARGET_NAME = "Junction Box";
const TARGET_HP = 3;

async function findTarget(game: GameServer): Promise<number | undefined> {
  const { entities } = await game.entities.list({
    filter: TARGET_NAME,
    limit: 500,
  });
  return entities.find((e) => e.name === TARGET_NAME)?.id;
}

async function isAlive(game: GameServer, id: number): Promise<boolean> {
  const { entities } = await game.entities.list({
    filter: TARGET_NAME,
    limit: 500,
  });
  return entities.some((e) => e.id === id);
}

test(
  "InternalSimpleHealth: prop survives damage up to its hit points, then dies",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "medsci1.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8092),
    });

    // Let the mission load and initialize scripts.
    await game.step({ frames: 2 });

    const id = await findTarget(game);
    assert.ok(id !== undefined, `expected to find a '${TARGET_NAME}' entity`);
    assert.ok(await isAlive(game, id), "target should start alive");

    // The first (HP - 1) hits are non-lethal: the entity must remain in world.
    for (let hit = 1; hit < TARGET_HP; hit++) {
      await game.entities.sendMessage(id, { type: "Damage", amount: 1.0 });
      await game.step({ frames: 1 });
      assert.ok(
        await isAlive(game, id),
        `target should survive hit ${hit} of ${TARGET_HP} (regression: one-hit-kill)`,
      );
    }

    // The final hit drops hit points to zero and slays the entity, which
    // removes it from the world.
    await game.entities.sendMessage(id, { type: "Damage", amount: 1.0 });
    await game.step({ frames: 1 });
    assert.ok(
      !(await isAlive(game, id)),
      `target should be slain after ${TARGET_HP} hits`,
    );

    // Messaging a now-dead entity is reported as a failure.
    await assert.rejects(
      game.entities.sendMessage(id, { type: "Damage", amount: 1.0 }),
      /not found or not alive/,
    );
  },
);
