import assert from "node:assert/strict";
import { test } from "node:test";
import { Game, GameServer } from "../src/index.js";

/** Stage at the authored chute entrance; cross its real tripwire by normal input. */
export async function verifyGravityAwareChute(game: Game, roundtrip = false): Promise<void> {
  assert.ok((await game.entities.byTemplate(1354)).length > 0,
    "stock Teleport5Tripwire is present at the bottom of the chute");
  await game.player.teleport({ x: 32, y: -30, z: 32 });
  await game.input.set("head.look", [0, 90]);
  await game.input.set("right_hand.thumbstick", [0, 1]);
  let reloaded = false;
  let arrived = false;
  let previousY = -30;
  for (let i = 0; i < 100; i++) {
    // Observe the authored transfer promptly as the player enters the arena.
    await game.step({ frames: previousY < -310 ? 1 : 30 });
    const { player } = await game.info();
    previousY = player.position[1];
    assert.equal(player.life_state, "alive", `died in the authored chute at ${player.position}`);
    if (Math.abs(player.position[2] - 32) > 12 && player.position[1] > -110) {
      arrived = true;
      break;
    }
    if (roundtrip && !reloaded && player.position[1] < -100) {
      const save = `gravity-aware-chute-${Date.now()}`;
      await game.save(save);
      await game.load(save);
      await game.input.set("head.look", [0, 90]);
      await game.input.set("right_hand.thumbstick", [0, 1]);
      reloaded = true;
    }
  }
  await game.input.set("right_hand.thumbstick", [0, 0]);
  assert.ok(arrived, "normal descent must reach the authored tripwire and transfer into the arena");
  if (roundtrip) assert.ok(reloaded, "the regression must reload while still in the shaft");
}

for (const roundtrip of [false, true]) {
  test(`authored low-gravity chute survives${roundtrip ? " with mid-fall save/load" : ""}`, {
    skip: process.env.SHOCK2_E2E !== "1", timeout: 180_000,
  }, async () => {
    await using game = await GameServer.launch({ mission: "shodan.mis" });
    await verifyGravityAwareChute(game, roundtrip);
  });
}
