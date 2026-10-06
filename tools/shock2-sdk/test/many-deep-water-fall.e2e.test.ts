import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";

const enabled = process.env.SHOCK2_E2E === "1";

for (const reloadDuringFall of [false, true]) {
  test(
    `many: deep stomach-pool fall survives${reloadDuringFall ? " across save/load" : ""}`,
    { skip: !enabled, timeout: 600_000 },
    async () => {
      await using game = await GameServer.launch({ mission: "many.mis", port: 0 });
      await game.step({ frames: 5 });
      // Regression fixture: stage above the authored pool, then let production
      // gravity and water-medium handling perform the entire descent. This is
      // not a campaign-route completion assertion and uses no private save.
      await game.player.teleport({ x: 190.743, y: 38.3, z: 177.5 });
      const hp = (await game.info()).player.hit_points;
      assert.ok(hp !== null && hp > 0);
      await game.step({ frames: 80 });
      const airborne = await game.player.position();
      assert.ok(airborne.y > 18 && airborne.y < 25, JSON.stringify(airborne));
      if (reloadDuringFall) {
        const save = `many_water_fall_${Date.now()}`;
        assert.equal((await game.save(save)).success, true);
        assert.equal((await game.load(save)).success, true);
      }
      // Includes the old fatal threshold, water entry at Y=-5.9, and enough
      // neutral frames afterward to prove that swimming actually catches it.
      await game.step({ frames: 160 });
      const landed = await game.player.position();
      assert.ok(landed.y <= -5.9 && landed.y > -6.3, JSON.stringify(landed));
      assert.equal((await game.info()).player.hit_points, hp);
      await game.step({ frames: 30 });
      const floating = await game.player.position();
      assert.ok(Math.abs(floating.y - landed.y) < 0.05, JSON.stringify(floating));
      assert.equal((await game.info()).player.life_state, "alive");
    },
  );
}
