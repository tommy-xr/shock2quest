import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer, PLAYER_EYE_HEIGHT_WORLD } from "../src/index.js";

const e2eEnabled = process.env.SHOCK2_E2E === "1";

// Read the map page's actual submitted render transform, not the anchor's
// private state. This debug scene consists only of the map and its decals;
// the first object is the full page behind the animated location overlays.
async function page(game: GameServer) {
  const { objects } = await game.scene.objects();
  assert.ok(objects.length > 0, "debug_map must submit a rendered page");
  return objects[0];
}

for (const presentation of ["flat", "vr"] as const) {
  test(
    `debug_map is eye-centered and world-locked in ${presentation}`,
    { skip: !e2eEnabled && "set SHOCK2_E2E=1 to run" },
    async () => {
      await using game = await GameServer.launch({
        mission: "debug_map",
        debugFlags: presentation === "vr" ? ["--vr"] : [],
      });
      await game.step({ frames: 10 });
      const initial = await page(game);
      assert.ok(
        Math.abs(initial.position[1] - PLAYER_EYE_HEIGHT_WORLD) < 0.01,
        `page center ${initial.position[1]} must match eye ${PLAYER_EYE_HEIGHT_WORLD}`,
      );

      // A moderate head glance must change the view, not drag or tilt the map.
      await game.input.set("head.look", [20, 15]);
      await game.step({ frames: 90 });
      const glanced = await page(game);
      assert.deepEqual(glanced.position, initial.position);
      assert.deepEqual(glanced.scale, initial.scale);

      // Moving the tracked eye far enough should re-place the same page at
      // the new height. The old fixed 1.5wu origin cannot satisfy this either.
      await game.input.set("head.look", [0, 0]);
      await game.input.set("head.position", [0, 2.4, 0]);
      await game.step({ frames: 90 });
      const moved = await page(game);
      assert.ok(
        Math.abs(moved.position[1] - 2.4) < 0.01,
        `page center ${moved.position[1]} must follow the relocated tracked eye`,
      );
    },
  );
}
