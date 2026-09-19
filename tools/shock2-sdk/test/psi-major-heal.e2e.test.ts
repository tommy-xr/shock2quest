import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import { selectPsiPower } from "./helpers/psi.js";
import { pullTrigger } from "./helpers/weapon.js";

for (const difficulty of ["easy", "normal", "hard", "impossible"] as const) {
  test(`Major Heal: authored amount, pool and full-health refusal on ${difficulty}`,
    { skip: process.env.SHOCK2_E2E !== "1", timeout: 600_000 }, async () => {
      await using game = await GameServer.launch({ mission: "debug_psi", difficulty });
      await game.step({ frames: 10 });
      const initial = (await game.info()).player;
      assert.equal(initial.difficulty, difficulty);
      // debug_psi provisions PSI 6. Never try to lower a stat with raise-only provisioning.
      const heal = 5 + 5 * 6;
      await game.entities.sendMessage(initial.entity_id!, { type: "Damage", amount: initial.hit_points! - 1 });
      await game.step({ frames: 1 });
      await selectPsiPower(game, "Major Heal");
      const before = (await game.info()).player;
      await pullTrigger(game);
      let after = (await game.info()).player;
      assert.equal(after.hit_points, Math.min(before.max_hit_points!, before.hit_points! + heal));
      assert.equal(after.psi_points, before.psi_points! - 5);
      assert.equal(after.max_psi_points, initial.max_psi_points);
      while (after.hit_points! < after.max_hit_points!) {
        const psi: number = after.psi_points!;
        await pullTrigger(game);
        after = (await game.info()).player;
        assert.equal(after.psi_points, psi - 5);
      }
      const fullPsi = after.psi_points;
      await pullTrigger(game);
      after = (await game.info()).player;
      assert.equal(after.hit_points, after.max_hit_points);
      assert.equal(after.psi_points, fullPsi, "full-health cast must not spend");
    });
}
