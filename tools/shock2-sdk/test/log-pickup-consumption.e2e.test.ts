import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";

const e2eEnabled = process.env.SHOCK2_E2E === "1";

// command2's Korenchkin "re: Miracles" log is a physical, world-present
// mission object. Runtime ids change every launch, so discover it by its stable
// mission-object id.
const KORENCHKIN_LOG = 1584;

test(
  "command2: collecting the Korenchkin log consumes its world pickup",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "command2.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8247),
    });
    await game.step({ frames: 5 });

    const [disc] = await game.entities.byTemplate(KORENCHKIN_LOG);
    assert.ok(disc, "command2 should contain Korenchkin's audio-log disc");
    assert.ok(
      (await game.physics.bodies({ entityId: disc.id })).bodies.length > 0,
      "the uncollected disc should have physical world presence",
    );

    await game.entities.sendMessage(disc.id, { type: "Frob" });
    await game.step({ frames: 5 });

    assert.deepEqual(
      (await game.info()).player.collected_logs,
      [{ deck: 6, log: 1 }],
      "frobbing the disc should download its log into the player's PDA state",
    );
    assert.equal(
      (await game.physics.bodies({ entityId: disc.id })).bodies.length,
      0,
      "a collected log must no longer have a frobbable world body",
    );

    // The consumed state is a real mission-state transition, not a transient
    // rendering trick: it must survive rebuilding the mission from a save.
    await game.save("log-pickup-consumption-e2e");
    await game.load("log-pickup-consumption-e2e");
    await game.step({ frames: 5 });
    const [reloadedDisc] = await game.entities.byTemplate(KORENCHKIN_LOG);
    assert.ok(reloadedDisc, "the hidden entity remains as reader backing state");
    assert.equal(
      (await game.physics.bodies({ entityId: reloadedDisc.id })).bodies.length,
      0,
      "the collected disc must stay out of the world after save/load",
    );
  },
);
