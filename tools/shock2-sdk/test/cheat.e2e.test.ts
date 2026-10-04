import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";

// The `cheat` dev param makes the player invulnerable so deterministic
// captures are not spoiled by damage. Every damage source reaches the player's
// hit points through one applier; these drive two different sources into it.
const e2eEnabled = process.env.SHOCK2_E2E === "1";
// SHODAN's finale entrance, as in shodan-fatal-fall.e2e.test.ts.
const TELEPORT_5_TRIPWIRE = 1354;
const SHODAN_ENTRANCE_SEAT = 776;

async function playerHp(game: GameServer): Promise<number> {
  const hp = (await game.info()).player.hit_points;
  assert.notEqual(hp, null, "the player should have hit points");
  return hp as number;
}

test(
  "cheat: injected damage leaves the player's HP alone, and lands again once off",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({ mission: "earth.mis", port: 0 });
    await game.step({ frames: 10 });
    const player = (await game.info()).player.entity_id;
    assert.ok(player !== null && player !== undefined, "expected a player entity");
    const damage = async () => {
      await game.entities.sendMessage(player, { type: "Damage", amount: 5 });
      await game.step({ frames: 5 });
    };

    const before = await playerHp(game);
    await game.devParams.set("cheat", 1);
    await damage();
    assert.equal(await playerHp(game), before, "cheat on: damage must not lower HP");

    await game.devParams.set("cheat", 0);
    await damage();
    assert.ok((await playerHp(game)) < before, "cheat off: damage must lower HP");
  },
);

test(
  "cheat: a fatal fall leaves the player alive at full HP",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({ mission: "shodan.mis", port: 0 });
    await game.step({ frames: 5 });
    await game.devParams.set("cheat", 1);

    const [tripwire] = await game.entities.byTemplate(TELEPORT_5_TRIPWIRE);
    const [seat] = await game.entities.byTemplate(SHODAN_ENTRANCE_SEAT);
    assert.ok(tripwire && seat, "expected SHODAN's entrance tripwire and seat");
    const [tripX, tripY, tripZ] = tripwire.position;
    await game.player.teleport({ x: tripX, y: tripY, z: tripZ });
    await game.step({ frames: 10 });
    const arrival = await game.player.position();
    const before = await playerHp(game);

    // Walk off the walkway into the central void (fatal without the cheat).
    const [seatX, , seatZ] = seat.position;
    await game.input.lookAtWorldPoint([seatX, arrival.y + 1.6, seatZ + 20]);
    await game.input.set("right_hand.thumbstick", [0, 1]);
    await game.step({ frames: 180 });
    await game.input.set("right_hand.thumbstick", [0, 0]);

    const fallen = await game.player.position();
    assert.ok(fallen.y < arrival.y - 20, `the player must really fall: ${fallen.y}`);
    assert.equal(await playerHp(game), before, "cheat on: the fall must not lower HP");
  },
);
