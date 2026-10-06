import assert from "node:assert/strict";
import { test } from "node:test";
import { Game, GameServer } from "../src/index.js";

/** Fresh fixture; verify the first queued motion after same-build reload. */
export async function verifySavedPlayerGravity(game: Game): Promise<void> {
  const state = async () => {
    const { player } = await game.info();
    assert.ok(player.entity_id !== null);
    assert.equal(player.life_state, "alive");
    const bodies = await game.physics.bodies({ entityId: player.entity_id });
    assert.equal(bodies.bodies.length, 1);
    const body = await game.physics.body(bodies.bodies[0].body_id);
    assert.ok(Math.abs(body.gravity_scale - 0.01) < 1e-6);
    return player.position;
  };
  await game.player.teleport({ x: 32, y: -55, z: 32 });
  await game.input.set("head.look", [0, 90]);
  await game.input.set("right_hand.thumbstick", [0, 1]);
  await game.step({ frames: 60 });
  await game.input.set("right_hand.thumbstick", [0, 0]);
  await game.step({ frames: 30 });
  const savedPosition = await state();
  const save = `player-gravity-${Date.now()}`;
  await game.save(save);
  await game.load(save);
  await game.input.set("right_hand.thumbstick", [0, 0]);
  await game.step({ frames: 1 });
  const first = await state();
  assert.ok(Math.abs(first[1] - savedPosition[1]) < 1e-5,
    "load must preserve the saved position before its first queued movement");
  for (let frame = 1; frame <= 30; frame++) {
    await game.step({ frames: 1 });
    const position = await state();
    const expectedY = first[1] - frame * 0.002;
    assert.ok(Math.abs(position[1] - expectedY) < 0.0002,
      `queued movement ${frame}: expected Y ${expectedY}, got ${position[1]}`);
  }
}

test("same-build reload preserves gravity for the first queued movement", {
  skip: process.env.SHOCK2_E2E !== "1", timeout: 180_000,
}, async () => {
  await using game = await GameServer.launch({ mission: "shodan.mis" });
  await verifySavedPlayerGravity(game);
});
