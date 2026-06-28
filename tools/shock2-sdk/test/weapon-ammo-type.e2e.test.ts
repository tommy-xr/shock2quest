import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";

// End-to-end regression test for ammo-type cycling (InputAction::CycleAmmo). The
// pistol carries three Projectile links (std / he / ap); cycling advances the
// selected one and wraps. Observable headlessly via /v1/info.wielded_ammo_type.
//
// Opt-in (compiles the runtime + needs Data/ assets):
//   npm run test:e2e        (or SHOCK2_E2E=1 node --test dist/test/)
const e2eEnabled = process.env.SHOCK2_E2E === "1";

async function ammoType(game: GameServer): Promise<string | null> {
  return (await game.info()).player.wielded_ammo_type;
}

test(
  "cycling advances the wielded weapon's ammo type and wraps",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "debug_weapons",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8099),
    });

    // Unarmed: no ammo type.
    await game.step({ frames: 5 });
    assert.equal(await ammoType(game), null, "unarmed has no ammo type");

    // Wield the pistol (std / he / ap).
    await game.input.trigger("CycleWeapon");
    await game.step({ frames: 5 });
    assert.equal(await ammoType(game), "std", "pistol starts on its first ammo type");

    // Cycle through all three and wrap back.
    const sequence: string[] = [];
    for (let i = 0; i < 3; i++) {
      await game.input.trigger("CycleAmmo");
      await game.step({ frames: 2 });
      sequence.push((await ammoType(game)) ?? "<null>");
    }
    assert.deepEqual(
      sequence,
      ["he", "ap", "std"],
      "cycling advances std -> he -> ap and wraps back to std",
    );
  },
);
