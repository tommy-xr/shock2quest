import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { EntitySummary } from "../src/index.js";

// End-to-end test against a real debug runtime. Requires game assets in
// Data/ and compiles the runtime on first run, so it is opt-in:
//
//   npm run test:e2e        (or SHOCK2_E2E=1 node --test dist/test/)
const e2eEnabled = process.env.SHOCK2_E2E === "1";

// earth.mis's shooting-range "Training Droid" (template -4015) shares the
// protocol droid's PropAI("Protocol"), so it takes the same self-destruct
// branch - but it wears the `Docile` metaproperty
// (PropAIAlertCap { max_level: Lowest }), which pins it below the alertness
// levels that reach any attack behavior. It must therefore stand still to be
// shot at, however close the player gets: the training range depends on it.
const DROID_NAME = "Training Droid";

async function listTrainingDroids(game: GameServer): Promise<EntitySummary[]> {
  const { entities } = await game.entities.list({ filter: DROID_NAME, limit: 20 });
  return entities.filter((e) => e.name === DROID_NAME);
}

test(
  "Docile training droids never self-destruct, however close the player stands",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "earth.mis",
    });

    await game.step({ frames: 10 });

    const droids = await listTrainingDroids(game);
    assert.ok(droids.length >= 1, `expected a ${DROID_NAME} in earth.mis`);
    const droid = droids[0];

    const before = await game.info();
    assert.ok(before.player.hit_points !== null, "player should have a health pool");

    // Stand right on top of it - well inside the detonation range a hostile
    // protocol droid would trigger at.
    const [dx, dy, dz] = droid.position;
    await game.player.teleport({ x: dx, y: dy, z: dz - 3.0 });

    // Give it far longer than the ~1s fuse plus the time to escalate.
    for (let i = 0; i < 10; i++) {
      await game.step({ frames: 60 });
    }

    const detail = await game.entities.detail(droid.id);
    const prop = (name: string) => detail.properties.find((p) => p.name === name)?.value;
    assert.equal(prop("AIAlertness"), "Lowest", "Docile caps alertness at Lowest");
    assert.notEqual(prop("AIBehavior"), "SelfDestruct", "a docile droid must not light a fuse");

    const after = await listTrainingDroids(game);
    assert.ok(
      after.some((e) => e.id === droid.id),
      "the training droid must still be standing (it did not blow itself up)",
    );

    const afterInfo = await game.info();
    assert.equal(
      afterInfo.player.hit_points,
      before.player.hit_points,
      "the player must take no damage standing beside a docile training droid",
    );
  },
);
