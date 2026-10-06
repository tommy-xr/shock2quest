import assert from "node:assert/strict";
import { test } from "node:test";
import { Game, GameServer } from "../src/index.js";

/** Declared fixture: lethal damage tests the authored timer, not combat. */
export async function verifySavedTrapDelay(game: Game): Promise<void> {
  await game.player.teleport({ x: 43.1, y: -91.2, z: 72.2 });
  await game.input.set("right_hand.thumbstick", [0, 0]);
  const heads = await game.entities.byTemplate(298);
  assert.equal(heads.length, 1);
  await game.entities.sendMessage(heads[0].id, { type: "Damage", amount: 1000 });
  await game.step({ frames: 2 });
  assert.equal((await game.entities.byTemplate(298)).length, 0);
  await game.step({ frames: 60 });
  assert.equal((await game.info()).campaign_completed, false);
  const save = `trap-delay-${Date.now()}`;
  await game.save(save);
  await game.load(save);
  // The queued load is applied on the next update; count that frame too.
  await game.step({ frames: 1 });
  assert.equal((await game.entities.byTemplate(298)).length, 0);
  assert.equal((await game.info()).player.life_state, "alive");
  await game.step({ frames: 229 });
  assert.equal((await game.info()).campaign_completed, false,
    "restoring an event must not deliver it immediately");
  let elapsedFrames = 230;
  while (elapsedFrames < 250 && !(await game.info()).campaign_completed) {
    await game.step({ frames: 1 });
    elapsedFrames++;
  }
  const completed = await game.info();
  assert.equal(completed.campaign_completed, true,
    "the pending head death event must survive same-build reload");
  assert.ok(elapsedFrames >= 238 && elapsedFrames <= 245,
    `resume the remaining four seconds, not restart five: ${elapsedFrames}`);
  assert.equal(completed.mission, "enhanced/cs3.ogv");
}

test("pending TrapDelay ending event survives same-build save/load", {
  skip: process.env.SHOCK2_E2E !== "1", timeout: 180_000,
}, async () => {
  await using game = await GameServer.launch({ mission: "shodan.mis" });
  await verifySavedTrapDelay(game);
});
