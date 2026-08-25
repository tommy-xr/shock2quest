import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { EntitySummary } from "../src/index.js";

// End-to-end test against a real debug runtime. Requires game assets in
// Data/ and compiles the runtime on first run, so it is opt-in:
//
//   npm run test:e2e        (or SHOCK2_E2E=1 node --test dist/test/)
const e2eEnabled = process.env.SHOCK2_E2E === "1";

// The protocol droid (template -174) carries no L$Weapon to swing with, but
// does link a Corpse "Incendiary Explosion" (intensity 15 over a 4-unit
// radius). Its attack IS that detonation: it spots the player, closes, lights
// a ~1s fuse at melee range and blows up - and the blast damages the player
// standing there. `debug_protocol_droid` puts one 10 units down +Z with
// nothing else in the scene, so the whole sequence runs on plain sight and
// steering (no DebugForceChase).
const DROID_NAME = "Protocol Droid";

async function findDroid(game: GameServer): Promise<EntitySummary | undefined> {
  const { entities } = await game.entities.list({ filter: DROID_NAME, limit: 10 });
  return entities.find((e) => e.name === DROID_NAME);
}

test(
  "Protocol droid: closes on the player, self-destructs, and the blast hurts them",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "debug_protocol_droid",
    });

    await game.step({ frames: 10 });

    const droid = await findDroid(game);
    assert.ok(droid, `expected a ${DROID_NAME} in the debug scene`);
    const startDistance = droid.distance;
    assert.ok(startDistance > 5, `droid should start away from the player`);

    const before = await game.info();
    assert.ok(before.player.hit_points !== null, "player should have a health pool");

    // It spots the player, walks in, and lights the fuse at melee range. (The
    // sim only advances on step, so poll by stepping.)
    let atRange: EntitySummary | undefined;
    for (let i = 0; i < 80 && !atRange; i++) {
      await game.step({ frames: 15 });
      const detail = await game.entities.detail(droid.id);
      const behavior = detail.properties.find((p) => p.name === "AIBehavior")?.value;
      if (behavior === "SelfDestruct") {
        atRange = await findDroid(game);
      }
    }
    assert.ok(atRange, "expected the droid to close and enter SelfDestruct");
    assert.ok(
      atRange.distance < startDistance,
      `expected the droid to approach (${startDistance.toFixed(1)} -> ${atRange.distance.toFixed(1)})`,
    );

    // ...and goes off: the droid is consumed by its own Corpse explosion.
    let detonated = false;
    for (let i = 0; i < 30 && !detonated; i++) {
      await game.step({ frames: 6 });
      detonated = (await findDroid(game)) === undefined;
    }
    assert.ok(detonated, "expected the lit fuse to detonate the droid");

    // The blast is the damage: the player it walked up to must feel it.
    await game.step({ frames: 5 });
    const after = await game.info();
    assert.ok(
      after.player.hit_points! < before.player.hit_points!,
      `expected the detonation to damage the player (${before.player.hit_points} -> ${after.player.hit_points})`,
    );
  },
);
