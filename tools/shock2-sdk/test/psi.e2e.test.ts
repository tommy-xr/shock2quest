import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import { fireOnce } from "./helpers/weapon.js";

// End-to-end test for psi amp casting: the debug_psi scene equips the player
// with the Psi Amp; firing casts the selected psi power. Projectile powers
// (Projected Cryokinesis is the default selection) spawn their PSI-scaled
// projectile and deduct the power's tier from the player's psi pool;
// not-yet-implemented power types are a no-op that spends nothing.
//
// Opt-in (compiles the runtime + needs Data/ assets):
//   npm run test:e2e        (or SHOCK2_E2E=1 node --test dist/test/)
const e2eEnabled = process.env.SHOCK2_E2E === "1";

test(
  "psi amp casts the selected power and drains psi points",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "debug_psi",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8097),
    });

    // The scene auto-equips the Psi Amp on the first update.
    await game.step({ frames: 10 });
    let player = (await game.info()).player;
    assert.ok(player.wielded_entity_id !== null, "psi amp should be auto-wielded");
    assert.equal(
      player.selected_psi_power,
      "Cryokinesis",
      "default selection is Projected Cryokinesis",
    );
    const startPsi = player.psi_points;
    assert.ok(
      startPsi !== null && startPsi > 0,
      `player should start with psi points (got ${startPsi})`,
    );
    assert.equal(player.max_psi_points, 50, "max psi pool comes from The Player template");

    // Cast Cryokinesis (tier 1): one psi point, and the PSI-scaled cryo
    // projectile appears.
    await fireOnce(game);
    await game.step({ frames: 3 });
    player = (await game.info()).player;
    assert.equal(player.psi_points, startPsi! - 1, "tier 1 cast costs one psi point");
    const cryoBolts = (await game.entities.list({ limit: 100 })).entities.filter((e) =>
      e.name?.startsWith("Cryo PSI"),
    );
    assert.ok(cryoBolts.length > 0, "cryokinesis cast spawns a Cryo PSI projectile");

    // Cycle to the next power (Codebreaker, an unimplemented non-projectile
    // type): casting is a no-op and spends nothing.
    await game.input.trigger("CyclePsiPower");
    await game.step({ frames: 2 });
    player = (await game.info()).player;
    assert.equal(player.selected_psi_power, "Codebreaker", "CyclePsiPower advances selection");
    await fireOnce(game);
    player = (await game.info()).player;
    assert.equal(
      player.psi_points,
      startPsi! - 1,
      "casting an unimplemented power spends no psi points",
    );
  },
);
