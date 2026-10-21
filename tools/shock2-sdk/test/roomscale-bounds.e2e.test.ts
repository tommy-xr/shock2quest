import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";

const skip = process.env.SHOCK2_E2E !== "1" && "set SHOCK2_E2E=1 to run";

test("roomscale.bounds round-trips through the input state", { skip }, async () => {
  await using game = await GameServer.launch({ mission: "debug_minimal", debugFlags: ["--vr"] });
  assert.equal((await game.input.state()).roomscale_bounds, null);
  await game.input.set("roomscale.bounds", [2, 1.5]);
  await game.step({ frames: 1 });
  assert.deepEqual((await game.input.state()).roomscale_bounds, [2, 1.5]);
  await game.input.set("roomscale.bounds", null);
  await game.step({ frames: 1 });
  assert.equal((await game.input.state()).roomscale_bounds, null);
});
