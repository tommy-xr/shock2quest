import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";

// End-to-end tests for trained-power gating: the player can only select (and
// cast) psi powers they have been trained in. In real missions the trained
// set is seeded from The Player template's learned-power bits (authored empty
// in the retail gamesys) plus the default OSA loadout - Projected Cryokinesis
// only - so `CyclePsiPower` must never move the selection off Cryokinesis.
// Debug scenes (`debug_*`) unlock every power so `debug_psi` keeps exercising
// the whole registry.
//
// Opt-in (compiles the runtime + needs Data/ assets):
//   npm run test:e2e        (or SHOCK2_E2E=1 node --test dist/test/)
const e2eEnabled = process.env.SHOCK2_E2E === "1";

test(
  "debug scenes unlock every psi power (cycling reaches many powers)",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "debug_psi",
    });

    await game.step({ frames: 10 });
    let player = (await game.info()).player;
    assert.equal(player.selected_psi_power, "Cryokinesis", "default selection");

    const seen = new Set([player.selected_psi_power!]);
    for (let i = 0; i < 6; i++) {
      await game.input.trigger("CyclePsiPower");
      await game.step({ frames: 2 });
      player = (await game.info()).player;
      seen.add(player.selected_psi_power!);
    }
    assert.ok(
      seen.size >= 5,
      `cycling in a debug scene should reach many distinct powers (got ${[...seen].join(", ")})`,
    );
  },
);

test(
  "real missions gate CyclePsiPower to trained powers only",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "medsci1.mis",
    });

    await game.step({ frames: 10 });
    let player = (await game.info()).player;
    assert.equal(
      player.selected_psi_power,
      "Cryokinesis",
      "selection starts on the default OSA power",
    );

    // With only Cryokinesis trained, cycling must never leave it.
    for (let i = 0; i < 5; i++) {
      await game.input.trigger("CyclePsiPower");
      await game.step({ frames: 2 });
      player = (await game.info()).player;
      assert.equal(
        player.selected_psi_power,
        "Cryokinesis",
        `cycle ${i + 1}: selection must stay on the only trained power`,
      );
    }
  },
);
