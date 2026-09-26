import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";

const enabled = process.env.SHOCK2_E2E === "1";

for (const vr of [false, true]) {
  test(`turret raises both moving parts and closes fully (${vr ? "VR" : "flat"})`,
    { skip: !enabled, timeout: 300_000 }, async () => {
      await using game = await GameServer.launch({
        mission: "debug_turret", debugFlags: vr ? ["--vr"] : [],
      });
      const [turret] = await game.entities.byTemplate(-168);
      assert.ok(turret);
      const [x, y, z] = turret.position;
      await game.player.teleport({ x, y, z: z + 8 });
      await game.step({ frames: 2 });
      const closed = await game.entities.animation(turret.id);
      assert.ok(closed);

      // The debug scene mounts this turret at 180 degrees. Its authored Dark
      // +X forward therefore faces world +X. Old AI watched world -Z instead.
      await game.player.teleport({ x: x + 12, y, z });
      await game.step({ frames: 30 });
      const open = await game.entities.animation(turret.id);
      assert.ok(open);
      for (const index of [1, 2]) {
        const before = closed.joints[index];
        const after = open.joints[index];
        assert.ok(Math.abs(after[1] - before[1] - 0.8) < 0.001,
          `part ${index} must rise the authored two Dark feet`);
        assert.ok(Math.abs(after[0] - before[0]) < 0.001);
        assert.ok(Math.abs(after[2] - before[2]) < 0.001);
      }
      assert.deepEqual(open.joints[0], closed.joints[0], "base stays fixed");

      // Aim off-axis before withdrawing, exercising the return-home phase.
      await game.player.teleport({ x: x + 12, y, z: z + 3 });
      await game.step({ frames: 30 });
      await game.player.teleport({ x: x - 12, y, z });
      await game.step({ frames: 120 });
      const returned = await game.entities.animation(turret.id);
      assert.ok(returned);
      for (const index of [0, 1, 2]) {
        for (let axis = 0; axis < 3; axis++) {
          assert.ok(Math.abs(returned.joints[index][axis] - closed.joints[index][axis]) < 0.001,
            `part ${index} must return to its closed position`);
        }
      }
    });
}
