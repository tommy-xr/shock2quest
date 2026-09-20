import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import { acquireOsUpgrade } from "./helpers/os-upgrade.js";

const enabled = process.env.SHOCK2_E2E === "1";
test("Pharmo-Friendly acquired at a machine improves psi hypos after transition", { skip: !enabled, timeout: 240_000 }, async () => {
  await using game = await GameServer.launch({ mission: "medsci2.mis" });
  await game.step({ frames: 5 });
  await acquireOsUpgrade(game, "Pharmo-Friendly");
  await game.transitionLevel("debug_psi");
  await game.step({ frames: 10 });
  assert.ok((await game.info()).player.stats!.os_traits.includes(2));
  const before = (await game.info()).player.psi_points!;
  for (let i = 0; i < 25; i++) {
    await game.input.set("right_hand.trigger", 1);
    await game.step({ frames: 3 });
    await game.input.set("right_hand.trigger", 0);
    await game.step({ frames: 3 });
  }
  const depleted = (await game.info()).player.psi_points!;
  assert.equal(depleted, before - 25);
  const hypo = await game.player.spawnItem(-57);
  await game.entities.sendMessage(hypo.entity_id, { type: "Frob" });
  await game.step({ frames: 2 });
  assert.equal((await game.info()).player.psi_points, depleted + 24);
});
