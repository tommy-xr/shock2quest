import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import { pullTrigger } from "./helpers/weapon.js";

// End-to-end test for Neural Decontamination (`Rad Shield`, issue #1300).
// `debug_psi` carries a hazard patch (a `Rad Burst` radius radiation source)
// beside the player: teleporting into it accumulates radiation, and casting
// Rad Shield must purge the accumulated level and block further exposure for
// the power's duration.
//
// Opt-in (compiles the runtime + needs Data/ assets):
//   npm run test:e2e        (or SHOCK2_E2E=1 node --test dist/test/)
const e2eEnabled = process.env.SHOCK2_E2E === "1";

/** The hazard patch's centre in `debug_psi` (RADIATION_SOURCE_POSITION). */
const HAZARD = { x: 0.0, y: 1.0, z: 9.0 };

/** Select a psi power by name, cycling with `CyclePsiPower`. */
async function selectPower(game: GameServer, name: string): Promise<void> {
  for (let attempt = 0; attempt < 40; attempt += 1) {
    if ((await game.info()).player.selected_psi_power === name) return;
    await game.input.trigger("CyclePsiPower");
    await game.step({ frames: 2 });
  }
  throw new Error(`never reached psi power "${name}"`);
}

test(
  "Rad Shield purges accumulated radiation and blocks new exposure until it expires",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({ mission: "debug_psi" });

    await game.step({ frames: 10 });
    assert.equal(
      (await game.info()).player.radiation_level,
      0,
      "the spawn point is outside the hazard patch's radius",
    );

    // Baseline: standing in the patch accumulates radiation.
    await game.player.teleport(HAZARD);
    await game.step({ frames: 180 });
    const irradiated = (await game.info()).player.radiation_level;
    assert.ok(irradiated > 0, `hazard patch should irradiate the player (got ${irradiated})`);

    await selectPower(game, "Rad Shield");
    const beforePsi = (await game.info()).player.psi_points;
    const beforeHp = (await game.info()).player.hit_points;

    // The psi amp casts on the trigger's rising edge; settle for half a second.
    await pullTrigger(game);
    await game.step({ frames: 30 });
    let player = (await game.info()).player;
    assert.deepEqual(
      player.active_psi_powers,
      ["Rad Shield"],
      "casting Rad Shield activates the sustained power",
    );
    assert.equal(player.psi_points, beforePsi! - 2, "tier 2 cast costs two psi points");
    assert.equal(
      player.radiation_level,
      0,
      "an active Rad Shield purges the accumulated radiation",
    );

    // ...and it stays purged while the player keeps standing in the hazard.
    await game.step({ frames: 300 });
    player = (await game.info()).player;
    assert.equal(player.radiation_level, 0, "Rad Shield blocks new exposure while active");
    assert.ok(player.active_psi_powers.includes("Rad Shield"), "the power is still active");
    assert.equal(player.hit_points, beforeHp, "no radiation damage lands while shielded");

    // Step past expiry (10 + 5 x PSI seconds) in <= 300-frame chunks.
    for (let chunk = 0; chunk < 12; chunk += 1) {
      await game.step({ frames: 300 });
      if (!(await game.info()).player.active_psi_powers.includes("Rad Shield")) break;
    }
    player = (await game.info()).player;
    assert.ok(
      !player.active_psi_powers.includes("Rad Shield"),
      "Rad Shield should expire on its own",
    );

    // Exposure resumes once the shield is gone.
    await game.step({ frames: 300 });
    const afterExpiry = (await game.info()).player.radiation_level;
    assert.ok(
      afterExpiry > 0,
      `radiation should accumulate again after expiry (got ${afterExpiry})`,
    );
  },
);
