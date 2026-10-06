import assert from "node:assert/strict";
import { test } from "node:test";
import { Game, GameServer } from "../src/index.js";

/** Declared fixture: stage a fresh character inside SHODAN's authored 1% shaft. */
export async function verifyRoomGravity(game: Game): Promise<void> {
  const checkGravity = async (expected: number) => {
    const info = await game.info();
    assert.ok(info.player.entity_id !== null);
    const bodies = await game.physics.bodies({ entityId: info.player.entity_id });
    assert.equal(bodies.bodies.length, 1);
    const body = await game.physics.body(bodies.bodies[0].body_id);
    assert.ok(Math.abs(body.gravity_scale - expected) < 1e-6,
      `at ${info.player.position}: expected gravity ${expected}, got ${body.gravity_scale}`);
    assert.equal(info.player.life_state, "alive");
    return info.player.position;
  };
  await game.player.teleport({ x: 32, y: -55, z: 32 });
  await game.input.set("head.look", [0, 90]);
  await game.input.set("right_hand.thumbstick", [0, 1]);
  for (let i = 0; i < 12; i++) {
    await game.step({ frames: 5 });
    await checkGravity(0.01);
  }
  await game.input.set("right_hand.thumbstick", [0, 0]);
  for (let i = 0; i < 30; i++) {
    await game.step({ frames: 1 });
    await checkGravity(0.01);
  }
  const save = `room-gravity-${Date.now()}`;
  await game.save(save);
  await game.load(save);
  await game.input.set("right_hand.thumbstick", [0, 0]);
  await game.step({ frames: 1 });
  await checkGravity(0.01);
  // #2036 separately tracks one movement queued at default gravity on load.
  // Assert occupancy every frame here; preserve its strict motion regression
  // in the dedicated follow-up rather than hiding it with a warmup.
  for (let i = 0; i < 30; i++) {
    await game.step({ frames: 1 });
    await checkGravity(0.01);
  }
  await game.input.set("head.look", [0, -90]);
  await game.input.set("right_hand.thumbstick", [0, 1]);
  for (let i = 0; i < 40; i++) {
    await game.step({ frames: 5 });
    if ((await checkGravity(0.01))[1] > -55) break;
  }
  await game.input.set("right_hand.thumbstick", [0, 0]);
  await game.player.teleport({ x: 50, y: -55, z: 32 });
  await game.step({ frames: 2 });
  await checkGravity(1);
}

test("room gravity survives overlapping volumes and save/load", {
  skip: process.env.SHOCK2_E2E !== "1", timeout: 180_000,
}, async () => {
  await using game = await GameServer.launch({ mission: "shodan.mis" });
  await verifyRoomGravity(game);
});
