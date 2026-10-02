import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer, PLAYER_EYE_HEIGHT_WORLD } from "../src/index.js";
import { teleportVerified } from "./helpers/teleport.js";

const e2eEnabled = process.env.SHOCK2_E2E === "1";

// #1770 (from #1776): this ladder extends through the world ceiling. An
// automatic top-out must not mistake the exterior roof for the upper
// corridor's landing. Fails before the ceiling cap: the climb rises above 42.
test(
  "flat climbing: rick1 Ladder 532 stays below its ceiling",
  { skip: !e2eEnabled, timeout: 600_000 },
  async (t) => {
    await using game = await GameServer.launch({ mission: "rick1.mis" });
    await game.step({ frames: 5 });
    const ladders = await game.entities.byTemplate(532);
    assert.equal(ladders.length, 1, "expected authored Rick1 Ladder 532");
    // Setup only. All subsequent motion uses ordinary forward/jump input.
    await teleportVerified(game, { x: 36.383, y: 33.244, z: -8.520 });
    const start = await game.player.position();
    const ceiling = await game.raycast({
      start: [start.x, start.y, start.z],
      end: [start.x, 45, start.z],
      collision_groups: ["world"],
    });
    assert.ok(ceiling.hit_point && Math.abs(ceiling.hit_point[1] - 42) < 0.01);
    const ceilingY = ceiling.hit_point[1];
    await game.input.lookAtWorldPoint([
      start.x,
      start.y + PLAYER_EYE_HEIGHT_WORLD + 10 * Math.sqrt(3),
      start.z + 10,
    ]);
    await game.input.set("right_hand.thumbstick", [0, 1]);
    let maxY = start.y;
    for (let frames = 0; frames < 184; frames += 8) {
      await game.step({ frames: 8 });
      const position = await game.player.position();
      maxY = Math.max(maxY, position.y);
      assert.ok(
        position.y <= ceilingY + 0.01,
        `player must stay inside below the ceiling, got ${JSON.stringify(position)}`,
      );
    }
    const climbed = await game.player.position();
    assert.ok(
      climbed.y > 39 && climbed.y + 1.2 <= ceilingY + 0.01,
      `ladder must reach the upper corridor with standing headroom, got ${JSON.stringify(climbed)}`,
    );
    // Continued pushing must not eventually find an escape route.
    await game.step({ frames: 120 });
    const held = await game.player.position();
    assert.ok(held.y + 1.2 <= ceilingY + 0.01);
    await game.input.lookAtWorldPoint([
      held.x, held.y + PLAYER_EYE_HEIGHT_WORLD, held.z - 10,
    ]);
    await game.input.setJump(true);
    await game.step({ frames: 20 });
    await game.input.setJump(false);
    await game.input.set("right_hand.thumbstick", [0, 0]);
    await game.step({ frames: 90 });
    const landed = await game.player.position();
    assert.ok(
      Math.abs(landed.y - 40.044) < 0.12 && landed.z < -9.5,
      `jumping south must land on the y=38.8 corridor, got ${JSON.stringify(landed)}`,
    );
    await game.step({ frames: 60 });
    const settled = await game.player.position();
    assert.ok(Math.abs(settled.y - landed.y) < 0.05, "corridor landing must remain supported");
    t.diagnostic(JSON.stringify({ start, climbed, held, maxY, landed, settled }));
  },
);
