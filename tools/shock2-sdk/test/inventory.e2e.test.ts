import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";

// End-to-end test for the player-inventory endpoint. Requires game assets in
// Data/ and compiles the runtime on first run, so it is opt-in:
//
//   npm run test:e2e        (or SHOCK2_E2E=1 node --test dist/test/)
//
// Negative-first: before GET /v1/player/inventory existed, the player's carried
// items were internal (Contains links) and unreadable from HTTP - a tester
// could not verify "did the player pick up / hold item X?". The debug_psi scene
// auto-equips the Psi Amp into a hand, giving a deterministic carried item.
const e2eEnabled = process.env.SHOCK2_E2E === "1";

test(
  "inventory: reports carried items with name and location",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "debug_psi",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8102),
    });
    // Let the scene auto-equip the amp.
    await game.step({ frames: 20 });

    const { items, count } = await game.player.inventory();
    assert.equal(count, items.length, "count should match items length");

    const amp = items.find((i) => i.name === "Psi Amp");
    assert.ok(amp, `expected a held 'Psi Amp', got ${JSON.stringify(items)}`);
    assert.ok(
      amp.location === "left_hand" || amp.location === "right_hand",
      `the amp should be hand-held, got location '${amp.location}'`,
    );
    assert.ok(
      Number.isInteger(amp.entity_id),
      "each item should carry its entity id",
    );
  },
);

test(
  "inventory: empty when the player carries nothing",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "medsci1.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8102) + 1,
    });
    await game.step({ frames: 10 });

    // The player starts medsci1 empty-handed; the endpoint returns an empty
    // (not errored) snapshot - a real reading, distinct from "unsupported".
    const { items, count } = await game.player.inventory();
    assert.equal(count, 0, `expected empty inventory, got ${JSON.stringify(items)}`);
  },
);
