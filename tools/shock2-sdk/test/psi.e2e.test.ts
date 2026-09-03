import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import { fireOnce } from "./helpers/weapon.js";

// End-to-end test for psi amp casting: the debug_psi scene equips the player
// with the Psi Amp; firing casts the selected psi power. Projectile powers
// (Projected Cryokinesis is the default selection) spawn the projectile
// variant matching the caster's PSI stat and deduct the power's tier from the
// player's psi pool; not-yet-implemented power types are a no-op that spends
// nothing.
//
// Opt-in (compiles the runtime + needs Data/ assets):
//   npm run test:e2e        (or SHOCK2_E2E=1 node --test dist/test/)
const e2eEnabled = process.env.SHOCK2_E2E === "1";

/** The trainer stat cap - the highest PSI the sheet can reach. */
const PSI_STAT = 6;

test(
  "psi amp casts the selected power and drains psi points",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "debug_psi",
    });

    // The scene auto-equips the Psi Amp on the first update.
    await game.step({ frames: 10 });
    // Cast at the stat cap so the tier the cast picks is unambiguous.
    await game.player.setStats({ psionic_ability: PSI_STAT });
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

    // Cast Cryokinesis (tier 1): one psi point, and the cryo projectile for
    // the caster's PSI stat appears.
    await fireOnce(game);
    await game.step({ frames: 3 });
    player = (await game.info()).player;
    assert.equal(player.psi_points, startPsi! - 1, "tier 1 cast costs one psi point");
    const cryoBolts = (await game.entities.list({ limit: 100 })).entities.filter((e) =>
      e.name?.startsWith("Cryo PSI"),
    );
    assert.deepEqual(
      cryoBolts.map((e) => e.name),
      [`Cryo PSI ${PSI_STAT}`],
      "the cast picks the projectile tier matching the player's PSI stat",
    );

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

test(
  "the projectile tier follows the player's PSI stat",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    // The same cast from a sheet raised only to PSI 2 fires the PSI-2 bolt,
    // not the capped one. (`/v1/player/stats` only raises, so the lower stat
    // needs its own runtime.)
    await using game = await GameServer.launch({
      mission: "debug_psi",
    });

    await game.step({ frames: 10 });
    await game.player.setStats({ psionic_ability: 2 });
    assert.equal((await game.info()).player.selected_psi_power, "Cryokinesis");

    await fireOnce(game);
    await game.step({ frames: 3 });
    const cryoBolts = (await game.entities.list({ limit: 100 })).entities.filter((e) =>
      e.name?.startsWith("Cryo PSI"),
    );
    assert.deepEqual(
      cryoBolts.map((e) => e.name),
      ["Cryo PSI 2"],
    );
  },
);
