import assert from "node:assert/strict";
import { test } from "node:test";

import { e2ePort } from "./helpers/e2e-port.js";
import { GameServer } from "../src/index.js";
import { teleportVerified } from "./helpers/teleport.js";

// End-to-end regression for #1137. A destroyed medsci2 Rad Barrel authors a
// Corpse link to the persistent Rad Burst (-1219), whose radius Radiation
// stimulus (-386) reaches The Player's retail `radiate` receptron. The patch
// must subtract 6 from stored RadLevel, consume only on a successful clear,
// and grant no protection against subsequent exposure.
//
// Negative-first on origin/main 83c3a9ea: `RadPatch` resolved to
// UnimplementedScript, the item survived Frob, and no radiation status flow
// existed to change.
const e2eEnabled = process.env.SHOCK2_E2E === "1";

test(
  "medsci2 VR Rad Barrel exposure is cleared by a consumed Rad Patch and resumes",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "medsci2.mis",
      port: e2ePort(),
      repoRoot: process.env.SHOCK2_E2E_REPO_ROOT,
      debugFlags: ["--vr"],
    });
    await game.step({ frames: 5 });

    const barrels = (await game.entities.list({ filter: "Rad Barrel" })).entities;
    assert.ok(barrels.length > 0, "medsci2 should contain an authored Rad Barrel");
    const barrel = barrels[0];
    await teleportVerified(game, {
      x: barrel.position[0] + 1.0,
      y: barrel.position[1] + 0.5,
      z: barrel.position[2],
    });
    await game.entities.sendMessage(barrel.id, { type: "Damage", amount: 100 });
    await game.step({ frames: 24 });

    const exposed = (await game.info()).player.radiation_level;
    assert.ok(exposed > 0, `Rad Burst must accumulate radiation, got ${exposed}`);

    const patch = await game.player.spawnItem("Rad Patch");
    await game.entities.sendMessage(patch.entity_id, { type: "Frob" });
    await game.step({ frames: 2 });
    const cleared = (await game.info()).player.radiation_level;
    assert.ok(cleared < exposed, `Rad Patch must lower RadLevel (${exposed} -> ${cleared})`);
    assert.ok(
      !(await game.player.inventory()).items.some(
        (item) => item.entity_id === patch.entity_id,
      ),
      "a successful Rad Patch use must consume its exact source entity",
    );

    await game.step({ frames: 24 });
    const reexposed = (await game.info()).player.radiation_level;
    assert.ok(
      reexposed > cleared,
      `Rad Patch must not grant a protection duration (${cleared} -> ${reexposed})`,
    );
  },
);
