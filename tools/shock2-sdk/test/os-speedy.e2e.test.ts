import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import { acquireOsUpgrade } from "./helpers/os-upgrade.js";

// Isolate horizontal movement above the level, without walls or floor friction.
// Every sample starts at the same position with the same fixed simulation time.
async function distance(game: GameServer, stick: [number, number]) {
  await game.input.set("right_hand.thumbstick", [0, 0]);
  await game.player.teleport({ x: 0, y: 100, z: 0 });
  await game.step({ frames: 1 });
  const start = (await game.info()).player.position;
  await game.input.set("right_hand.thumbstick", stick);
  await game.step({ frames: 12 });
  await game.input.set("right_hand.thumbstick", [0, 0]);
  const end = (await game.info()).player.position;
  return Math.hypot(end[0] - start[0], end[2] - start[2]);
}

test("Speedy acquired at a machine increases forward and strafe movement by 15%", {
  skip: process.env.SHOCK2_E2E !== "1", timeout: 240_000,
}, async () => {
  await using game = await GameServer.launch({ mission: "medsci2.mis" });
  await game.step({ frames: 5 });
  const forward = await distance(game, [0, 1]);
  const strafe = await distance(game, [1, 0]);
  assert.ok(forward > 0.1 && strafe > 0.1);
  await acquireOsUpgrade(game, "Speedy");
  assert.ok(Math.abs((await distance(game, [0, 1])) / forward - 1.15) < 0.005);
  assert.ok(Math.abs((await distance(game, [1, 0])) / strafe - 1.15) < 0.005);
  await game.transitionLevel("medsci1.mis");
  assert.ok((await game.info()).player.stats!.os_traits.includes(4));
  assert.ok(Math.abs((await distance(game, [0, 1])) / forward - 1.15) < 0.005);
});
