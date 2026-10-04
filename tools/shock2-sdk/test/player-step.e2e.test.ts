import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";

test("diagonal walking climbs Earth's exposed Basic Training tread", {
  skip: process.env.SHOCK2_E2E !== "1", timeout: 120_000,
}, async () => {
  await using game = await GameServer.launch({ mission: "earth.mis", port: 0 });
  await game.step({ frames: 360 });
  // Reproduce the authored post-lesson state: trap 300 removes fields 548/566.
  const [trap] = await game.entities.byTemplate(300);
  assert.ok(trap, "Basic Training Destroy Trap 300 must exist");
  await game.entities.sendMessage(trap.id, { type: "TurnOn" });
  await game.step({ frames: 2 });
  assert.equal((await game.entities.list({ filter: "BlueForceField" })).entities.length, 0);

  await game.player.teleport({ x: 6.6, y: 22.2, z: 239.6 });
  await game.step({ frames: 30 });
  const start = await game.player.position();
  await game.input.set("head.look", [-63.43494882, 0]);
  await game.step({ frames: 1 });
  await game.input.set("right_hand.thumbstick", [0, 1]);
  await game.step({ frames: 20 });
  await game.input.set("right_hand.thumbstick", [0, 0]);
  const end = await game.player.position();
  assert.ok(end.y - start.y > 0.3 && end.z > 240.8,
    `ordinary diagonal walking must climb the tread: ${JSON.stringify({ start, end })}`);
});
