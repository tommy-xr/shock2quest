import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";

const e2eEnabled = process.env.SHOCK2_E2E === "1";

// command2 contains two concrete descendants of the gamesys `electrical
// sparks` source. Its inherited arSrcDesc emits Electricity at intensity 5 in
// a 2.4-world-unit radius immediately, then every 5 seconds. The player's
// Human Vulnerability has an Electricity damage receptron at x1.
test(
  "Periodic stim: electrical sparks damage on their authored five-second cadence",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "command2.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8365),
    });

    // Initialize source scripts while the player is away from the sparks. The
    // birth firing is deliberately not replayed when the player later enters.
    await game.step({ frames: 2 });
    const sparks = (
      await game.entities.list({ filter: "electrical sparks", limit: 20 })
    ).entities.find((entity) => entity.name === "electrical sparks");
    assert.ok(sparks !== undefined, "expected a concrete electrical-sparks source");

    const [x, y, z] = sparks.position;
    // Hold the raw debug fly channel at the value that cancels the fixed
    // character-controller gravity step, keeping the player at the source
    // center instead of dropping to the deck below it.
    await game.input.set("left_hand.thumbstick", [0, 0.5]);
    await game.player.teleport({ x, y, z });
    await game.step({ frames: 1 });
    const hpBefore = (await game.info()).player.hit_points;
    assert.notEqual(hpBefore, null, "player should have a hit-point pool");

    // Four seconds is still before the next authored pulse.
    await game.step({ frames: 240 });
    assert.equal(
      (await game.info()).player.hit_points,
      hpBefore,
      "sparks should not damage continuously between pulses",
    );

    // Crossing five seconds emits Electricity 5 at the source center.
    await game.step({ frames: 90 });
    assert.equal(
      (await game.info()).player.hit_points,
      (hpBefore as number) - 5,
      "the first five-second pulse should deal the authored five damage",
    );

    // The next pulse uses the same cadence rather than firing every frame.
    await game.step({ frames: 240 });
    assert.equal((await game.info()).player.hit_points, (hpBefore as number) - 5);
    await game.step({ frames: 90 });
    assert.equal(
      (await game.info()).player.hit_points,
      (hpBefore as number) - 10,
      "the second five-second pulse should deal another five damage",
    );
  },
);
