import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import { cycleToWeapon } from "./helpers/weapon.js";

// The debug wall ends at z=15. A bolt centered at z=15.2 clears it,
// although its authored 0.4-unit object-hit sphere overlaps the edge.
test("laser clears a terrain edge but still hits the wall head-on", {
  skip: process.env.SHOCK2_E2E !== "1",
  timeout: 180_000,
}, async () => {
  await using game = await GameServer.launch({ mission: "debug_weapons" });
  await game.step({ frames: 10 });
  await cycleToWeapon(game, entity => entity.template_id === -22, { settleFrames: 10 });
  for (const [z, shouldClear] of [[15.2, true], [14.8, false]] as const) {
    await game.player.teleport({ x: -8, y: 0.9, z });
    await game.input.set("head.look", [0, 0]);
    await game.step({ frames: 5 });
    await game.input.set("right_hand.trigger", 1);
    await game.step({ frames: 1 });
    await game.input.set("right_hand.trigger", 0);
    const [shot] = await game.entities.byTemplate(-2474);
    assert.ok(shot, "normal firing must launch a laser bolt");
    await game.step({ frames: 30 });
    const surviving = (await game.entities.byTemplate(-2474)).find(e => e.id === shot.id);
    if (shouldClear) {
      assert.ok(surviving, "a bolt beside the wall must not impact its edge");
      assert.ok(surviving.position[0] < -12.5, "the bolt must pass the entire wall");
    } else {
      assert.equal(surviving, undefined, "a direct shot must impact the wall");
    }
    await game.step({ frames: 90 });
  }
});
